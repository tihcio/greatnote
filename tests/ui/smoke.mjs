// Test dell'interfaccia con backend simulato (npm run test:ui).
//
// Avvia Vite su una porta dedicata, apre frontend/test-harness.html in
// Firefox headless (WebDriver BiDi, nessun browser scaricato) ed esegue i
// flussi principali: menu, editor, dialog, viste, schede e finestra nota.
// Fallisce se un passaggio non riesce o se la pagina registra errori.
// Gli screenshot finiscono in target/ui-smoke/.
//
// Variabili: FIREFOX (percorso dell'eseguibile, predefinito /usr/bin/firefox).

import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = resolve(root, "target/ui-smoke");
const port = 1435;
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
  // L'avvio può essere lento su alcune macchine (sonda dell'accelerazione
  // video): la decodifica hardware è disattivata e il limite è più ampio.
  timeout: 120_000,
  env: { ...process.env, LIBVA_DRIVER_NAME: "none" },
  extraPrefsFirefox: {
    "intl.accept_languages": "it-IT, it",
    "intl.locale.requested": "it-IT",
    "media.hardware-video-decoding.enabled": false,
    "media.ffmpeg.vaapi.enabled": false,
    "gfx.webrender.software": true,
  },
});

const failures = [];
const errors = [];
const page = await browser.newPage();
await page.setViewport({ width: 1400, height: 900 });
page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(`console: ${message.text()}`);
});

const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const keys = async (...combo) => {
  for (const key of combo) await page.keyboard.down(key);
  for (const key of [...combo].reverse()) await page.keyboard.up(key);
};
const clickText = async (selector, text) => {
  for (const handle of await page.$$(selector)) {
    const value = await handle.evaluate((element) => element.textContent?.trim() ?? "");
    if (value.includes(text)) {
      await handle.click();
      return;
    }
  }
  throw new Error(`elemento non trovato: ${selector} «${text}»`);
};
const hover = async (selector, text) => {
  for (const handle of await page.$$(selector)) {
    if ((await handle.evaluate((element) => element.textContent ?? "")).includes(text)) {
      await handle.hover();
      await pause(250);
      return;
    }
  }
  throw new Error(`elemento non trovato: ${selector} «${text}»`);
};
const expectVisible = async (selector) => {
  if (!(await page.$(selector))) throw new Error(`atteso ${selector}`);
};
const step = async (name, action) => {
  try {
    await action();
    await pause(300);
    await page.screenshot({ path: `${output}/${name}.png` });
    const frame = await page.$eval(".app-frame", (element) => element.className).catch(() => "-");
    console.log(`ok   ${name}   [${frame}]`);
  } catch (error) {
    failures.push(`${name}: ${error.message}`);
    console.log(`FAIL ${name}: ${error.message}`);
    await page.screenshot({ path: `${output}/${name}-errore.png` }).catch(() => undefined);
    await page.keyboard.press("Escape").catch(() => undefined);
  }
};
const openNote = async () => {
  await clickText(".navigation-item", "Tutte le note");
  await pause(300);
  await page.click(".note-list-item");
  await pause(400);
};

