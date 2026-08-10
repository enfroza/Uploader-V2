# TikTok to Telegram Bot

A lightweight, high-performance Telegram bot written in **100% pure Rust**. Converts TikTok links sent in chat into direct, watermark-free MP4 video files. 

This version is completely refactored to eliminate all legacy Python dependencies, `yt-dlp` script invocations, and heavy external sub-processes, resulting in minimal idle memory usage (~15–30 MB RAM).

---

## Key Features

* **100% Pure Rust**: Built using `tokio`, `teloxide`, and `reqwest` with zero Python runtime dependencies or sub-process overhead.
* **Watermark-Free Extraction**: Queries the public TikWM API directly via async HTTP POST requests to fetch clean MP4 streams.
* **Low-RAM Protection**: Integrated `tokio::sync::Semaphore` caps simultaneous video downloads (default: 2) to keep memory usage safe on low-spec VPS hosts (e.g., AWS `t3.micro` 1 GiB instances).
* **Telegram Size Safeguard**: Enforces Telegram’s 50 MB Bot API file limit before attempting heavy downloads.
* **Strict File Cleanup**: Temp download paths use unique UUID filenames and guarantee removal from disk after sending or on error.
* **30-Second Request Timeout**: All HTTP metadata and binary stream downloads are bound by explicit timeouts to prevent hung tasks.

---

## Project Structure

```text
tiktok-to-telegram/
├── .github/
│   └── workflows/
│       └── deploy.yml        # CI/CD GitHub Actions workflow
├── Cargo.toml                # Rust dependencies and configuration
├── .env.example              # Environment variable template
├── .gitignore                # Ignores target/ and local .env files
└── src/
    ├── main.rs               # Entry point, bot setup, and semaphore control
    ├── telegram.rs           # Teloxide update routing & upload handler
    └── downloader.rs         # Async TikWM API client & binary downloader
