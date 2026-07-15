"""OCR poller: claims label-recognition jobs from the Cloudflare ocr-queue,
runs Qwen2.5-VL on the local llama-swap server, validates with the Atwater
check, and posts the structured per-100g result back.

Runs forever (the container restarts it); one job at a time.

Network robustness (see the 2026-07 wedge post-mortem):
  * Every Cloudflare call is bounded by a HARD SIGALRM deadline, not just the
    `requests` timeout. `requests`/PySocks does NOT reliably honour its read
    timeout on a half-open SOCKS connection (a flaky hysteria/VPN tunnel can
    leave the socket "established" with no bytes and no EOF, hanging `recv`
    forever). The alarm always fires and unwinds the wedged syscall.
  * Egress is ROUND-ROBIN/failover across `EGRESS_PROXIES` (e.g. the v2ray and
    hysteria tunnels). A claim tries each proxy in turn; a dead path is skipped.
  * When a claim fails through ALL egress paths for several sweeps in a row, an
    optional `TUNNEL_RESTART_CMD` bounces the tunnels.
Calls to the local LLM (LLAMA_URL) never use a proxy (they're on the LAN).
"""
import base64
import json
import os
import re
import signal
import subprocess
import time

import requests

QUEUE_URL = os.environ.get("QUEUE_URL", "").rstrip("/")
POLLER_SECRET = os.environ["POLLER_SECRET"]
LLAMA_URL = os.environ.get("LLAMA_URL", "http://192.168.1.17:8080").rstrip("/")


def _parse_queues():
    """One poller can drain SEVERAL queues (e.g. prod + a dev stand) WITHOUT a
    second process — it round-robins the /claim across them and still runs ONE
    job at a time, so GPU/request load does not multiply.

    `QUEUES` = comma/newline-separated `url` or `url|secret` entries (a bare url
    uses POLLER_SECRET). Falls back to the legacy single QUEUE_URL+POLLER_SECRET.
    To go back to prod-only later: drop the extra entry (or the whole QUEUES var)
    and restart the container."""
    raw = os.environ.get("QUEUES", "").strip()
    out = []
    if raw:
        for item in re.split(r"[,\n]", raw):
            item = item.strip()
            if not item:
                continue
            url, secret = item.split("|", 1) if "|" in item else (item, POLLER_SECRET)
            out.append({"url": url.strip().rstrip("/"), "secret": secret.strip()})
    elif QUEUE_URL:
        out.append({"url": QUEUE_URL, "secret": POLLER_SECRET})
    if not out:
        raise SystemExit("no queues configured: set QUEUES or QUEUE_URL")
    return out
VLM_MODEL = os.environ.get("VLM_MODEL", "qwen2.5-vl-32b")
POLL_INTERVAL = float(os.environ.get("POLL_INTERVAL", "3"))
ATWATER_TOLERANCE = float(os.environ.get("ATWATER_TOLERANCE", "15"))  # kcal

# --- Egress: round-robin across one or more proxies ------------------------
# `EGRESS_PROXIES` is a comma-separated list (e.g.
# "socks5h://v2ray:1080,socks5h://hysteria:1080"). An empty entry means DIRECT.
# Falls back to the legacy ALL_PROXY/HTTPS_PROXY single value for back-compat.
def _parse_proxies():
    raw = os.environ.get("EGRESS_PROXIES", "").strip()
    if raw:
        items = [p.strip() for p in raw.split(",")]
    else:
        single = os.environ.get("ALL_PROXY") or os.environ.get("HTTPS_PROXY") or ""
        items = [single.strip()]
    # normalise "" -> None (direct); keep order, drop nothing so an explicit
    # empty slot can mean "also try direct".
    return [(p or None) for p in items] or [None]


PROXIES = _parse_proxies()
QUEUES = _parse_queues()
# After a claim fails through EVERY proxy this many sweeps in a row, bounce the
# tunnels (best-effort). Empty TUNNEL_RESTART_CMD => just keep retrying.
FULL_FAILURES_BEFORE_RESTART = int(os.environ.get("FULL_FAILURES_BEFORE_RESTART", "2"))
TUNNEL_RESTART_CMD = os.environ.get("TUNNEL_RESTART_CMD", "").strip()
RESTART_COOLDOWN = float(os.environ.get("RESTART_COOLDOWN", "10"))

# Hard wall-clock caps (seconds) per network op. These ALWAYS fire (SIGALRM),
# unlike the requests timeout, so a wedged tunnel can never hang the loop.
CLAIM_HARD = int(os.environ.get("CLAIM_HARD", "40"))
IMAGE_HARD = int(os.environ.get("IMAGE_HARD", "150"))
COMPLETE_HARD = int(os.environ.get("COMPLETE_HARD", "40"))
REPORT_HARD = int(os.environ.get("REPORT_HARD", "15"))
# The local LLM call is on the LAN (no proxy) — bound it with a plain requests
# (connect, read-between-chunks) timeout; the read timer resets on each token.
LLM_TIMEOUT = (10, float(os.environ.get("LLM_READ_TIMEOUT", "300")))

# The local LLM must never go through the egress proxy.
LOCAL_PROXIES = {"http": None, "https": None}

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


class HardTimeout(Exception):
    pass


def _on_alarm(signum, frame):
    raise HardTimeout()


signal.signal(signal.SIGALRM, _on_alarm)


class hard_deadline:
    """Wall-clock backstop that ALWAYS fires (SIGALRM), unwinding a wedged
    socket that the `requests`/PySocks read timeout failed to break. Main-thread
    only; the poller loop is single-threaded, and the calls below never nest."""

    def __init__(self, secs):
        self.secs = max(1, int(secs))

    def __enter__(self):
        signal.alarm(self.secs)

    def __exit__(self, *exc):
        signal.alarm(0)
        return False


