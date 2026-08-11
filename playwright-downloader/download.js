#!/usr/bin/env node
/**
 * Low-memory Playwright video downloader
 * Usage: node download.js <video_url> <output_path>
 *
 * Optimized for ~4GB RAM instances:
 * - Single browser, single context
 * - Blocks images / CSS / fonts
 * - Aggressive Chrome flags
 */

const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const url = process.argv[2];
const outputPath = process.argv[3];

if (!url || !outputPath) {
  console.error('Usage: node download.js <video_url> <output_path>');
  process.exit(1);
}

const MAX_SIZE = 50 * 1024 * 1024; // 50 MB Telegram limit

(async () => {
  let browser = null;
  try {
    browser = await chromium.launch({
      headless: true,
      args: [
        '--disable-gpu',
        '--disable-dev-shm-usage',
        '--disable-setuid-sandbox',
        '--no-sandbox',
        '--disable-extensions',
        '--disable-background-networking',
        '--disable-background-timer-throttling',
        '--disable-backgrounding-occluded-windows',
        '--disable-breakpad',
        '--disable-component-extensions-with-background-pages',
        '--disable-features=TranslateUI,BlinkGenPropertyTrees',
        '--disable-ipc-flooding-protection',
        '--disable-renderer-backgrounding',
        '--force-color-profile=srgb',
        '--metrics-recording-only',
        '--mute-audio',
        '--no-default-browser-check',
        '--no-first-run',
        '--disable-hang-monitor',
        '--disable-prompt-on-repost',
        '--disable-sync',
        '--disable-domain-reliability',
        '--disable-client-side-phishing-detection',
        '--disable-component-update',
        '--disable-default-apps',
        '--disable-features=AudioServiceOutOfProcess',
        '--memory-pressure-off',
        '--js-flags=--max-old-space-size=256',
      ],
    });

    const context = await browser.newContext({
      userAgent:
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36',
      viewport: { width: 1280, height: 720 },
      javaScriptEnabled: true,
      ignoreHTTPSErrors: true,
    });

    // Block heavy resources to save RAM & bandwidth
    await context.route('**/*', (route) => {
      const type = route.request().resourceType();
      if (['image', 'stylesheet', 'font', 'media', 'texttrack', 'manifest'].includes(type)) {
        return route.abort();
      }
      return route.continue();
    });

    const page = await context.newPage();

    // Prefer direct navigation to the video URL
    const response = await page.goto(url, {
      waitUntil: 'domcontentloaded',
      timeout: 60000,
    });

    if (!response) {
      throw new Error('No response received');
    }

    const status = response.status();
    if (status >= 400) {
      throw new Error(`HTTP ${status}`);
    }

    const contentType = response.headers()['content-type'] || '';

    // Case 1: Direct video response
    if (contentType.includes('video') || contentType.includes('octet-stream') || url.match(/\.(mp4|webm|mkv|mov|m4v)(\?|$)/i)) {
      const buffer = await response.body();
      if (buffer.length > MAX_SIZE) {
        throw new Error(`File too large: ${(buffer.length / 1024 / 1024).toFixed(2)} MB > 50 MB`);
      }
      if (buffer.length === 0) {
        throw new Error('Empty file');
      }
      fs.writeFileSync(outputPath, buffer);
      console.log(JSON.stringify({ ok: true, size: buffer.length, path: outputPath }));
      return;
    }

    // Case 2: HTML page – try to find a video source
    // Wait a bit for possible JS redirects / players
    await page.waitForTimeout(2000);

    const videoSrc = await page.evaluate(() => {
      const video = document.querySelector('video');
      if (video) {
        if (video.src) return video.src;
        const source = video.querySelector('source');
        if (source && source.src) return source.src;
      }
      // Common player data attributes
      const el = document.querySelector('[data-video], [data-src], [data-mp4]');
      if (el) {
        return el.getAttribute('data-video') || el.getAttribute('data-src') || el.getAttribute('data-mp4');
      }
      return null;
    });

    if (videoSrc) {
      const videoResp = await context.request.get(videoSrc, {
        timeout: 90000,
        headers: {
          Referer: url,
        },
      });
      if (!videoResp.ok()) {
        throw new Error(`Video fetch failed: HTTP ${videoResp.status()}`);
      }
      const buffer = await videoResp.body();
      if (buffer.length > MAX_SIZE) {
        throw new Error(`File too large: ${(buffer.length / 1024 / 1024).toFixed(2)} MB > 50 MB`);
      }
      fs.writeFileSync(outputPath, buffer);
      console.log(JSON.stringify({ ok: true, size: buffer.length, path: outputPath }));
      return;
    }

    throw new Error('Could not extract video source from page');
  } catch (err) {
    console.error(JSON.stringify({ ok: false, error: err.message }));
    process.exit(1);
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }
  }
})();
