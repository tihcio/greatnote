// Schermate dell'interfaccia per la revisione grafica (npm run ui:screens).
//
// Come il test dell'interfaccia usa Vite, frontend/test-harness.html (backend
// simulato con dati sintetici) e Firefox headless, ma non verifica nulla:
// salva le viste principali in tema chiaro e scuro in target/ui-screens/.

import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = resolve(root, "target/ui-screens");
const port = 1436;
const harness = `http://127.0.0.1:${port}/test-harness.html`;
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
await page.setViewport({ width: 1440, height: 900 });

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
  await pause(350);
  await page.screenshot({ path: `${output}/${name}.png` });
  console.log(`${name}.png`);
};

try {
  for (const theme of ["light", "dark"]) {
    await page.goto(harness, { waitUntil: "load" });
    await pause(1000);
    await page.evaluate((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    await shot(`${theme}-home`);
    await clickText(".navigation-item", "Tutte le note");
    await pause(300);
    await page.click(".note-list-item");
    await shot(`${theme}-nota`);
    await clickText(".attachment-image-actions .text-button", "Inserisci nel testo");
    await pause(400);
    await page.hover(".note-editor .note-image-frame");
    await shot(`${theme}-immagine-nel-testo`);
    await clickText(".note-image-tools button", "Annota");
    await pause(400);
    const area = await (await page.$(".annotator-surface")).boundingBox();
    const stroke = async (points) => {
      await page.mouse.move(area.x + area.width * points[0][0], area.y + area.height * points[0][1]);
      await page.mouse.down();
      for (const [x, y] of points.slice(1)) await page.mouse.move(area.x + area.width * x, area.y + area.height * y, { steps: 4 });
      await page.mouse.up();
    };
    await stroke([[0.05, 0.75], [0.3, 0.85], [0.6, 0.8], [0.9, 0.9]]);
    await page.click('.annotator-tool[aria-label="Evidenziatore"]');
    await page.click('.annotator-color[aria-label="#fdd835"]');
    await stroke([[0.04, 0.5], [0.6, 0.5]]);
    await page.click('.annotator-tool[aria-label="Freccia"]');
    await page.click('.annotator-color[aria-label="#1e88e5"]');
    await stroke([[0.9, 0.2], [0.65, 0.45]]);
    await shot(`${theme}-annotazioni`);
    await clickText(".annotator .dialog-submit-button", "Salva annotazioni");
    await pause(300);
    await page.click('.toolbar-dropdown[aria-label="Funzioni AI"]');
    await clickText(".menu-item", "Riassumi");
    await shot(`${theme}-ai-riassunto`);
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "Nota");
    await clickText(".menu-item", "Informazioni");
    await pause(300);
    await clickText(".organization-dialog-actions .dialog-cancel-button", "Cronologia");
    await pause(300);
    await clickText(".history-dialog .segment", "Differenze");
    await shot(`${theme}-cronologia-differenze`);
    await clickText(".history-dialog .segment", "Spiegazione AI");
    await clickText(".history-preview .text-button", "Spiega le differenze");
    await shot(`${theme}-cronologia-spiegazione-ai`);
    await clickText(".history-dialog .dialog-cancel-button", "Chiudi");
    await clickText(".navigation-item", "Attività");
    await shot(`${theme}-attivita`);
    await clickText(".navigation-item", "Assistente AI");
    await shot(`${theme}-assistente`);
    await clickText(".segment", "Cronologia");
    await page.click(".ai-history-summary");
    await shot(`${theme}-cronologia-ai`);
    await clickText(".navigation-item", "Calendario");
    await shot(`${theme}-calendario`);
    await clickText(".calendar-item.external", "Dentista");
    await shot(`${theme}-evento-esterno`);
    await page.keyboard.press("Escape");
    await clickText(".navigation-item", "Promemoria");
    await shot(`${theme}-promemoria`);
    await page.keyboard.down("Control");
    await page.keyboard.press("Comma");
    await page.keyboard.up("Control");
    await shot(`${theme}-impostazioni`);
    await clickText(".settings-nav-item", "Intelligenza artificiale");
    await clickText(".segment", "Claude Code");
    await clickText(".data-action", "Verifica Claude Code");
    await shot(`${theme}-impostazioni-claude-code`);
    await clickText(".settings-nav-item", "Sincronizzazione");
    if (theme === "light") {
      await clickText(".sync-settings .data-action", "Scegli");
      await clickText(".sync-settings .data-action.primary", "Attiva");
      await clickText(".confirm-dialog .dialog-submit-button", "Attiva");
    }
    await shot(`${theme}-impostazioni-sincronizzazione`);
    await clickText(".settings-nav-item", "Integrazioni");
    await shot(`${theme}-impostazioni-integrazioni`);
    await page.evaluate(() => document.querySelector(".settings-content, .settings-panel")?.scrollBy(0, 500));
    await shot(`${theme}-impostazioni-integrazioni-2`);
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "File");
    await shot(`${theme}-menu`);
    await page.keyboard.press("Escape");
  }
} finally {
  await browser.close();
  await server.close();
}
