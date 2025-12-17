/*
  mira-fetchd

  A tiny localhost sidecar that fetches pages using a REAL browser (Playwright).
  This is the "proper fix" for sites that 403 or require JS/cookies.

  API:
    POST /fetch
      { url, max_chars?, selector?, wait_until?, timeout_ms?, cache_ttl_ms? }

  Response:
      { status, final_url, title, text, truncated, notes[] }
*/

const path = require('path');
const fs = require('fs');
const dns = require('dns').promises;
const net = require('net');

const express = require('express');
const { chromium } = require('playwright');

const HOST = process.env.MIRA_FETCHD_HOST || '127.0.0.1';
const PORT = parseInt(process.env.MIRA_FETCHD_PORT || process.env.PORT || '7337', 10);

const PROFILE_DIR = process.env.MIRA_FETCHD_PROFILE_DIR ||
  path.join(process.env.HOME || process.cwd(), '.cache', 'mira-fetchd-profile');

const DEFAULT_MAX_CHARS = parseInt(process.env.MIRA_FETCHD_MAX_CHARS || '20000', 10);
const DEFAULT_TIMEOUT_MS = parseInt(process.env.MIRA_FETCHD_TIMEOUT_MS || '45000', 10);
const DEFAULT_WAIT_UNTIL = process.env.MIRA_FETCHD_WAIT_UNTIL || 'domcontentloaded';
const DEFAULT_CACHE_TTL_MS = parseInt(process.env.MIRA_FETCHD_CACHE_TTL_MS || '300000', 10); // 5 min

// Very small in-memory cache so we don't repeatedly hammer the same page.
const cache = new Map();

function nowMs() {
  return Date.now();
}

function cacheKey(body) {
  // Keep it intentionally dumb. If you care, pass selector/wait.
  return JSON.stringify({
    url: body.url,
    max_chars: body.max_chars,
    selector: body.selector,
    wait_until: body.wait_until,
  });
}

function isPrivateIPv4(ip) {
  // Assumes ip is a valid dotted IPv4 string.
  const parts = ip.split('.').map((x) => parseInt(x, 10));
  const [a, b] = parts;

  if (a === 10) return true;
  if (a === 127) return true;
  if (a === 0) return true;
  if (a === 169 && b === 254) return true; // link-local
  if (a === 172 && b >= 16 && b <= 31) return true;
  if (a === 192 && b === 168) return true;
  return false;
}

function isPrivateIPv6(ip) {
  const s = ip.toLowerCase();
  if (s === '::1') return true;
  if (s.startsWith('fc') || s.startsWith('fd')) return true; // unique local
  if (s.startsWith('fe80:')) return true; // link-local
  return false;
}

function isPrivateIP(ip) {
  const family = net.isIP(ip);
  if (family === 4) return isPrivateIPv4(ip);
  if (family === 6) return isPrivateIPv6(ip);
  return true;
}

async function assertSafeUrl(rawUrl) {
  let u;
  try {
    u = new URL(rawUrl);
  } catch {
    throw new Error('Invalid URL');
  }

  if (u.protocol !== 'http:' && u.protocol !== 'https:') {
    throw new Error(`Blocked protocol: ${u.protocol}`);
  }

  const host = u.hostname;
  if (!host) throw new Error('Missing hostname');
  if (host === 'localhost' || host.endsWith('.localhost')) {
    throw new Error('Blocked hostname: localhost');
  }

  // If it's an IP literal, block private ranges.
  if (net.isIP(host)) {
    if (isPrivateIP(host)) throw new Error('Blocked private IP');
    return;
  }

  // Best-effort DNS check.
  // Not perfect against DNS rebinding, but it stops obvious SSRF.
  const addrs = await dns.lookup(host, { all: true });
  for (const a of addrs) {
    if (isPrivateIP(a.address)) {
      throw new Error('Blocked hostname resolving to private IP');
    }
  }
}

