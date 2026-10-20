import { t } from "../../i18n";
import { separator } from "../../app/menu";
import type { MenuEntry } from "../../app/menu";
import type { TargetLanguage, Tone, TransformAction } from "./aiApi";

export interface AiNoteHandlers {
  /** L'AI è attiva e configurata (vedi `aiReady`). */
  ready: boolean;
  /** C'è una nota modificabile aperta nell'editor. */
  hasNote: boolean;
  transform: (action: TransformAction, label: string) => void;
  askAboutNote: () => void;
  suggestTags: () => void;
  extractTasks: () => void;
  openSettings: () => void;
}

const TONES: Tone[] = ["formal", "friendly", "concise", "professional"];
const LANGUAGES: TargetLanguage[] = ["it", "en", "fr", "de", "es", "pt"];

export function toneLabel(tone: Tone): string {
  return {
    formal: t.aiToneFormal,
    friendly: t.aiToneFriendly,
    concise: t.aiToneConcise,
    professional: t.aiToneProfessional,
  }[tone];
}

export function languageLabel(language: TargetLanguage): string {
  return {
    it: t.aiLanguageIt,
    en: t.aiLanguageEn,
    fr: t.aiLanguageFr,
    de: t.aiLanguageDe,
    es: t.aiLanguageEs,
    pt: t.aiLanguagePt,
  }[language];
}

/** Voci AI per la nota aperta: barra dell'editor e menu Strumenti. Se l'AI
 *  non è configurata resta solo la voce per aprire le impostazioni. */
export function aiNoteItems(handlers: AiNoteHandlers): MenuEntry[] {
  if (!handlers.ready) {
    return [{ id: "ai-setup", label: t.aiSetupAction, run: handlers.openSettings }];
  }
  const enabled = handlers.hasNote;
  const transform = (id: string, action: TransformAction, label: string, fullLabel = label): MenuEntry => ({
    id: `ai-${id}`,
    label,
    enabled,
    run: () => handlers.transform(action, fullLabel),
  });
  return [
    { id: "ai-ask-note", label: t.aiAskAboutNote, enabled, run: handlers.askAboutNote },
    separator,
    transform("proofread", { kind: "proofread" }, t.aiProofread),
    {
      kind: "submenu",
      id: "ai-rewrite",
      label: t.aiRewrite,
      enabled,
      items: TONES.map((tone) =>
        transform(`rewrite-${tone}`, { kind: "rewrite", tone }, toneLabel(tone), `${t.aiRewrite}: ${toneLabel(tone)}`),
      ),
    },
    transform("summarize", { kind: "summarize" }, t.aiSummarize),
    transform("expand", { kind: "expand" }, t.aiExpand),
    {
      kind: "submenu",
      id: "ai-translate",
      label: t.aiTranslate,
      enabled,
      items: LANGUAGES.map((language) =>
        transform(
          `translate-${language}`,
          { kind: "translate", language },
          languageLabel(language),
          `${t.aiTranslate}: ${languageLabel(language)}`,
        ),
      ),
    },
    transform("bullets", { kind: "bullet-points" }, t.aiBulletPoints),
    transform("actions", { kind: "extract-actions" }, t.aiExtractActions),
    transform("clean-up", { kind: "clean-up" }, t.aiCleanUp),
    separator,
    { id: "ai-tags", label: t.aiSuggestTags, enabled, run: handlers.suggestTags },
    { id: "ai-tasks", label: t.aiExtractTasks, enabled, run: handlers.extractTasks },
  ];
}
