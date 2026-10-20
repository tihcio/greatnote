import { Channel, invoke } from "@tauri-apps/api/core";
import type { Note } from "../../types";

/** Impostazioni AI salvate nel database (mai segreti). */
export type ProviderKind = "openai-compatible" | "claude-code";

export interface AiSettings {
  enabled: boolean;
  provider: ProviderKind;
  claude_path: string;
  base_url: string;
  chat_model: string;
  embedding_model: string;
  auto_index: boolean;
  keep_history: boolean;
  ocr_engine: "ai" | "tesseract";
  ocr_on_attach: boolean;
  vision_model: string;
  vision_max_side: number;
  use_api_key: boolean;
  remote_consent: boolean;
  confirm_remote_sends: boolean;
  excluded_notebook_ids: string[];
  max_context_chars: number;
  timeout_seconds: number;
}

export interface AiIndexStatus {
  model: string;
  indexed_notes: number;
  pending_notes: number;
  chunks: number;
}

export interface AiStatus {
  settings: AiSettings;
  local: boolean;
  host: string;
  index: AiIndexStatus;
  indexing: boolean;
}

export interface ConnectionTest {
  models: string[];
  local: boolean;
  host: string;
  claude_code: { executable: string; version: string; logged_in: boolean; auth_method: string | null } | null;
}

export type RetrievalMethod = "semantic" | "keyword" | "selected";

export interface ContextSource {
  number: number;
  note_id: string;
  title: string;
  char_start: number;
  char_end: number;
  text: string;
  method: RetrievalMethod;
  score: number | null;
}

export interface PreparedContext {
  sources: ContextSource[];
  local: boolean;
  host: string;
  warning: string | null;
}

