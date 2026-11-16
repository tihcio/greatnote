// Schermate della disposizione mobile (npm run ui:screens:mobile).
//
// Stesso banco di prova di screens.mjs (backend simulato, dati sintetici),
// aperto con `?mobile` a larghezza di telefono. Non verifica nulla: salva le
// viste in target/ui-screens/mobile/. Il comportamento reale su Android va
// comunque provato su emulatore o dispositivo.

import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = resolve(root, "target/ui-screens/mobile");
const port = 1437;
const harness = `http://127.0.0.1:${port}/test-harness.html?mobile`;
mkdirSync(output, { recursive: true });

const server = await createServer({
  configFile: resolve(root, "frontend/vite.config.ts"),
  server: { port, strictPort: true, host: "127.0.0.1" },
  logLevel: "error",
});
await server.listen();
const browser = await puppeteer.launch({
  browser: "firefox",
  executablePath: process.env.FIREFOX ?? "/usr/bin/firefox",
  headless: true,
  timeout: 120_000,
  env: { ...process.env, LIBVA_DRIVER_NAME: "none" },
  extraPrefsFirefox: {
    "intl.accept_languages": "it-IT, it",
    "intl.locale.requested": "it-IT",
    "media.hardware-video-decoding.enabled": false,
    "gfx.webrender.software": true,
  },
});

const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const page = await browser.newPage();
await page.setViewport({ width: 412, height: 915 });

const clickText = async (selector, text) => {
  for (const handle of await page.$$(selector)) {
    if ((await handle.evaluate((element) => element.textContent ?? "")).includes(text)) {
      await handle.click();
      return;
    }
  }
  throw new Error(`elemento non trovato: ${selector} «${text}»`);
};
const shot = async (name) => {
  await pause(400);
  await page.screenshot({ path: `${output}/${name}.png` });
  console.log(`${name}.png`);
};
const openDrawer = async () => {
  await page.click(".mobile-bar-button");
  await pause(300);
};

try {
  await page.goto(harness, { waitUntil: "load" });
  await pause(1000);
  await shot("home");
  await openDrawer();
  await shot("navigazione");
  await clickText(".navigation-item", "Tutte le note");
  await shot("elenco");
  await page.click(".note-list-item");
  await shot("nota");
  await page.click(".mobile-bar-button");
  await shot("elenco-dopo-indietro");
  for (const [name, label] of [
    ["attivita", "Attività"],
    ["calendario", "Calendario"],
    ["promemoria", "Promemoria"],
  ]) {
    await openDrawer();
    await clickText(".navigation-item", label);
    await shot(name);
  }
  await openDrawer();
  await page.click(".sidebar-data-button");
  await shot("impostazioni");
} finally {
  await browser.close();
  await server.close();
}
