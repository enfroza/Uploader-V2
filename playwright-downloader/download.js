#!/usr/bin/env node
/**
 * Low-memory Playwright video downloader
 * Usage: node download.js <video_url> <output_path>
 *
 * Optimized for ~4GB RAM instances.
 */

const { chromium } = require('playwright');
const fs = require('fs');

const url = process.argv[2];
const outputPath = process.argv[3];

if (!url || !outputPath) {
  console.error('Usage: node download.js <video_url> <output_path>');
  process.exit(1);
}

const MAX_SIZE = 50 * 1024 * 1024; // 50 MB

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

    // ---------- Method 1: Direct request (best for .mp4 links) ----------
    try {
      const origin = new URL(url).origin + '/';
      const resp = await context.request.get(url, {
        timeout: 120000,
        headers: {
          'Accept': '*/*',
          'Accept-Language': 'en-US,en;q=0.9',
          'Referer': origin,
          'Origin': origin.replace(/\/$/, ''),
          'Sec-Fetch-Dest': 'video',
          'Sec-Fetch-Mode': 'no-cors',
          'Sec-Fetch-Site': 'same-origin',
        },
      });

      if (resp.ok()) {
        const buffer = await resp.body();
        if (buffer.length === 0) throw new Error('Empty body');
        if (buffer.length > MAX_SIZE) {
          throw new Error(`File too large: ${(buffer.length / 1024 / 1024).toFixed(2)} MB > 50 MB`);
        }
        fs.writeFileSync(outputPath, buffer);
        console.log(JSON.stringify({ ok: true, size: buffer.length, path: outputPath, method: 'request' }));
        return;
      }
      // If not ok, fall through to page method
      console.error(`Direct request got HTTP ${resp.status()}, trying page method...`);
    } catch (e) {
      console.error(`Direct request failed: ${e.message}, trying page method...`);
    }

    // ---------- Method 2: Page navigation + response listener ----------
    const page = await context.newPage();

    // Capture the main response before it can be closed
    let mainBuffer = null;
    let mainStatus = 0;
    let mainContentType = '';

    page.on('response', async (response) => {
      try {
        if (response.url() === url || response.url().split('?')[0] === url.split('?')[0]) {
          mainStatus = response.status();
          mainContentType = response.headers()['content-type'] || '';
          if (mainStatus >= 200 && mainStatus < 400) {
            mainBuffer = await response.body();
          }
        }
      } catch (_) {
        // ignore body already closed errors on secondary responses
      }
    });

    await page.goto(url, {
      waitUntil: 'commit',   // don't wait for full load
      timeout: 90000,
    });

    // Give the response listener a moment
    await page.waitForTimeout(1500);

    if (mainBuffer && mainBuffer.length > 0) {
      if (mainBuffer.length > MAX_SIZE) {
        throw new Error(`File too large: ${(mainBuffer.length / 1024 / 1024).toFixed(2)} MB > 50 MB`);
      }
      fs.writeFileSync(outputPath, mainBuffer);
      console.log(JSON.stringify({ ok: true, size: mainBuffer.length, path: outputPath, method: 'page-response' }));
      return;
    }

    // ---------- Method 3: Look for <video> src on HTML pages ----------
    await page.waitForTimeout(2000);

    const videoSrc = await page.evaluate(() => {
      const video = document.querySelector('video');
      if (video) {
        if (video.currentSrc) return video.currentSrc;
        if (video.src) return video.src;
        const source = video.querySelector('source');
        if (source && source.src) return source.src;
      }
      const el = document.querySelector('[data-video], [data-src], [data-mp4]');
      if (el) {
        return el.getAttribute('data-video') || el.getAttribute('data-src') || el.getAttribute('data-mp4');
      }
      return null;
    }).catch(() => null);

    if (videoSrc) {
      const origin = new URL(url).origin + '/';
      const videoResp = await context.request.get(videoSrc, {
        timeout: 120000,
        headers: {
          Referer: url,
          Origin: origin.replace(/\/$/, ''),
        },
      });
      if (!videoResp.ok()) {
        throw new Error(`Video source fetch failed: HTTP ${videoResp.status()}`);
      }
      const buffer = await videoResp.body();
      if (buffer.length > MAX_SIZE) {
        throw new Error(`File too large: ${(buffer.length / 1024 / 1024).toFixed(2)} MB > 50 MB`);
      }
      fs.writeFileSync(outputPath, buffer);
      console.log(JSON.stringify({ ok: true, size: buffer.length, path: outputPath, method: 'video-src' }));
      return;
    }
