"""OCR poller: claims label-recognition jobs from the Cloudflare ocr-queue,
runs Qwen2.5-VL on the local llama-swap server, validates with the Atwater
check, and posts the structured per-100g result back.

Runs forever (the container restarts it); one job at a time.

Network: all calls to Cloudflare honor standard proxy env vars
(HTTPS_PROXY / ALL_PROXY, e.g. socks5h://v2ray:1080) so the egress can be
pinned to the Italy VPN. Calls to the local LLM (LLAMA_URL) bypass the proxy
(NO_PROXY).
"""
import base64
import json
import os
import re
import time

import requests

QUEUE_URL = os.environ["QUEUE_URL"].rstrip("/")
POLLER_SECRET = os.environ["POLLER_SECRET"]
LLAMA_URL = os.environ.get("LLAMA_URL", "http://192.168.1.17:8080").rstrip("/")
VLM_MODEL = os.environ.get("VLM_MODEL", "qwen2.5-vl-32b")
POLL_INTERVAL = float(os.environ.get("POLL_INTERVAL", "3"))
ATWATER_TOLERANCE = float(os.environ.get("ATWATER_TOLERANCE", "15"))  # kcal

PROMPT = (
    "You are reading a photo of a food package. Nutrition info may be a TABLE or a "
    "sentence in fine print, e.g. «Пищевая ценность (среднее значение) в 100 г: белки – 9,0 г; "
    "жиры – 2,1 г; углеводы – 3,5 г; 68,9 ккал / 290 кДж». Read ALL text on the package, then "
    "extract per-100g nutrition.\n"
    "RU mapping: белки->protein, жиры->fat, углеводы->carbs, энергетическая ценность/калорийность->energy.\n"
    "energy: kcal only (number before «ккал»), ignore kJ. Per-portion -> convert to per 100g. "
    "Do NOT use front-of-pack marketing («11 г белка в упаковке»). If info is absent/illegible, use null.\n"
    "Also fill custom_nutrients for any of these requested keys found on the label: {custom}.\n"
    "product_name: a SHORT name, MAXIMUM 3-4 words. Keep ONLY brand + core product (and a "
    "defining number like fat %). DROP process/marketing/descriptor words such as "
    "ультрапастеризованные, пастеризованные, стерилизованные, питьевые, отборное, «с массовой "
    "долей жира», «среднее значение». NEVER output more than 4 words. "
    "Example: «ВкусВилл Сливки питьевые ультрапастеризованные с массовой долей жира 10%» "
    "-> «ВкусВилл Сливки 10%».\n"
    "Return ONLY JSON: {{\"source_text\":\"<exact nutrition sentence>\",\"product_name\":\"...\","
    "\"energy_kcal\":0,\"protein_g\":0,\"fat_g\":0,\"carbs_g\":0,\"package_weight_g\":0,"
    "\"custom_nutrients\":{{}}}}"
)

# Cloudflare calls go through the proxy; the local LLM must not.
CF_PROXIES = None  # requests reads HTTPS_PROXY/ALL_PROXY from env automatically
LOCAL_PROXIES = {"http": None, "https": None}


def claim():
    r = requests.post(f"{QUEUE_URL}/claim", headers={"Authorization": f"Bearer {POLLER_SECRET}"}, timeout=30)
    r.raise_for_status()
    data = r.json()
    return data if data.get("job_id") else None


def get_image_b64(job_id):
    r = requests.get(f"{QUEUE_URL}/image/{job_id}", headers={"Authorization": f"Bearer {POLLER_SECRET}"}, timeout=120)
    r.raise_for_status()
    return r.text


def complete(job_id, result=None, error=None):
    body = {"job_id": job_id}
    if error is not None:
        body["error"] = str(error)
    else:
        body["result"] = result
    r = requests.post(f"{QUEUE_URL}/complete", headers={"Authorization": f"Bearer {POLLER_SECRET}"}, json=body, timeout=30)
    r.raise_for_status()


