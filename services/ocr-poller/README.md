# ocr-poller

On-prem service (runs on the GPU box `october` / 192.168.1.17) that processes
nutrition-label OCR jobs from the Cloudflare `ocr-queue` worker.

```
PWA ──submit img──▶ ocr-queue (CF, DO in weur) ◀──claim/image/complete── ocr-poller ──▶ Qwen2.5-VL (llama-swap :8080)
   ◀──poll status──                                                                       │
                                                                            Atwater check ┘
```

## Run

```sh
cp .env.example .env        # set POLLER_SECRET / QUEUE_URL
docker compose up -d --build
docker compose logs -f poller
```

The poller claims one job at a time, fetches the image, runs `qwen2.5-vl-32b`
on the local llama-swap, validates with the Atwater check
(`4·protein + 9·fat + 4·carbs ≈ kcal`), and posts the per-100g result back.

## Egress via the Italy VPN (v2ray)

By default Cloudflare traffic goes out directly. To route it through an
always-on SOCKS5 (so CF sees the Italy egress):

1. Put your v2ray client config at `./v2ray/config.json` (inbound: SOCKS5 on
   `0.0.0.0:1080`; outbound: your VPN).
2. In `.env` set `ALL_PROXY=socks5h://v2ray:1080`.
3. `docker compose --profile proxy up -d`