def _label(proxy):
    return proxy if proxy else "direct"


def _proxies_dict(proxy):
    return {"http": proxy, "https": proxy}


def claim(queue, proxy):
    r = requests.post(
        f"{queue['url']}/claim",
        headers={"Authorization": f"Bearer {queue['secret']}"},
        proxies=_proxies_dict(proxy),
        timeout=(10, 25),
    )
    r.raise_for_status()
    data = r.json()
    return data if data.get("job_id") else None


def get_image_b64(queue, job_id, proxy):
    r = requests.get(
        f"{queue['url']}/image/{job_id}",
        headers={"Authorization": f"Bearer {queue['secret']}"},
        proxies=_proxies_dict(proxy),
        timeout=(10, 120),
    )
    r.raise_for_status()
    return r.text


def complete(queue, job_id, proxy, result=None, error=None):
    body = {"job_id": job_id}
    if error is not None:
        body["error"] = str(error)
    else:
        body["result"] = result
    r = requests.post(
        f"{queue['url']}/complete",
        headers={"Authorization": f"Bearer {queue['secret']}"},
        json=body,
        proxies=_proxies_dict(proxy),
        timeout=(10, 25),
    )
    r.raise_for_status()


def report(queue, job_id, proxy, phase, thinking_tokens, answer_tokens):
    """Push the live LLM phase + token counts to the queue (best-effort)."""
    try:
        with hard_deadline(REPORT_HARD):
            requests.post(
                f"{queue['url']}/progress",
                headers={"Authorization": f"Bearer {queue['secret']}"},
                json={"job_id": job_id, "phase": phase,
                      "thinking_tokens": thinking_tokens, "answer_tokens": answer_tokens},
                proxies=_proxies_dict(proxy),
                timeout=(5, 10),
            )
    except Exception as e:
        print(f"progress report failed: {e}", flush=True)


def recognize(queue, job_id, image_blob, custom_nutrients, proxy):
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
    # LOCAL call — no proxy, plain requests timeout is enough (LAN sockets honour it).
    r = requests.post(f"{LLAMA_URL}/v1/chat/completions", json=body, proxies=LOCAL_PROXIES, timeout=LLM_TIMEOUT, stream=True)
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
            report(queue, job_id, proxy, phase, tt, at)
            last_report = now
    report(queue, job_id, proxy, phase or "answer", tt, at)

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


class AllEgressFailed(Exception):
    pass


def poll_once(queue, start_idx):
    """Claim from `queue` trying each proxy starting at `start_idx` (round-robin)
    until one is REACHABLE. Returns (job_or_None, proxy_used). A reachable-but-
    empty queue is success (proves the tunnel works). Raises AllEgressFailed if
    EVERY proxy errors this sweep."""
    errs = []
    n = len(PROXIES)
    for k in range(n):
        proxy = PROXIES[(start_idx + k) % n]
        try:
            with hard_deadline(CLAIM_HARD):
                job = claim(queue, proxy)
            return job, proxy
        except Exception as e:
            errs.append(f"{_label(proxy)}: {e}")
    raise AllEgressFailed("; ".join(errs))


def restart_tunnels():
    if not TUNNEL_RESTART_CMD:
        print("all egress paths down; TUNNEL_RESTART_CMD unset — will keep retrying", flush=True)
        return
    print(f"all egress paths down; restarting tunnels: {TUNNEL_RESTART_CMD}", flush=True)
    try:
        subprocess.run(TUNNEL_RESTART_CMD, shell=True, timeout=90, check=False)
    except Exception as e:
        print(f"tunnel restart command failed: {e}", flush=True)
    time.sleep(RESTART_COOLDOWN)


def main():
    print(
        f"poller up: queues={[q['url'] for q in QUEUES]} model={VLM_MODEL} "
        f"llama={LLAMA_URL} egress={[_label(p) for p in PROXIES]}",
        flush=True,
    )
    qidx = 0   # round-robin over QUEUES (one claim per cycle → no extra load)
    pidx = 0   # round-robin start over PROXIES
    full_failures = 0
    while True:
        queue = QUEUES[qidx % len(QUEUES)]
        try:
            job, proxy = poll_once(queue, pidx)
            full_failures = 0
        except AllEgressFailed as e:
            full_failures += 1
            print(f"claim {queue['url']}: all egress failed (sweep {full_failures}): {e}", flush=True)
            if full_failures >= FULL_FAILURES_BEFORE_RESTART:
                restart_tunnels()
                full_failures = 0
            qidx += 1
            pidx += 1
            time.sleep(POLL_INTERVAL)
            continue
        qidx += 1
        pidx += 1

        if not job:
            time.sleep(POLL_INTERVAL)
            continue

        job_id = job["job_id"]
        print(f"claimed {job_id} from {queue['url']} via {_label(proxy)}", flush=True)
        try:
            with hard_deadline(IMAGE_HARD):
                image_b64 = get_image_b64(queue, job_id, proxy)
            result = recognize(queue, job_id, image_b64, job.get("custom_nutrients", []), proxy)
            with hard_deadline(COMPLETE_HARD):
                complete(queue, job_id, proxy, result=result)
            print(f"done {job_id}: {json.dumps(result, ensure_ascii=False)[:200]}", flush=True)
        except Exception as e:
            print(f"job {job_id} failed: {e}", flush=True)
            try:
                with hard_deadline(COMPLETE_HARD):
                    complete(queue, job_id, proxy, error=str(e))
            except Exception as e2:
                print(f"could not report failure for {job_id}: {e2}", flush=True)


if __name__ == "__main__":
    main()
