# Uploader-V2

A lightweight, high-performance Telegram bot written in **100% pure Rust**.

Supports:

1. **TikTok links** → watermark-free MP4 via TikWM API  
2. **Direct video URLs** (`.mp4`, `.webm`, etc.) → download and upload to Telegram  

Example direct URL that works:

```
https://jilhub.org/contents/videos/11000/11058/11058.mp4
```

This version is completely free of Python, `yt-dlp`, and any external sub-processes. Idle memory usage is typically ~15–30 MB.

---

## Key Features

* **100% Pure Rust**: `tokio` + `teloxide` + `reqwest` only.
* **TikTok watermark-free extraction** via public TikWM API.
* **Direct video download** for any public HTTP(S) media link (`.mp4`, `.webm`, `.mkv`, `.mov`, `.m4v` or paths containing `/videos/`).
* **Low-RAM protection**: `tokio::sync::Semaphore` limits concurrent downloads to 2.
* **Telegram 50 MB safeguard** (checked via Content-Length when available + final size check).
* **Strict temp-file cleanup** with UUID filenames.
* **60-second HTTP timeouts** on all network operations.
* Follows redirects (up to 10) so signed/CDN links still work.

---

## Project Structure

```text
Uploader-V2/
├── Cargo.toml
├── .env.example
├── .gitignore
└── src/
    ├── main.rs          # Bot entry point & routing
    ├── telegram.rs      # TikTok + direct-URL handlers
    └── downloader.rs    # HTTP client (TikWM + direct downloads)
```

---

## Setup

```bash
cp .env.example .env
# Edit .env and put your Telegram bot token
cargo run --release
```

Required environment variable:

```
TELOXIDE_TOKEN=your_telegram_bot_token_here
```

(or `TELEGRAM_BOT_TOKEN`)

---

## Usage

Send the bot either:

* A TikTok share link, **or**
* A direct video URL (must start with `http://` or `https://` and end with a video extension / contain `/videos/`)

The bot replies with the video file (or an error message if the file exceeds 50 MB or the download fails).

---

## Notes

* Direct downloads rely on the remote server allowing the bot’s User-Agent and not requiring cookies/auth.
* Some hosts block HEAD requests — the bot falls back to a full GET and still enforces the size limit after download.
* All temporary files are deleted immediately after upload (or on error).
