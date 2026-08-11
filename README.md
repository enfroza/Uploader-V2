# Uploader-V2

Telegram bot written in Rust that downloads:

1. **TikTok** links → watermark-free MP4 (pure Rust via TikWM)
2. **Direct video URLs** → tries fast HTTP, falls back to **yt-dlp**

Optimized for low-RAM VPS (concurrency limited to 1).

---

## Features

* TikTok watermark-free download
* Direct `.mp4` / `.webm` / etc. support
* Automatic **yt-dlp** fallback when HTTP gets blocked (403)
* Telegram 50 MB limit enforced
* Strict temp-file cleanup
* Low memory usage

---

## Requirements

- Rust
- `yt-dlp`
- `ffmpeg` (recommended)

---

## Setup (Ubuntu)

```bash
# Install yt-dlp
sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp
sudo chmod a+rx /usr/local/bin/yt-dlp

# Optional but recommended
sudo apt install -y ffmpeg

# Project
cd ~/Uploader-V2   # or clone it
cp .env.example .env
nano .env          # put your TELOXIDE_TOKEN

cargo build --release
./target/release/uploader-v2
```

---

## Environment

```env
TELOXIDE_TOKEN=your_bot_token

# Optional
DIRECT_VIDEO_COOKIES=cf_clearance=...
YTDLP_COOKIES=/path/to/cookies.txt
YTDLP_ARGS=--impersonate chrome
```

---

## How it works

1. You send a TikTok or direct video link
2. For direct URLs the bot first tries a normal HTTP download
3. If it fails (403 etc.) it automatically runs `yt-dlp`
4. Video is uploaded to Telegram and temp files are deleted

---

## Notes

* yt-dlp often works better than pure HTTP / Playwright on Cloudflare sites
* You can pass extra flags with `YTDLP_ARGS`
* For some sites a cookies.txt (exported from browser) helps
