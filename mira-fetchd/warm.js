/*
  mira-fetchd warm-up helper

  Launches a persistent (cookie-saving) Chromium context in *headful* mode so you
  can manually clear Cloudflare / log in once. Then you close the window and the
  cookies remain in the profile directory used by the normal headless daemon.

  Usage:
    cd mira-fetchd
    MIRA_FETCHD_PROFILE_DIR=... node warm.js https://platform.openai.com/docs/overview
*/

const path = require('path');
const fs = require('fs');

const { chromium } = require('playwright');

const PROFILE_DIR = process.env.MIRA_FETCHD_PROFILE_DIR ||
  path.join(process.env.HOME || process.cwd(), '.cache', 'mira-fetchd-profile');

const USER_AGENT = process.env.MIRA_FETCHD_USER_AGENT ||
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123 Safari/537.36';

async function main() {
  const url = process.argv[2];
  if (!url) {
    console.error('usage: node warm.js <url>');
    process.exit(2);
  }

  fs.mkdirSync(PROFILE_DIR, { recursive: true });

  console.log(`Launching headful Chromium with profile: ${PROFILE_DIR}`);
  console.log(`Go clear any challenges / login, then close the browser window.`);

  const ctx = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: false,
    viewport: { width: 1280, height: 800 },
    userAgent: USER_AGENT,
  });

  const page = await ctx.newPage();
  await page.goto(url, { waitUntil: 'load', timeout: 120000 });

  // Keep process alive until the browser is closed.
  // (closing the window should close the persistent context)
  await new Promise((resolve) => ctx.on('close', resolve));
}

main().catch((e) => {
  console.error(String(e && e.stack ? e.stack : e));
  process.exit(1);
});
