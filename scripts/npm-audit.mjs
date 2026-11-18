#!/usr/bin/env node
// `npm audit` con un elenco di avvisi accettati (CI e scripts/release-check.sh).
//
// Fallisce per qualunque vulnerabilità di gravità moderata o superiore che
// non derivi solo da avvisi elencati qui sotto. Ogni avviso accettato ha una
// motivazione e va ricontrollato a ogni rilascio (docs/RELEASE_CHECKLIST.md):
// appena esiste una versione corretta si aggiorna la dipendenza e la voce
// si toglie.

import { execFileSync } from "node:child_process";

const ACCEPTED = {
  // picomatch ≤ 4.0.7 (ReDoS con extglob ripetuti come `+(*)`), nessuna
  // versione corretta al 2026-11-18. Lo usano solo Vite, tinyglobby e fdir
  // durante la compilazione, sui pattern della configurazione del progetto:
  // non entra nel pacchetto dell'app e i pattern non arrivano dagli utenti.
  "GHSA-fc5q-39xc-vjpf": "picomatch: solo strumenti di build, nessuna correzione disponibile",
};

const LEVELS = ["info", "low", "moderate", "high", "critical"];
const THRESHOLD = LEVELS.indexOf("moderate");

let output;
try {
  output = execFileSync("npm", ["audit", "--json"], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
} catch (error) {
  // `npm audit` esce con errore quando trova vulnerabilità: il JSON c'è comunque.
  output = error.stdout;
}
if (!output) {
  console.error("npm audit non ha prodotto un risultato");
  process.exit(1);
}

const report = JSON.parse(output);
const vulnerabilities = report.vulnerabilities ?? {};
const advisoryId = (url) => url?.split("/").pop();

/** Avvisi all'origine di un pacchetto vulnerabile, seguendo le dipendenze. */
function rootAdvisories(name, seen = new Set()) {
  if (seen.has(name)) return [];
  seen.add(name);
  const entry = vulnerabilities[name];
  if (!entry) return [];
  return entry.via.flatMap((via) =>
    typeof via === "string" ? rootAdvisories(via, seen) : [{ id: advisoryId(via.url), title: via.title, severity: via.severity }],
  );
}

const blocking = [];
const accepted = new Map();
for (const [name, entry] of Object.entries(vulnerabilities)) {
  if (LEVELS.indexOf(entry.severity) < THRESHOLD) continue;
  for (const advisory of rootAdvisories(name)) {
    if (ACCEPTED[advisory.id]) accepted.set(advisory.id, ACCEPTED[advisory.id]);
    else blocking.push(`${name}: ${advisory.id ?? "?"} ${advisory.title ?? ""} (${advisory.severity})`);
  }
}

for (const [id, reason] of accepted) console.log(`accettato ${id} — ${reason}`);
if (blocking.length > 0) {
  console.error(`\nVulnerabilità non accettate:\n${[...new Set(blocking)].map((line) => `  ${line}`).join("\n")}`);
  process.exit(1);
}
console.log("npm audit: nessuna vulnerabilità da correggere.");
