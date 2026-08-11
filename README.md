# Uploader-V2

Telegram bot that downloads TikTok videos and **direct video URLs** (including Cloudflare-protected sites).

- TikTok → pure Rust (TikWM API)
- Direct URLs → tries fast HTTP first, falls back to **low-memory Playwright** on 403

Optimized for **4 GB RAM** instances (max 1 concurrent download).

---

## Features

* TikTok watermark-free download (pure Rust)
* Direct `.mp4` / `.webm` / etc. support
* Automatic Playwright fallback for Cloudflare 403s
* Memory-conscious (concurrency = 1, heavy Chromium flags, resource blocking)
* Telegram 50 MB limit enforced
* Strict temp-file cleanup

---

## Requirements

- Rust (1.75+)
- Node.js 18+
- ~1 GB free RAM while downloading

---

## Setup

```bash
# 1. Clone / enter project
cd Uploader-V2

# 2. Install Node dependencies + Chromium
cd playwright-downloader
npm install
npx playwright install chromium
cd ..

# 3. Configure bot token
cp .env.example .env
# Edit .env → put your TELOXIDE_TOKEN

# 4. Run
cargo run --release
```

---

## Environment variables

```env
TELOXIDE_TOKEN=your_bot_token_here
RUST_LOG=info

# Optional
DIRECT_VIDEO_COOKIES=cf_clearance=...
PLAYWRIGHT_SCRIPT=./playwright-downloader/download.js
```

---

## How it works

1. You send a TikTok link or a direct video URL
2. For direct URLs the bot first tries a normal HTTP download (very light)
3. If the site returns **403 Forbidden**, it automatically launches a memory-optimized Playwright (Chromium) instance, downloads the video, then closes the browser
4. Video is uploaded to Telegram and temp files are deleted

Only **one** download runs at a time to stay safe on 4 GB RAM.

---

## Memory usage (approximate)

| State                    | RAM        |
|--------------------------|------------|
| Idle (Rust only)         | 20–40 MB   |
| HTTP download            | 40–80 MB   |
| Playwright download      | 450–700 MB |
| Peak (safe)              | < 1 GB     |

---

## Notes

* Playwright is only used when normal HTTP gets blocked.
* Chromium is closed immediately after each download.
* For best results on heavily protected sites, a residential IP still helps, but Playwright solves most Cloudflare challenges.