function truncateText(text, maxChars) {
  if (!text) return { text: '', truncated: false };
  if (text.length <= maxChars) return { text, truncated: false };
  return {
    text: text.slice(0, maxChars) + `\n\n[Truncated, ${text.length} total chars]`,
    truncated: true,
  };
}

let browserContextPromise = null;

async function getBrowserContext() {
  if (browserContextPromise) return browserContextPromise;

  fs.mkdirSync(PROFILE_DIR, { recursive: true });

  browserContextPromise = chromium.launchPersistentContext(PROFILE_DIR, {
    headless: true,
    viewport: { width: 1280, height: 800 },
    userAgent: process.env.MIRA_FETCHD_USER_AGENT ||
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123 Safari/537.36',
  });

  return browserContextPromise;
}

async function fetchWithBrowser(body) {
  const url = body.url;
  const maxChars = Number.isFinite(body.max_chars) ? body.max_chars : DEFAULT_MAX_CHARS;
  const selector = typeof body.selector === 'string' && body.selector.trim() ? body.selector.trim() : null;
  const waitUntil = typeof body.wait_until === 'string' && body.wait_until.trim() ? body.wait_until.trim() : DEFAULT_WAIT_UNTIL;
  const timeoutMs = Number.isFinite(body.timeout_ms) ? body.timeout_ms : DEFAULT_TIMEOUT_MS;

  const notes = [];

  await assertSafeUrl(url);

  const ctx = await getBrowserContext();
  const page = await ctx.newPage();

  try {
    // Block heavy stuff.
    await page.route('**/*', (route) => {
      const rt = route.request().resourceType();
      if (rt === 'image' || rt === 'media' || rt === 'font') {
        return route.abort();
      }
      return route.continue();
    });

    const resp = await page.goto(url, { waitUntil, timeout: timeoutMs });

    // Some interstitials need an extra beat.
    await page.waitForTimeout(250);

    const status = resp ? resp.status() : 0;
    const finalUrl = page.url();
    const title = await page.title().catch(() => '');

    let text = '';
    if (selector) {
      try {
        text = await page.$eval(selector, (el) => el.innerText || el.textContent || '');
      } catch (e) {
        notes.push(`selector_failed:${String(e && e.message ? e.message : e)}`);
        text = await page.evaluate(() => document.body ? (document.body.innerText || '') : '');
      }
    } else {
      text = await page.evaluate(() => document.body ? (document.body.innerText || '') : '');
    }

    // Normalize a little.
    text = String(text || '').replace(/\r\n/g, '\n');

    const t = truncateText(text, maxChars);

    return {
      status,
      final_url: finalUrl,
      title,
      text: t.text,
      truncated: t.truncated,
      notes,
    };
  } finally {
    await page.close().catch(() => {});
  }
}

const app = express();
app.use(express.json({ limit: '256kb' }));

app.get('/health', (req, res) => {
  res.json({ ok: true });
});

app.post('/fetch', async (req, res) => {
  const body = req.body || {};
  if (!body.url || typeof body.url !== 'string') {
    return res.status(400).json({ error: 'Missing url' });
  }

  const ttlMs = Number.isFinite(body.cache_ttl_ms) ? body.cache_ttl_ms : DEFAULT_CACHE_TTL_MS;
  const key = cacheKey(body);

  if (ttlMs > 0) {
    const hit = cache.get(key);
    if (hit && (nowMs() - hit.at) < ttlMs) {
      return res.json({ ...hit.value, notes: (hit.value.notes || []).concat(['cache_hit']) });
    }
  }

  try {
    const value = await fetchWithBrowser(body);
    if (ttlMs > 0) {
      cache.set(key, { at: nowMs(), value });
    }
    res.json(value);
  } catch (e) {
    res.status(500).json({
      error: String(e && e.message ? e.message : e),
      url: body.url,
    });
  }
});

app.listen(PORT, HOST, () => {
  console.log(`mira-fetchd listening on http://${HOST}:${PORT}`);
  console.log(`profile: ${PROFILE_DIR}`);
});
