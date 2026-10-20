import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import NoteWindow from "./features/NoteWindow";
import { language } from "./i18n";
// Manrope (SIL OFL 1.1) incluso nell'app, solo caratteri latini: nessuna
// richiesta di rete e nessuna origine esterna nella CSP.
import "@fontsource/manrope/latin-600.css";
import "@fontsource/manrope/latin-700.css";
import "./styles.css";

document.documentElement.lang = language;

// Una finestra aperta con "Apri in una nuova finestra" mostra solo la nota.
const noteId = /^#note=([0-9a-f-]{36})$/i.exec(window.location.hash)?.[1];

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{noteId ? <NoteWindow noteId={noteId} /> : <App />}</React.StrictMode>,
);