export interface Provenance {
  host: string;
  model: string;
  local: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface AiAnswer {
  text: string;
  sources: ContextSource[];
  provenance: Provenance;
}

export interface AiTextResult {
  text: string;
  provenance: Provenance;
}

export interface SuggestedTask {
  title: string;
  due_date: string | null;
}

export interface SemanticResult {
  note: Note;
  excerpt: string;
  score: number;
}

export interface IndexProgress {
  done: number;
  total: number;
}

export interface IndexReport {
  indexed: number;
  unchanged: number;
  failed: number;
  last_error: string | null;
}

export interface IndexFinished {
  report: IndexReport | null;
  error: string | null;
  cancelled: boolean;
}

export type Tone = "formal" | "friendly" | "concise" | "professional";
export type TargetLanguage = "it" | "en" | "fr" | "de" | "es" | "pt";

export type TransformAction =
  | { kind: "proofread" }
  | { kind: "rewrite"; tone: Tone }
  | { kind: "summarize" }
  | { kind: "expand" }
  | { kind: "translate"; language: TargetLanguage }
  | { kind: "bullet-points" }
  | { kind: "extract-actions" }
  | { kind: "clean-up" };

export type HistoryKind = "chat" | "transform" | "tags" | "tasks" | "search";

export interface HistoryEntry {
  id: string;
  kind: HistoryKind;
  conversation_id: string | null;
  note_id: string | null;
  prompt: string;
  response: string;
  sources: { number: number; note_id: string; title: string }[];
  model: string;
  host: string;
  local: boolean;
  error: string | null;
  created_at: string;
}

export interface ImageTextCapabilities {
  tesseract: { executable: string; version: string; languages: string[] } | null;
  pending_images: number;
}

export interface ImageTextFinished {
  read: number;
  failed: number;
  skipped: number;
  error: string | null;
  cancelled: boolean;
}

export const IMAGE_TEXT_PROGRESS_EVENT = "image-text-progress";
export const IMAGE_TEXT_FINISHED_EVENT = "image-text-finished";

export const INDEX_PROGRESS_EVENT = "ai-index-progress";
export const INDEX_FINISHED_EVENT = "ai-index-finished";

export function newRequestId(): string {
  return crypto.randomUUID();
}

function streamChannel(onDelta: (text: string) => void): Channel<string> {
  const channel = new Channel<string>();
  channel.onmessage = onDelta;
  return channel;
}

export const aiApi = {
  status: () => invoke<AiStatus>("get_ai_status"),
  updateSettings: (settings: AiSettings) => invoke<AiStatus>("update_ai_settings", { settings }),
  setApiKey: (key: string) => invoke<AiStatus>("set_ai_api_key", { key }),
  deleteApiKey: () => invoke<AiStatus>("delete_ai_api_key"),
  /** `apiKey`: chiave digitata e non ancora salvata, usata solo per la prova. */
  testConnection: (settings: AiSettings, apiKey: string | null) =>
    invoke<ConnectionTest>("test_ai_connection", { settings, apiKey }),
  cancel: (requestId: string) => invoke<void>("cancel_ai_request", { requestId }),
  prepareChat: (requestId: string, question: string, noteId: string | null) =>
    invoke<PreparedContext>("ai_prepare_chat", { requestId, question, noteId }),
  chat: (
    requestId: string,
    question: string,
    history: ChatMessage[],
    sources: ContextSource[],
    conversationId: string,
    onDelta: (text: string) => void,
  ) =>
    invoke<AiAnswer>("ai_chat", {
      requestId,
      question,
      history,
      conversationId,
      // Solo i riferimenti: il testo viene riletto dalle note nel backend.
      sources: sources.map(({ note_id, char_start, char_end }) => ({ note_id, char_start, char_end })),
      onDelta: streamChannel(onDelta),
    }),
  transform: (
    requestId: string,
    noteId: string | null,
    action: TransformAction,
    text: string,
    onDelta: (text: string) => void,
  ) =>
    invoke<AiTextResult>("ai_transform", { requestId, noteId, action, text, onDelta: streamChannel(onDelta) }),
  /** Spiega le differenze tra una revisione e la nota attuale (i testi
   *  vengono letti dal backend). */
  compareRevision: (requestId: string, revisionId: string, onDelta: (text: string) => void) =>
    invoke<AiTextResult>("ai_compare_revision", { requestId, revisionId, onDelta: streamChannel(onDelta) }),
  suggestTags: (requestId: string, noteId: string) =>
    invoke<{ tags: string[]; provenance: Provenance }>("ai_suggest_tags", { requestId, noteId }),
  extractTasks: (requestId: string, noteId: string) =>
    invoke<{ tasks: SuggestedTask[]; provenance: Provenance }>("ai_extract_tasks", { requestId, noteId }),
  markdownToDocument: (markdown: string) => invoke<unknown>("ai_markdown_to_document", { markdown }),
  startIndexing: () => invoke<void>("start_ai_indexing"),
  cancelIndexing: () => invoke<void>("cancel_ai_indexing"),
  clearIndex: () => invoke<AiStatus>("clear_ai_index"),
  semanticSearch: (requestId: string, query: string, limit: number) =>
    invoke<SemanticResult[]>("ai_semantic_search", { requestId, query, limit }),
  listHistory: (query: string | null, limit: number, offset: number) =>
    invoke<HistoryEntry[]>("list_ai_history", { query, limit, offset }),
  conversation: (conversationId: string) =>
    invoke<HistoryEntry[]>("get_ai_conversation", { conversationId }),
  deleteHistoryEntry: (id: string) => invoke<boolean>("delete_ai_history_entry", { id }),
  clearHistory: () => invoke<number>("clear_ai_history"),
  imageTextCapabilities: () => invoke<ImageTextCapabilities>("image_text_capabilities"),
  startImageTextAll: (includeFailed: boolean) => invoke<void>("start_image_text_all", { includeFailed }),
  cancelImageTextAll: () => invoke<void>("cancel_image_text_all"),
};

/** Utilizzabile per inviare dati: attiva, modello di chat scelto e consenso
 *  per i server remoti. */
export function aiReady(status: AiStatus | null): boolean {
  if (!status?.settings.enabled) return false;
  // Con Claude Code il modello vuoto è quello predefinito dell'account.
  if (!status.settings.chat_model && status.settings.provider !== "claude-code") return false;
  return status.local || status.settings.remote_consent;
}
