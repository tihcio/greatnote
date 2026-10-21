import { Channel, invoke } from "@tauri-apps/api/core";
import type { Note } from "../../types";

/** Impostazioni AI salvate nel database (mai segreti). */
export type ProviderKind = "openai-compatible" | "claude-code" | "builtin";

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
  /** La chat usa anche attività, eventi e promemoria come contesto. */
  planner_context: boolean;
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

export type RetrievalMethod = "semantic" | "keyword" | "selected" | "date";

export type SourceKind = "note" | "task" | "event" | "reminder";

/** Elemento aperto da una citazione (per un promemoria: la sua nota). */
export interface SourceTarget {
  kind: SourceKind;
  id: string;
}

export interface ContextSource {
  number: number;
  kind: SourceKind;
  /** Nota del frammento o nota collegata all'elemento ("" se nessuna). */
  note_id: string;
  /** Attività, evento o promemoria; "" per le note. */
  item_id: string;
  open: SourceTarget;
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

/** Fonte salvata nella cronologia; le voci meno recenti hanno solo la nota. */
export interface HistorySource {
  number: number;
  kind?: SourceKind;
  note_id: string;
  item_id?: string;
  title: string;
  open?: SourceTarget | null;
}

/** Elemento da aprire per una fonte salvata. */
export function historyTarget(source: HistorySource): SourceTarget {
  return source.open ?? { kind: "note", id: source.note_id };
}

export interface HistoryEntry {
  id: string;
  kind: HistoryKind;
  conversation_id: string | null;
  note_id: string | null;
  prompt: string;
  response: string;
  sources: HistorySource[];
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

export type BuiltinRole = "chat" | "embedding" | "vision";

/** Modello del catalogo del motore integrato (ADR 0020). */
export interface BuiltinModel {
  id: string;
  role: BuiltinRole;
  name: string;
  license: string;
  memory_gb: number;
  recommended: boolean;
  size_bytes: number;
  /** Byte ancora da scaricare (meno del totale se un file è già presente). */
  missing_bytes: number;
  installed: boolean;
  downloading: boolean;
  /** Quanto il modello è adatto a questo computer. */
  fit: ModelFit;
  /** Catalogo curato, provato con il motore distribuito. */
  curated: boolean;
  /** Repository di Hugging Face per i modelli scelti dalla ricerca. */
  repo: string | null;
}

/** gpu: sta nella scheda video · cpu: sta in RAM, più lento · tight: al
 *  limite della memoria · too-large: non ci sta · unknown: memoria ignota. */
export type ModelFit = "gpu" | "cpu" | "tight" | "too-large" | "unknown";

export interface GpuDevice {
  name: string;
  memory_bytes: number;
  dedicated: boolean;
}

export interface HardwareInfo {
  ram_bytes: number | null;
  gpus: GpuDevice[];
}

export interface BuiltinCatalog {
  engine_available: boolean;
  models: BuiltinModel[];
  models_directory: string;
  used_bytes: number;
  running_servers: number;
  hardware: HardwareInfo;
}

/** Repository GGUF trovato su Hugging Face (ADR 0021). */
export interface HubRepo {
  id: string;
  downloads: number;
  likes: number;
  license: string | null;
  pipeline_tag: string | null;
  architecture: string | null;
  gated: boolean;
  last_modified: string | null;
}

export interface HubModelFile {
  file_name: string;
  size_bytes: number;
}

export interface HubVariant {
  model: {
    id: string;
    name: string;
    license: string;
    memory_gb: number;
    model: HubModelFile;
    parts: HubModelFile[];
    projector: HubModelFile | null;
  };
  quantization: string | null;
  fit: ModelFit;
}

export interface HubRepoDetails {
  repo: string;
  revision: string;
  role: BuiltinRole;
  license: string | null;
  pipeline_tag: string | null;
  architecture: string | null;
  variants: HubVariant[];
  recommended: number | null;
  has_projector: boolean;
}

/** Dimensione complessiva di una variante (parti e proiettore inclusi). */
export function variantSize(variant: HubVariant): number {
  const { model } = variant;
  return (
    model.model.size_bytes +
    model.parts.reduce((sum, part) => sum + part.size_bytes, 0) +
    (model.projector?.size_bytes ?? 0)
  );
}

export interface ModelDownloadProgress {
  model_id: string;
  done: number;
  total: number;
}

export interface ModelDownloadFinished {
  model_id: string;
  error: string | null;
  cancelled: boolean;
}

export const MODEL_DOWNLOAD_PROGRESS_EVENT = "ai-model-download-progress";
export const MODEL_DOWNLOAD_FINISHED_EVENT = "ai-model-download-finished";

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
      // Solo i riferimenti: il testo viene riletto dal database nel backend.
      sources: sources.map(({ kind, note_id, item_id, char_start, char_end }) => ({
        kind,
        note_id,
        item_id,
        char_start,
        char_end,
      })),
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
  builtinCatalog: () => invoke<BuiltinCatalog>("get_builtin_ai_catalog"),
  downloadBuiltinModel: (modelId: string) => invoke<void>("start_builtin_model_download", { modelId }),
  searchHubModels: (query: string, role: BuiltinRole) => invoke<HubRepo[]>("search_hub_models", { query, role }),
  hubModelDetails: (repo: string, role: BuiltinRole) => invoke<HubRepoDetails>("get_hub_model_details", { repo, role }),
  downloadHubModel: (repo: string, role: BuiltinRole, modelId: string) =>
    invoke<void>("start_hub_model_download", { repo, role, modelId }),
  cancelBuiltinDownload: (modelId: string) => invoke<void>("cancel_builtin_model_download", { modelId }),
  deleteBuiltinModel: (modelId: string) => invoke<BuiltinCatalog>("delete_builtin_model", { modelId }),
};

/** Utilizzabile per inviare dati: attiva, modello di chat scelto e consenso
 *  per i server remoti. */
export function aiReady(status: AiStatus | null): boolean {
  if (!status?.settings.enabled) return false;
  // Con Claude Code il modello vuoto è quello predefinito dell'account.
  if (!status.settings.chat_model && status.settings.provider !== "claude-code") return false;
  return status.local || status.settings.remote_consent;
}