def report(job_id, phase, thinking_tokens, answer_tokens):
    """Push the live LLM phase + token counts to the queue (best-effort)."""
    try:
        requests.post(
            f"{QUEUE_URL}/progress",
            headers={"Authorization": f"Bearer {POLLER_SECRET}"},
            json={"job_id": job_id, "phase": phase,
                  "thinking_tokens": thinking_tokens, "answer_tokens": answer_tokens},
            timeout=15,
        )
    except Exception as e:
        print(f"progress report failed: {e}", flush=True)


def recognize(job_id, image_blob, custom_nutrients):
    # The blob is a JSON array of base64 images (front/back of a label).
    try:
        images = json.loads(image_blob)
        if not isinstance(images, list):
            images = [image_blob]
    except (ValueError, TypeError):
        images = [image_blob]

    keys = ", ".join(f'"{c.get("key")}"' for c in custom_nutrients) or "(none)"
    prompt = PROMPT.format(custom=keys)
    parts = [{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64," + img}} for img in images]
    parts.append({"type": "text", "text": prompt})
    body = {
        "model": VLM_MODEL,
        "temperature": 0,
        "stream": True,
        "messages": [{"role": "user", "content": parts}],
    }
    # Stream so we can report thinking/answer progress; reasoning tokens (if the
    # model emits them) arrive in `reasoning_content`, the answer in `content`.
    r = requests.post(f"{LLAMA_URL}/v1/chat/completions", json=body, proxies=LOCAL_PROXIES, timeout=600, stream=True)
    # Surface the server's actual error body (model-not-found, bad request shape,
    # etc.) instead of the opaque "400 Client Error for url" from raise_for_status.
    if r.status_code >= 400:
        raise RuntimeError(f"LLM {r.status_code} from {LLAMA_URL} (model={VLM_MODEL}): {r.text[:500]}")
    answer_parts = []
    tt = at = 0
    phase = None
    last_report = 0.0
    for raw in r.iter_lines(decode_unicode=True):
        if not raw or not raw.startswith("data:"):
            continue
        data = raw[5:].strip()
        if data == "[DONE]":
            break
        try:
            delta = json.loads(data).get("choices", [{}])[0].get("delta", {})
        except (ValueError, IndexError):
            continue
        if delta.get("reasoning_content"):
            tt += 1
            phase = "thinking"
        if delta.get("content"):
            answer_parts.append(delta["content"])
            at += 1
            phase = "answer"
        now = time.time()
        if phase and now - last_report >= 0.7:
            report(job_id, phase, tt, at)
            last_report = now
    report(job_id, phase or "answer", tt, at)

    content = "".join(answer_parts)
    m = re.search(r"\{[\s\S]*\}", content)
    if not m:
        raise ValueError(f"no JSON in model output: {content[:200]}")
    data = json.loads(m.group(0))

    # Atwater sanity check: 4*protein + 9*fat + 4*carbs ≈ kcal. Catches swapped
    # fields / misreads. We report it; the client decides how to surface it.
    e, p, f, c = (data.get(k) for k in ("energy_kcal", "protein_g", "fat_g", "carbs_g"))
    if all(isinstance(x, (int, float)) for x in (e, p, f, c)):
        atwater = 4 * p + 9 * f + 4 * c
        data["atwater_kcal"] = round(atwater, 1)
        data["atwater_ok"] = abs(atwater - e) <= ATWATER_TOLERANCE
    return data


def main():
    print(f"poller up: queue={QUEUE_URL} model={VLM_MODEL} llama={LLAMA_URL}", flush=True)
    while True:
        try:
            job = claim()
        except Exception as e:
            print(f"claim error: {e}", flush=True)
            time.sleep(POLL_INTERVAL)
            continue
        if not job:
            time.sleep(POLL_INTERVAL)
            continue

        job_id = job["job_id"]
        print(f"claimed {job_id}", flush=True)
        try:
            image_b64 = get_image_b64(job_id)
            result = recognize(job_id, image_b64, job.get("custom_nutrients", []))
            complete(job_id, result=result)
            print(f"done {job_id}: {json.dumps(result, ensure_ascii=False)[:200]}", flush=True)
        except Exception as e:
            print(f"job {job_id} failed: {e}", flush=True)
            try:
                complete(job_id, error=str(e))
            except Exception as e2:
                print(f"could not report failure for {job_id}: {e2}", flush=True)


if __name__ == "__main__":
    main()