try {
  await page.goto(harness, { waitUntil: "load" });
  await pause(1200);

  await step("home", async () => expectVisible(".home-grid"));
  await step("sezioni-barra-laterale-comprimibili", async () => {
    const sections = ["navigation", "recent", "spaces", "notebooks", "tags"];
    for (const section of sections) {
      const toggle = await page.$(`[data-sidebar-section="${section}"]`);
      if (!toggle) throw new Error(`intestazione non trovata: ${section}`);
      const content = await page.$(`#sidebar-section-${section}`);
      if (!content) throw new Error(`contenuto non trovato: ${section}`);
      await toggle.click();
      const collapsed = await toggle.evaluate((button) => button.getAttribute("aria-expanded") === "false");
      const hidden = await content.evaluate((element) => element.hidden);
      if (!collapsed || !hidden) throw new Error(`sezione non compressa: ${section}`);
      if (section === "navigation") {
        await pause(400);
        const saved = await page.evaluate(() => window.__UI_PREFERENCES__?.collapsedSidebarSections ?? []);
        if (!saved.includes("navigation")) throw new Error(`stato non salvato: ${JSON.stringify(saved)}`);
      }
      await toggle.click();
      const expanded = await toggle.evaluate((button) => button.getAttribute("aria-expanded") === "true");
      const visible = await content.evaluate((element) => !element.hidden);
      if (!expanded || !visible) throw new Error(`sezione non espansa: ${section}`);
    }
  });
  await step("menu-file", async () => {
    await clickText(".menu-bar-item", "File");
    await expectVisible(".menu-list");
  });
  await step("menu-visualizza-tema", async () => {
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "Visualizza");
    await hover(".menu-item", "Tema");
    await clickText(".menu-item", "Scuro");
    const theme = await page.evaluate(() => document.documentElement.dataset.theme);
    if (theme !== "dark") throw new Error(`tema non applicato: ${theme}`);
  });
  await step("apri-nota", async () => {
    await openNote();
    await expectVisible(".note-editor");
    await expectVisible(".editor-toolbar");
  });
  await step("tag-nella-barra-in-basso", async () => {
    await page.click(".tag-input");
    await page.keyboard.type("imp");
    await pause(200);
    await expectVisible(".tag-suggestion.highlighted");
    await page.keyboard.press("Enter");
    await page.keyboard.type("nuovo-tag");
    await page.keyboard.press("Enter");
    await pause(300);
    const chips = await page.$$eval(".tag-chip", (items) => items.map((item) => item.textContent?.trim()));
    if (chips.join(",") !== "importante,nuovo-tag") throw new Error(`tag mostrati: ${chips}`);
    await page.click('.tag-chip-remove[aria-label="Togli il tag importante"]');
    await pause(200);
    const left = await page.$$eval(".tag-chip", (items) => items.length);
    if (left !== 1) throw new Error(`tag rimasti: ${left}`);
  });
  await step("recenti-stabili", async () => {
    const order = () =>
      page.$$eval(".recent-note .organization-name", (items) => items.map((item) => item.textContent?.trim()));
    const before = await order();
    if (before.length < 2) throw new Error(`recenti: ${before}`);
    // La nota più in basso, aperta, deve restare al suo posto.
    await clickText(".recent-note", before[before.length - 1]);
    await pause(600);
    const after = await order();
    if (after.join("|") !== before.join("|")) throw new Error(`ordine cambiato: ${before} → ${after}`);
    // I passaggi successivi lavorano sulla prima nota.
    await clickText(".recent-note", before[0]);
    await pause(500);
  });
  await step("immagine-nel-testo-e-ocr", async () => {
    await clickText(".attachment-image-actions .text-button", "Inserisci nel testo");
    await pause(500);
    const loaded = await page.$eval(".note-editor .note-image img", (image) => image.src.startsWith("blob:") && image.naturalWidth > 0);
    if (!loaded) throw new Error("immagine non caricata nel testo");
    await clickText(".attachment-image-actions .text-button", "Leggi il testo");
    await pause(300);
    await clickText(".attachment-ocr-text summary", "Testo letto");
    const text = await page.$eval(".attachment-ocr-text pre", (element) => element.textContent);
    if (text !== "Totale 37,50") throw new Error(`testo letto: ${text}`);
  });
  await step("immagine-ridimensiona-e-annota", async () => {
    const frameWidth = () => page.$eval(".note-editor .note-image-frame", (frame) => Math.round(frame.getBoundingClientRect().width));
    await page.hover(".note-editor .note-image-frame");
    await clickText(".note-image-tools button", "Piccola");
    await pause(200);
    if ((await frameWidth()) !== 240) throw new Error(`larghezza dopo Piccola: ${await frameWidth()}`);
    await page.hover(".note-editor .note-image-frame");
    const handle = await (await page.$(".note-image-resize")).boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(handle.x + 110, handle.y + 10, { steps: 6 });
    await page.mouse.up();
    await pause(200);
    const resized = await frameWidth();
    if (resized < 330 || resized > 370) throw new Error(`larghezza dopo il trascinamento: ${resized}`);

    await page.hover(".note-editor .note-image-frame");
    await clickText(".note-image-tools button", "Annota");
    await pause(500);
    await expectVisible(".annotator .annotator-surface");
    const area = await (await page.$(".annotator-surface")).boundingBox();
    await page.mouse.move(area.x + area.width * 0.2, area.y + area.height * 0.3);
    await page.mouse.down();
    for (const step of [0.3, 0.4, 0.5, 0.6]) {
      await page.mouse.move(area.x + area.width * step, area.y + area.height * (0.3 + step / 4), { steps: 3 });
    }
    await page.mouse.up();
    await page.click('.annotator-tool[aria-label^="Testo"]');
    await page.mouse.click(area.x + area.width * 0.6, area.y + area.height * 0.2);
    await pause(200);
    // Si scrive solo quando il campo della nota ha davvero il focus.
    await page.waitForFunction(() => document.activeElement?.classList.contains("annotator-text-input"), { timeout: 3000 });
    await page.keyboard.type("Scadenza");
    await page.keyboard.press("Enter");
    await pause(200);
    await clickText(".annotator .dialog-submit-button", "Salva annotazioni");
    await pause(400);
    if (await page.$(".annotator")) throw new Error("editor delle annotazioni ancora aperto");
    const overlay = await page.$eval(".note-editor .note-image-annotations", (svg) => ({
      paths: svg.querySelectorAll("path").length,
      text: svg.textContent,
    }));
    if (overlay.paths < 1 || !overlay.text.includes("Scadenza")) throw new Error(`annotazioni nella nota: ${JSON.stringify(overlay)}`);
  });
  await step("colonne-ridimensionabili", async () => {
    const widths = () =>
      page.evaluate(() => ({
        sidebar: Math.round(document.querySelector(".sidebar").getBoundingClientRect().width),
        list: Math.round(document.querySelector(".note-list-panel").getBoundingClientRect().width),
      }));
    const drag = async (selector, dx) => {
      const box = await (await page.$(selector)).boundingBox();
      await page.mouse.move(box.x + box.width / 2, box.y + 200);
      await page.mouse.down();
      await page.mouse.move(box.x + box.width / 2 + dx, box.y + 200, { steps: 8 });
      await page.mouse.up();
      await pause(300);
    };
    const before = await widths();
    await drag(".column-resizer.sidebar-edge", 60);
    await drag(".column-resizer.list-edge", 120);
    const after = await widths();
    if (Math.abs(after.sidebar - before.sidebar - 60) > 3) throw new Error(`barra laterale: ${JSON.stringify([before, after])}`);
    if (Math.abs(after.list - before.list - 120) > 3) throw new Error(`elenco: ${JSON.stringify([before, after])}`);
    const saved = await page.evaluate(() => window.__SMOKE_LOG__.filter((entry) => entry === "set_ui_preferences").length);
    if (saved === 0) throw new Error("larghezze non salvate");
    // Doppio clic: larghezze predefinite.
    for (const selector of [".column-resizer.list-edge", ".column-resizer.sidebar-edge"]) {
      const box = await (await page.$(selector)).boundingBox();
      await page.mouse.click(box.x + box.width / 2, box.y + 200, { count: 2 });
      await pause(300);
    }
    const reset = await widths();
    if (Math.abs(reset.sidebar - before.sidebar) > 3 || Math.abs(reset.list - before.list) > 3) {
      throw new Error(`ripristino: ${JSON.stringify([before, reset])}`);
    }
    await page.mouse.move(1100, 100);
    await pause(200);
    for (const selector of [".column-resizer.sidebar-edge", ".column-resizer.list-edge"]) {
      const resizer = await page.$(selector);
      const idle = await resizer.evaluate((element) => {
        const style = getComputedStyle(element, "::after");
        return { left: style.left, opacity: style.opacity };
      });
      if (idle.opacity !== "0") throw new Error(`${selector}: indicatore visibile senza hover (${idle.opacity})`);
      await resizer.hover();
      await pause(200);
      const hovered = await resizer.evaluate((element) => {
        const style = getComputedStyle(element, "::after");
        return { left: style.left, opacity: style.opacity };
      });
      if (hovered.left !== idle.left) throw new Error(`${selector}: indicatore spostato al passaggio del mouse (${idle.left} → ${hovered.left})`);
      if (Number(hovered.opacity) < 0.65) throw new Error(`${selector}: indicatore non evidenziato (${hovered.opacity})`);
      await page.mouse.move(1100, 100);
      await pause(200);
    }
  });
  await step("menu-nota", async () => {
    await clickText(".menu-bar-item", "Nota");
    await expectVisible(".menu-list");
  });
  await step("inserisci", async () => {
    await page.keyboard.press("Escape");
    await page.click(".toolbar-dropdown");
    await expectVisible(".toolbar-menu .menu-list");
  });
  await step("comandi-rapidi", async () => {
    await page.keyboard.press("Escape");
    await page.click(".note-editor");
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("/");
    await pause(300);
    await expectVisible(".slash-menu");
    await page.keyboard.press("Escape");
  });
  await step("trova-sostituisci", async () => {
    await keys("Control", "KeyH");
    await page.keyboard.type("giorno");
    await pause(200);
    await expectVisible(".search-match.current");
  });
  await step("menu-contestuale", async () => {
    await page.keyboard.press("Escape");
    const item = await page.$(".note-list-item");
    const box = await item.boundingBox();
    await page.mouse.click(box.x + 40, box.y + 20, { button: "right" });
    await pause(400);
    await expectVisible(".context-menu-layer .menu-list");
  });
  await step("info-nota", async () => {
    await page.keyboard.press("Escape");
    await keys("Control", "Shift", "KeyI");
    await expectVisible(".info-grid");
  });
  await step("modifica-tag", async () => {
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "Nota");
    await clickText(".menu-item", "Modifica tag");
    await expectVisible(".picker-list");
  });
  await step("sposta-in", async () => {
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "Nota");
    await clickText(".menu-item", "Sposta in");
    await expectVisible(".picker-list");
  });
  await step("vai-a", async () => {
    await page.keyboard.press("Escape");
    await keys("Control", "KeyQ");
    await page.keyboard.type("spesa");
    await pause(300);
    await expectVisible(".switcher-item");
  });
  await step("impostazioni", async () => {
    await page.keyboard.press("Escape");
    await keys("Control", "Comma");
    await expectVisible(".settings-layout");
  });
  await step("nuova-attivita", async () => {
    await page.keyboard.press("Escape");
    await keys("Alt", "KeyT");
    await pause(400);
    await expectVisible(".task-form");
  });
  await step("schede-e-indietro", async () => {
    await page.keyboard.press("Escape");
    await keys("Alt", "Shift", "KeyT");
    await pause(300);
    const tabs = await page.$$(".tab");
    if (tabs.length < 2) throw new Error("nuova scheda non creata");
    await clickText(".navigation-item", "Modelli");
    await pause(300);
    await page.click('.tab-nav-button[aria-label="Indietro"]');
    await pause(300);
  });
  for (const view of ["Modelli", "File", "Calendario", "Promemoria", "Cestino"]) {
    await step(`vista-${view.toLowerCase()}`, async () => clickText(".navigation-item", view));
  }
  // Dal Cestino ai Modelli: la nota creata dal modello non deve aprirsi in
  // sola lettura come se fosse nel cestino, nemmeno tornando indietro e avanti.
  await step("modello-dal-cestino", async () => {
    await clickText(".navigation-item", "Modelli");
    await pause(300);
    await clickText(".subtle-action.primary-action", "Usa");
    await pause(500);
    const editable = async (where) => {
      const value = await page.$eval(".ProseMirror", (element) => element.getAttribute("contenteditable"));
      if (value !== "true") throw new Error(`nota in sola lettura (${where})`);
    };
    await editable("dopo Usa");
    const selected = await page.$$eval(".navigation-item[aria-current]", (items) =>
      items.map((item) => item.textContent?.trim() ?? ""),
    );
    if (!selected.some((label) => label.includes("Tutte le note"))) throw new Error(`vista selezionata: ${selected}`);
    await page.click('.tab-nav-button[aria-label="Indietro"]');
    await pause(300);
    await page.click('.tab-nav-button[aria-label="Avanti"]');
    await pause(400);
    await editable("dopo Indietro e Avanti");
  });
  await step("concentrazione", async () => {
    await openNote();
    await clickText(".menu-bar-item", "Nota");
    await hover(".menu-item", "Vista della nota");
    await clickText(".menu-item", "Concentrazione");
    await expectVisible(".app-frame.layout-focus");
  });
  await step("ripristina-barra-laterale", async () => {
    await page.click(".sidebar-toggle");
    await pause(300);
    if (await page.$(".app-frame.layout-focus")) throw new Error("ancora in concentrazione");
    const visible = await page.$eval(".sidebar", (element) => getComputedStyle(element).visibility);
    if (visible !== "visible") throw new Error("barra laterale nascosta");
    // Concentrazione e poi un'altra sezione: si torna alla vista normale.
    await clickText(".menu-bar-item", "Nota");
    await hover(".menu-item", "Vista della nota");
    await clickText(".menu-item", "Editor ampliato");
    await clickText(".navigation-item", "Home");
    await pause(300);
    if (await page.$(".app-frame.layout-expanded")) throw new Error("vista ampliata rimasta fuori dalle note");
  });
  await step("scorciatoie", async () => {
    await keys("Control", "Slash");
    await expectVisible(".shortcuts-dialog");
  });
  await step("assistente-chat", async () => {
    await page.keyboard.press("Escape");
    await clickText(".navigation-item", "Assistente AI");
    await page.click(".ai-composer textarea");
    await page.keyboard.type("Quali decisioni abbiamo preso?");
    await page.keyboard.press("Enter");
    await pause(400);
    await expectVisible(".ai-answer .ai-citation");
    await expectVisible(".ai-sources");
  });
  await step("assistente-fonte-attivita", async () => {
    const labels = await page.$$eval(".ai-source-method", (items) => items.map((item) => item.textContent));
    if (!labels.some((label) => label?.includes("attività · per data"))) throw new Error(`etichette: ${labels}`);
    const citations = await page.$$(".ai-answer .ai-citation");
    if (citations.length !== 2) throw new Error(`citazioni: ${citations.length}`);
    await citations[1].click();
    await pause(400);
    await expectVisible("#tasks-title");
    // L'attività citata si apre nel suo dialog.
    await expectVisible('[role="dialog"]');
    await page.keyboard.press("Escape");
    await pause(200);
    await clickText(".navigation-item", "Assistente AI");
    await pause(200);
  });
  await step("assistente-ricerca", async () => {
    await clickText(".segment", "Cerca per significato");
    await page.type('input[type="search"]', "decisioni prese");
    await page.keyboard.press("Enter");
    await pause(300);
    await expectVisible(".ai-result-item");
  });
  await step("assistente-cronologia", async () => {
    await clickText(".segment", "Cronologia");
    await pause(300);
    const prompts = await page.$$eval(".ai-history-prompt", (items) => items.map((item) => item.textContent));
    if (prompts.length !== 2) throw new Error(`voci: ${prompts}`);
    await clickText(".ai-history-summary", "Quali decisioni");
    await clickText(".data-action", "Riprendi la conversazione");
    await pause(300);
    const question = await page.$eval(".ai-question", (element) => element.textContent);
    if (question !== "Quali decisioni abbiamo preso?") throw new Error(`conversazione: ${question}`);
    await expectVisible(".ai-answer .ai-citation");
  });
  await step("ai-modifica-con-anteprima", async () => {
    await openNote();
    await page.click('.toolbar-dropdown[aria-label="Funzioni AI"]');
    await clickText(".menu-item", "Correggi grammatica");
    await pause(400);
    await expectVisible(".ai-result-dialog .ai-output");
    await clickText(".segment", "Confronta");
    await expectVisible(".ai-diff ins");
    await clickText(".dialog-submit-button", "Sostituisci");
    await pause(400);
    const text = await page.$eval(".note-editor", (element) => element.textContent ?? "");
    if (!text.includes("decisioni corrette")) throw new Error(`risultato non applicato: ${text}`);
  });
  await step("ai-suggerisci-tag", async () => {
    await clickText(".menu-bar-item", "Strumenti");
    await hover(".menu-item", "AI sulla nota");
    await clickText(".menu-item", "Suggerisci tag");
    await pause(400);
    await expectVisible(".ai-suggestions");
    await clickText(".dialog-submit-button", "Aggiungi i tag scelti");
    await pause(300);
    if (await page.$(".ai-suggestions")) throw new Error("dialog dei tag ancora aperto");
  });
  await step("cronologia-confronta-versioni", async () => {
    await clickText(".menu-bar-item", "Nota");
    await clickText(".menu-item", "Informazioni");
    await pause(300);
    await clickText(".organization-dialog-actions .dialog-cancel-button", "Cronologia");
    await pause(400);
    await clickText(".history-dialog .segment", "Differenze");
    await expectVisible(".history-preview .ai-diff ins");
    await clickText(".history-dialog .segment", "Spiegazione AI");
    await clickText(".history-preview .text-button", "Spiega le differenze");
    await pause(400);
    const text = await page.$eval(".history-ai-output", (element) => element.textContent ?? "");
    if (!text.includes("decisioni corrette")) throw new Error(`spiegazione assente: ${text}`);
    await expectVisible(".history-dialog .ai-provenance");
    await clickText(".history-dialog .dialog-cancel-button", "Chiudi");
    await pause(200);
    if (await page.$(".history-dialog")) throw new Error("cronologia ancora aperta");
  });
  await step("calendario-eventi-nella-cella", async () => {
    await clickText(".navigation-item", "Calendario");
    await pause(400);
    const overflow = await page.evaluate(() => {
      const problems = [];
      for (const item of document.querySelectorAll(".calendar-grid .calendar-item")) {
        const day = item.closest(".calendar-day").getBoundingClientRect();
        const box = item.getBoundingClientRect();
        if (box.left < day.left - 1 || box.right > day.right + 1) problems.push(item.textContent);
      }
      const grid = document.querySelector(".calendar-grid");
      if (grid.scrollWidth > grid.clientWidth + 1) problems.push(`griglia ${grid.scrollWidth} > ${grid.clientWidth}`);
      // Un giorno con più eventi allunga la settimana invece di sovrapporsi
      // a quella successiva.
      const weeks = [...document.querySelectorAll(".calendar-week")];
      weeks.forEach((week, index) => {
        const bottom = Math.max(...[...week.children].map((day) => day.getBoundingClientRect().bottom));
        const next = weeks[index + 1]?.getBoundingClientRect().top ?? Infinity;
        if (bottom > next + 1) problems.push(`settimana ${index + 1} sovrapposta di ${Math.round(bottom - next)}px`);
      });
      return problems;
    });
    if (overflow.length > 0) throw new Error(`eventi fuori dalla cella: ${overflow.join(" | ")}`);
  });
  await step("calendario-esterno", async () => {
    await clickText(".navigation-item", "Calendario");
    await pause(400);
    const color = await page.$eval(".calendar-item.external", (element) => element.style.getPropertyValue("--event-color"));
    if (color !== "#d50000") throw new Error(`colore del calendario non applicato: ${color}`);
    await clickText(".calendar-item.external", "Standup");
    await pause(300);
    if (!(await page.$eval(".event-fields", (element) => element.disabled))) throw new Error("occorrenza ricorrente modificabile");
    await expectVisible(".external-event-origin");
    await clickText(".organization-dialog-actions .dialog-cancel-button", "Chiudi");
    await pause(200);
    await clickText(".calendar-item.external", "Dentista");
    await pause(300);
    const input = await page.$(".planner-dialog .dialog-field input");
    await input.click();
    await keys("Control", "KeyA");
    await input.type("Dentista spostato");
    await clickText(".dialog-submit-button", "Salva");
    await pause(300);
    const updates = await page.evaluate(() => window.__EXTERNAL_UPDATES__ ?? []);
    if (updates.length !== 1 || updates[0].id !== "ext-1" || updates[0].input.title !== "Dentista spostato")
      throw new Error(`modifica non inviata al fornitore: ${JSON.stringify(updates)}`);
    // Nuovo evento: si può scegliere il calendario Google.
    await clickText(".calendar-controls .primary-action", "Nuovo evento");
    await pause(300);
    const options = await page.$$eval(".planner-dialog select option", (items) => items.map((item) => item.textContent));
    if (!options.some((text) => text.includes("anna@example.com"))) throw new Error(`calendari: ${options}`);
    await page.keyboard.press("Escape");
  });
  await step("impostazioni-calendari-esterni", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Integrazioni");
    await pause(300);
    await expectVisible(".calendar-account");
    const text = await page.$eval(".calendar-integrations", (element) => element.textContent ?? "");
    if (!text.includes("anna@example.com") || !text.includes("sola lettura")) throw new Error("account non mostrato");
    const disabled = await page.$$eval(".calendar-integrations .data-action.primary", (buttons) =>
      buttons.map((button) => [button.textContent, button.disabled]),
    );
    if (!disabled.some(([label, off]) => label.includes("Outlook") && off)) throw new Error("Microsoft senza ID attivo");
    await clickText(".calendar-account-calendars .checkbox-field", "Festività");
    await pause(200);
    await page.keyboard.press("Escape");
  });
  await step("calendari-registrazione-inclusa", async () => {
    await page.evaluate(() => (window.__CALENDAR_BUILTIN__ = true));
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Integrazioni");
    await pause(300);
    const advancedOpen = await page.$eval(".calendar-advanced", (element) => element.open);
    if (advancedOpen) throw new Error("opzioni avanzate aperte con la registrazione inclusa");
    const buttons = await page.$$eval(".calendar-integrations .data-action.primary", (items) =>
      items.map((button) => [button.textContent, button.disabled]),
    );
    if (buttons.some(([, off]) => off)) throw new Error(`pulsanti disattivati: ${JSON.stringify(buttons)}`);
    await page.evaluate(() => (window.__CALENDAR_BUILTIN__ = false));
    await page.keyboard.press("Escape");
  });
  await step("calendari-ical", async () => {
    await clickText(".navigation-item", "Calendario");
    await pause(400);
    const color = await page.$$eval(".calendar-item.external", (items) =>
      items.filter((item) => item.textContent.includes("Partita")).map((item) => item.style.getPropertyValue("--event-color")),
    );
    if (color[0] !== "#43a047") throw new Error(`colore iCal: ${color}`);
    await clickText(".calendar-item.external", "Partita");
    await pause(300);
    const dialog = await page.$eval(".planner-dialog", (element) => element.textContent ?? "");
    if (!dialog.includes("Calcio") || !dialog.includes("sola lettura")) throw new Error("origine iCal non mostrata");
    if (!(await page.$eval(".event-fields", (element) => element.disabled))) throw new Error("evento iCal modificabile");
    await clickText(".organization-dialog-actions .dialog-cancel-button", "Chiudi");
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Integrazioni");
    await pause(300);
    const url = await page.$(".ics-add input[type=url]");
    await url.type("http://example.com/calendario.ics");
    await clickText(".ics-add .data-action.primary", "Aggiungi calendario");
    await pause(300);
    await expectVisible(".ics-subscriptions .dialog-error");
    await url.click();
    await keys("Control", "KeyA");
    await url.type("https://calendar.google.com/calendar/ical/it.italian%23holiday%40group.v.calendar.google.com/public/basic.ics");
    await clickText(".ics-add .data-action.primary", "Aggiungi calendario");
    await pause(300);
    const text = await page.$eval(".ics-subscriptions", (element) => element.textContent ?? "");
    if (!text.includes("Festività") || !text.includes("12 eventi")) throw new Error(`abbonamento non aggiunto: ${text}`);
    await page.click('.planner-dialog button[aria-label="Chiudi"]');
    await pause(200);
  });
  await step("calendari-nascosti-senza-registrazione", async () => {
    await page.evaluate(() => (window.__CALENDAR_NONE__ = true));
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Integrazioni");
    await pause(300);
    if (await page.$(".calendar-integrations, .calendar-advanced")) throw new Error("sezione calendari mostrata senza registrazioni");
    await expectVisible(".integration-snippet");
    await page.evaluate(() => (window.__CALENDAR_NONE__ = false));
    await page.keyboard.press("Escape");
  });
  await step("sincronizzazione", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Sincronizzazione");
    await pause(300);
    // Prima configurazione: Dropbox proposto per primo.
    const selected = await page.$eval(".sync-transport .segment.selected", (element) => element.textContent ?? "");
    if (selected !== "Dropbox") throw new Error(`destinazione iniziale: ${selected}`);
    await clickText(".sync-transport .segment", "Cartella sincronizzata");
    await pause(200);
    await clickText(".sync-settings .data-action", "Scegli");
    await pause(200);
    const folder = await page.$eval(".sync-folder-row input", (element) => element.value);
    if (folder !== "/home/test/Nextcloud") throw new Error(`cartella: ${folder}`);
    const name = await page.$('.sync-settings input[placeholder^="Es. Portatile"]');
    await name.type("Fisso");
    await clickText(".sync-settings .data-action.primary", "Attiva");
    await pause(200);
    await expectVisible(".confirm-dialog");
    await clickText(".confirm-dialog .dialog-submit-button", "Attiva");
    await pause(400);
    const text = await page.$eval(".sync-settings", (element) => element.textContent ?? "");
    for (const expected of ["Ultima sincronizzazione", "Portatile", "Modifiche contemporanee", "Riunione di lunedì"]) {
      if (!text.includes(expected)) throw new Error(`manca «${expected}»: ${text}`);
    }
    await clickText(".sync-conflicts .text-button", "Ho controllato");
    await pause(300);
    if (await page.$(".sync-conflicts")) throw new Error("conflitto non chiuso");
    await page.click('.planner-dialog button[aria-label="Chiudi"]');
    await pause(300);
    await openNote();
    await pause(300);
    const indicator = await page.$eval(".sync-indicator", (element) => element.textContent ?? "").catch(() => "");
    if (!indicator.includes("Sincronizzato")) throw new Error(`indicatore: ${indicator}`);
  });
  await step("sincronizzazione-dropbox", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Sincronizzazione");
    await pause(300);
    await clickText(".sync-transport .segment", "Dropbox");
    await clickText(".sync-settings .data-action.primary", "Collega Dropbox");
    await pause(200);
    await clickText(".confirm-dialog .dialog-submit-button", "Collega Dropbox");
    await pause(400);
    const text = await page.$eval(".sync-settings", (element) => element.textContent ?? "");
    if (!text.includes("Collegato a Dropbox come anna@example.com")) throw new Error(`non collegato: ${text}`);
    await clickText(".sync-settings .data-action.destructive", "Scollega Dropbox");
    await pause(200);
    await clickText(".confirm-dialog .dialog-submit-button", "Scollega Dropbox");
    await pause(300);
    await expectVisible(".sync-settings .data-action.primary");
    await page.click('.planner-dialog button[aria-label="Chiudi"]');
    await pause(200);
  });
  await step("aggiornamenti", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Aggiornamenti");
    await pause(300);
    const before = await page.$eval(".settings-content", (element) => element.textContent ?? "");
    if (!before.includes("Versione installata: 0.1.0") || !before.includes("è aggiornato"))
      throw new Error(`stato iniziale: ${before}`);
    await clickText(".settings-content .data-action", "Cerca aggiornamenti");
    await pause(300);
    const ready = await page.$eval(".settings-content", (element) => element.textContent ?? "");
    if (!ready.includes("La versione 0.2.0 è pronta") || !ready.includes("assistente con calendario"))
      throw new Error(`aggiornamento non mostrato: ${ready}`);
    await page.keyboard.press("Escape");
    await pause(200);
    const footer = await page.$(".sidebar-footer");
    await footer?.screenshot({ path: "target/ui-smoke/aggiornamento-pronto.png", scrollIntoView: true });
    // Il pulsante nella barra laterale riapre la sezione.
    await clickText(".update-ready-button", "Aggiornamento 0.2.0 pronto");
    await pause(300);
    await clickText(".settings-content .data-action", "Riavvia e aggiorna");
    await pause(300);
    if (!(await page.evaluate(() => window.__UPDATE_INSTALLED__))) throw new Error("installazione non richiesta");
    await page.keyboard.press("Escape");
  });
  await step("impostazioni-integrazioni-mcp", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Integrazioni");
    await pause(300);
    const snippet = await page.$eval(".integration-snippet", (element) => element.textContent ?? "");
    if (!snippet.startsWith("claude mcp add greatnote -- /opt/Greatnote/greatnote --mcp --data-dir"))
      throw new Error(`comando inatteso: ${snippet}`);
    await clickText(".integrations-settings .checkbox-field", "Consenti");
    await pause(200);
    const hint = await page.$eval(".integrations-settings", (element) => element.textContent ?? "");
    if (!hint.includes("revocare")) throw new Error("stato attivo non mostrato");
    await expectVisible(".integration-audit-error");
    await page.keyboard.press("Escape");
  });
  await step("impostazioni-ai", async () => {
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Intelligenza artificiale");
    await expectVisible(".ai-settings");
    await clickText(".data-action", "Prova la connessione");
    await pause(300);
    const status = await page.$eval(".ai-settings", (element) => element.textContent ?? "");
    if (!status.includes("Connessione riuscita")) throw new Error("prova di connessione non mostrata");
  });
  await step("importa-da-obsidian", async () => {
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await clickText(".menu-bar-item", "File");
    await clickText(".menu-item", "Importa");
    await clickText(".data-action", "Vault di Obsidian");
    await pause(300);
    const destination = await page.$eval(".import-destination select", (element) => element.value);
    if (destination !== "keep-structure") throw new Error(`destinazione proposta: ${destination}`);
    const space = await page.$eval(".import-destination input:not([type=checkbox])", (element) => element.value);
    if (space !== "Vault") throw new Error(`spazio proposto: ${space}`);
    await clickText(".dialog-submit-button", "Importa 2 note");
    await pause(300);
    const log = await page.evaluate(() => window.__SMOKE_LOG__.join("\n"));
    if (!log.includes('"kind":"keep-structure","space_name":"Vault"')) throw new Error("destinazione non inviata");
    await expectVisible(".data-success");
  });
  await step("impostazioni-claude-code", async () => {
    const dataDialog = await page.$(".data-dialog .dialog-close-button");
    if (dataDialog) await dataDialog.click();
    await keys("Control", "Comma");
    await clickText(".settings-nav-item", "Intelligenza artificiale");
    await pause(200);
    await clickText(".segment", "Claude Code");
    await pause(200);
    const previousTheme = await page.evaluate(() => {
      const theme = document.documentElement.dataset.theme;
      document.documentElement.dataset.theme = "light";
      return theme;
    });
    try {
      for (const selector of [".ai-unsaved .data-action.primary", ".ai-save-row .data-action.primary"]) {
        const saveButton = await page.$(selector);
        if (!saveButton) throw new Error(`${selector}: pulsante per salvare le preferenze non visibile`);
        await saveButton.hover();
        const contrast = await saveButton.evaluate((button) => {
          const parse = (color) => color.match(/[\d.]+/g)?.slice(0, 3).map(Number) ?? [];
          const luminance = (color) => {
            const channels = parse(color).map((value) => {
              const normalized = value / 255;
              return normalized <= 0.04045 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
            });
            return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
          };
          const foreground = luminance(getComputedStyle(button).color);
          const background = luminance(getComputedStyle(button).backgroundColor);
          return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05);
        });
        if (contrast < 4.5) throw new Error(`${selector}: contrasto del testo al passaggio del mouse insufficiente (${contrast.toFixed(2)}:1)`);
      }
    } finally {
      await page.evaluate((theme) => {
        if (theme === undefined) delete document.documentElement.dataset.theme;
        else document.documentElement.dataset.theme = theme;
      }, previousTheme);
    }
    await clickText(".data-action", "Verifica Claude Code");
    await pause(300);
    const text = await page.$eval(".ai-settings", (element) => element.textContent ?? "");
    if (!text.includes("Claude Code 2.1.0 (Claude Code) pronto")) throw new Error("stato di Claude Code non mostrato");
    if (await page.$('input[placeholder="http://localhost:11434/v1"]')) throw new Error("campo URL ancora visibile");
    await clickText(".checkbox-field", "Acconsento a inviare ad Anthropic");
    await clickText(".data-action", "Salva le impostazioni AI");
    await pause(300);
    await page.keyboard.press("Escape");
  });
  await step("claude-code-consenso-una-volta", async () => {
    const openDialog = await page.$(".planner-dialog .dialog-close-button");
    if (openDialog) await openDialog.click();
    await pause(200);
    await clickText(".navigation-item", "Assistente AI");
    await clickText(".segment", "Chat");
    const ask = async (text) => {
      await page.click(".ai-composer textarea");
      await page.keyboard.type(text);
      await page.keyboard.press("Enter");
      await pause(400);
    };
    await ask("Prima domanda");
    await expectVisible(".ai-remote-warning .ai-remember-consent input:checked");
    await clickText(".dialog-submit-button", "Invia a Anthropic");
    await pause(400);
    await ask("Seconda domanda");
    if (await page.$(".ai-remote-warning")) throw new Error("conferma chiesta di nuovo");
    const answers = await page.$$eval(".ai-answer .ai-citation", (items) => items.length);
    if (answers < 2) throw new Error(`risposte: ${answers}`);
  });
  await step("finestra-nota", async () => {
    await page.keyboard.press("Escape");
    await page.goto(`${harness}#note=11111111-1111-4111-8111-111111111111`, { waitUntil: "load" });
    await page.reload({ waitUntil: "load" });
    await pause(900);
    await expectVisible(".note-window .note-editor");
  });
} finally {
  await browser.close();
  await server.close();
}

for (const error of errors) failures.push(error);
if (failures.length > 0) {
  console.error(`\n${failures.length} problemi:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(`\nTest dell'interfaccia superato. Screenshot in ${output}`);
