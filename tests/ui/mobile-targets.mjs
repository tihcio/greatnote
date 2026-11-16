// Dimensione dei controlli toccabili nella disposizione mobile
// (npm run test:ui:mobile-targets).
//
// Apre le viste principali con `?mobile` a larghezza di telefono e misura
// ogni pulsante, collegamento, casella e voce di menu visibile: sotto i
// 44×44 px (la misura minima adottata in docs/ANDROID_PORTING.md) il
// controllo è difficile da toccare. Esce con errore se ne trova.
//
// Escluso il testo della nota (collegamenti e caselle dentro l'editor
// seguono la riga di testo) e i controlli nascosti o fuori dallo schermo.

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";

const MIN = 44;
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const port = 1438;
const harness = `http://127.0.0.1:${port}/test-harness.html?mobile`;

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
    "gfx.webrender.software": true,
  },
});

const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const page = await browser.newPage();
await page.setViewport({ width: 412, height: 915 });

const clickText = async (selector, text) => {
  for (const handle of await page.$$(selector)) {
    if ((await handle.evaluate((element) => element.textContent ?? "")).includes(text)) {
      await handle.evaluate((element) => element.scrollIntoView({ block: "center" }));
      await pause(150);
      await handle.click();
      return;
    }
  }
  throw new Error(`elemento non trovato: ${selector} «${text}»`);
};

/** Controlli visibili più piccoli della misura minima. */
const measure = (min) => {
  const selector = [
    "button",
    "a[href]",
    "summary",
    "select",
    "input[type=checkbox]",
    "input[type=radio]",
    "[role=button]",
    "[role=menuitem]",
    "[role=menuitemcheckbox]",
    "[role=menuitemradio]",
    "[role=tab]",
    "[role=option]",
    "[role=switch]",
  ].join(",");
  const found = [];
  for (const element of document.querySelectorAll(selector)) {
    if (element.closest(".ProseMirror")) continue;
    const box = element.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) continue;
    if (box.bottom <= 0 || box.top >= window.innerHeight || box.right <= 0 || box.left >= window.innerWidth) continue;
    const style = getComputedStyle(element);
    if (style.visibility === "hidden" || style.pointerEvents === "none") continue;
    // Una casella dentro un'etichetta più grande si tocca tramite l'etichetta.
    const label = element.closest("label");
    const target = label ? label.getBoundingClientRect() : box;
    // Mezzo pixel di tolleranza per gli arrotondamenti dell'impaginazione.
    if (target.width >= min - 0.5 && target.height >= min - 0.5) continue;
    const name =
      element.getAttribute("aria-label") ||
      element.getAttribute("title") ||
      (element.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 40);
    const classes = typeof element.className === "string" ? element.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
    found.push(
      `${element.tagName.toLowerCase()}${classes ? `.${classes}` : ""} «${name}» ${Math.round(target.width)}×${Math.round(target.height)}`,
    );
  }
  return found;
};

const problems = new Map();
const check = async (view) => {
  await pause(400);
  // Scorre la vista per misurare anche i controlli più in basso.
  const heights = await page.evaluate(() =>
    [...document.querySelectorAll("*")]
      .filter((element) => element.scrollHeight > element.clientHeight + 4 && getComputedStyle(element).overflowY !== "visible")
      .map((element) => element.scrollHeight),
  );
  const steps = Math.min(6, Math.ceil(Math.max(0, ...heights) / 700));
  for (let step = 0; step <= steps; step += 1) {
    for (const entry of await page.evaluate(measure, MIN)) {
      if (!problems.has(entry)) problems.set(entry, view);
    }
    await page.evaluate((offset) => {
      for (const element of document.querySelectorAll("*")) {
        if (element.scrollHeight > element.clientHeight + 4 && getComputedStyle(element).overflowY !== "visible") {
          element.scrollTop = offset;
        }
      }
    }, (step + 1) * 700);
    await pause(150);
  }
  await page.evaluate(() => {
    for (const element of document.querySelectorAll("*")) element.scrollTop = 0;
  });
};
/** Apre la navigazione, tornando prima all'elenco se è aperta una nota. */
const openDrawer = async () => {
  // Un menu rimasto aperto intercetta il primo tocco: si riprova.
  for (let attempt = 0; attempt < 4; attempt += 1) {
    if (await page.$(".mobile-drawer-open")) return;
    await page.click(".mobile-bar-button");
    await pause(300);
  }
  throw new Error("navigazione non aperta");
};
const escape = async () => {
  await page.keyboard.press("Escape");
  await pause(250);
};

try {
  await page.goto(harness, { waitUntil: "load" });
  await pause(1000);
  await check("home");
  await openDrawer();
  await check("navigazione");
  await clickText(".navigation-item", "Tutte le note");
  await check("elenco");
  await page.click(".note-list-item");
  await check("nota");
  await page.click("[aria-label='Azioni sulla nota']");
  await check("nota: menu");
  await escape();
  await page.click(".toolbar-menu [aria-label='Altro']").catch(() => undefined);
  await check("nota: menu Altro");
  await escape();
  for (const [name, label] of [
    ["attività", "Attività"],
    ["calendario", "Calendario"],
    ["promemoria", "Promemoria"],
  ]) {
    await openDrawer();
    await clickText(".navigation-item", label);
    await check(name);
    if (name === "calendario") {
      for (const mode of ["Settimana", "Giorno"]) {
        await clickText(".segment", mode);
        await check(`calendario: ${mode.toLowerCase()}`);
      }
      await clickText(".subtle-action", "Nuovo evento");
      await check("calendario: nuovo evento");
      await escape();
    }
    if (name === "attività") {
      await page.click(".task-title").catch(() => undefined);
      await check("attività: dettaglio");
      await escape();
    }
  }
  await openDrawer();
  await page.click(".sidebar-data-button");
  await check("impostazioni");
  for (const section of ["Barra laterale", "Note ed editor", "Integrazioni", "Sincronizzazione", "Dati"]) {
    await clickText(".settings-nav-item", section);
    await check(`impostazioni: ${section.toLowerCase()}`);
  }
  await escape();
} catch (error) {
  await page.screenshot({ path: resolve(root, "target/mobile-targets-error.png") });
  throw error;
} finally {
  await browser.close();
  await server.close();
}

if (problems.size === 0) {
  console.log(`Nessun controllo sotto ${MIN}×${MIN} px.`);
} else {
  const byView = new Map();
  for (const [entry, view] of problems) byView.set(view, [...(byView.get(view) ?? []), entry]);
  for (const [view, entries] of byView) {
    console.log(`\n[${view}]`);
    for (const entry of entries) console.log(`  ${entry}`);
  }
  console.log(`\n${problems.size} controlli sotto ${MIN}×${MIN} px.`);
  process.exitCode = 1;
}
