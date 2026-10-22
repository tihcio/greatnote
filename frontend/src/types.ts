import type { JSONContent } from "@tiptap/react";

export interface Note {
  id: string;
  notebook_id: string | null;
  title: string;
  content: JSONContent;
  created_at: string;
  updated_at: string;
  is_pinned: boolean;
  archived_at: string | null;
}

export interface Space {
  id: string;
  name: string;
  description: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface Notebook {
  id: string;
  space_id: string | null;
  name: string;
  description: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface Tag {
  id: string;
  name: string;
  color: string | null;
  created_at: string;
  updated_at: string;
}

export interface NoteDraft {
  note_id: string;
  title: string;
  content: JSONContent;
  saved_at: string;
}

export interface SearchResult {
  note: Note;
  highlighted_title: string;
  snippet: string;
}

export interface Attachment {
  id: string;
  note_id: string;
  original_filename: string;
  mime_type: string;
  size_bytes: number;
  sha256: string;
  created_at: string;
  ocr_status?: "done" | "failed" | null;
  ocr_text?: string | null;
  ocr_engine?: string | null;
  ocr_error?: string | null;
}

export interface NoteLink {
  target_note_id: string;
  title: string;
  label: string | null;
}

export type NoteView =
  | { kind: "all" }
  | { kind: "pinned" }
  | { kind: "recent" }
  | { kind: "archived" }
  | { kind: "trash" }
  | { kind: "space"; id: string }
  | { kind: "notebook"; id: string }
  | { kind: "unfiled" }
  | { kind: "tag"; id: string };

export type SaveState = "saved" | "saving" | "error";

export type ExportFormat = "markdown" | "html" | "text";

export interface ExportSummary {
  note_count: number;
  attachment_count: number;
}

export type ImportItemStatus =
  | "new"
  | "duplicate"
  | "error"
  | "imported"
  | "skipped"
  | "failed";

export interface ImportItem {
  source: string;
  title: string;
  status: ImportItemStatus;
  message: string | null;
}

export type ImportKind = "files" | "archive" | "evernote" | "joplin" | "obsidian";

export interface ImportPreview {
  kind: ImportKind;
  items: ImportItem[];
  new_count: number;
  duplicate_count: number;
  error_count: number;
  attachment_count: number;
  notebook_count: number;
  tag_count: number;
  /** Collegamenti tra note riconosciuti (Joplin, Obsidian). */
  link_count: number;
  /** Raccolte da altre app: nome proposto per lo spazio. */
  suggested_space: string | null;
}

export interface PreparedImport {
  token: string;
  preview: ImportPreview;
}

export interface ImportReport {
  imported: number;
  skipped: number;
  failed: number;
  attachments: number;
  notebook_id: string | null;
  notebooks: number;
  links: number;
  items: ImportItem[];
}

export type ImportDestination =
  | { kind: "new-notebook"; name: string }
  | { kind: "notebook"; id: string }
  | { kind: "unfiled" }
  | { kind: "keep-structure"; space_name: string | null };

export type BackupKind = "manual" | "automatic" | "pre-migration" | "pre-restore";

export interface BackupInfo {
  file_name: string;
  kind: BackupKind;
  created_at: string;
  size_bytes: number;
}

export interface BackupVerification {
  created_at: string;
  kind: BackupKind;
  schema_version: number;
  note_count: number;
  attachment_count: number;
  files_checked: number;
}

export interface ExternalBackup {
  token: string;
  file_name: string;
  encrypted: boolean;
  /** Assente finché un backup cifrato non viene sbloccato. */
  verification: BackupVerification | null;
}

export interface BackupSettings {
  auto_enabled: boolean;
  interval_days: number;
  retention: number;
}

export interface AttachmentIssue {
  note_id: string;
  note_title: string;
  filename: string;
  problem: "missing" | "corrupted";
}

export interface IntegrityReport {
  ok: boolean;
  sqlite_result: string;
  foreign_key_violations: number;
  search_index_ok: boolean;
  missing_search_rows: number;
  note_count: number;
  attachment_count: number;
  attachment_issues: AttachmentIssue[];
  orphan_files: number;
}

export type TaskStatus = "open" | "done";
export type TaskView =
  | "today"
  | "overdue"
  | "upcoming"
  | "no-date"
  | "open"
  | "flagged"
  | "completed";

export interface Task {
  id: string;
  note_id: string | null;
  /** Titolo della nota collegata, assente se la nota è nel cestino. */
  note_title: string | null;
  title: string;
  description: string;
  status: TaskStatus;
  /** 0 nessuna, 1 bassa, 2 media, 3 alta. */
  priority: number;
  /** Data civile AAAA-MM-GG. */
  due_date: string | null;
  completed_at: string | null;
  flagged: boolean;
  created_at: string;
  updated_at: string;
  next_reminder_at: string | null;
  recurrence: Recurrence | null;
}

export type RecurrenceFrequency = "daily" | "weekly" | "monthly" | "yearly";

/** Ripetizione di un'attività (`domain::recurrence::Recurrence`). */
export interface Recurrence {
  frequency: RecurrenceFrequency;
  interval: number;
  /** Solo settimanale: 1 = lunedì … 7 = domenica; vuoto = giorno della scadenza. */
  weekdays?: number[];
  /** Mensile/annuale: giorno del mese, scelto dal backend se assente. */
  month_day?: number | null;
  /** Ultima data possibile, inclusa. */
  until?: string | null;
}

export interface TaskInput {
  title: string;
  description: string;
  priority: number;
  due_date: string | null;
  note_id: string | null;
  flagged: boolean;
  recurrence: Recurrence | null;
}

export interface TaskCounts {
  today: number;
  overdue: number;
}

export type ReminderEntity = "note" | "task" | "event";
export type ReminderStatus = "pending" | "fired" | "done";
export type SnoozePreset = "ten-minutes" | "one-hour" | "tomorrow";

export interface Reminder {
  id: string;
  entity_type: ReminderEntity;
  entity_id: string;
  /** Istante UTC. */
  remind_at: string;
  timezone: string;
  local_time: string;
  status: ReminderStatus;
  last_triggered_at: string | null;
  entity_title: string;
  note_id: string | null;
  /** Per un promemoria gestito, il momento della gestione. */
  updated_at: string;
}

export interface CalendarEvent {
  id: string;
  note_id: string | null;
  note_title: string | null;
  title: string;
  description: string;
  all_day: boolean;
  start_at: string | null;
  end_at: string | null;
  start_date: string | null;
  end_date: string | null;
  timezone: string;
  created_at: string;
  updated_at: string;
  /** Calendario esterno d'origine (null per gli eventi di Greatnote). */
  external_calendar_id: string | null;
  calendar_provider: "google" | "microsoft" | "ics" | null;
  /** Occorrenza di un evento ricorrente o calendario in sola lettura. */
  read_only: boolean;
  web_link: string | null;
  /** Abbonamento iCal d'origine. */
  subscription_id: string | null;
}

export interface EventInput {
  title: string;
  description: string;
  all_day: boolean;
  /** AAAA-MM-GGTHH:MM per eventi con orario, AAAA-MM-GG per giorni interi. */
  start: string;
  end: string;
  timezone: string | null;
  note_id: string | null;
}

export type Workspace =
  | "home"
  | "notes"
  | "tasks"
  | "reminders"
  | "calendar"
  | "files"
  | "templates"
  | "assistant";

export type TrashKind = "space" | "notebook" | "tag" | "task" | "event" | "template";

export interface TrashItem {
  kind: TrashKind;
  id: string;
  title: string;
  deleted_at: string;
  /** Collegamenti che il ripristino ricrea. */
  related_count: number;
}

export interface EmptyTrashSummary {
  notes: number;
  items: number;
}

export interface NoteRevision {
  id: string;
  note_id: string;
  title: string;
  created_at: string;
  preview: string;
  word_count: number;
}

export interface NoteRevisionDetail {
  revision: NoteRevision;
  content: import("@tiptap/react").JSONContent;
}

export interface Template {
  id: string;
  name: string;
  title: string;
  content: import("@tiptap/react").JSONContent;
  builtin: boolean;
  created_at: string | null;
  updated_at: string | null;
}

export interface FileEntry {
  id: string;
  note_id: string;
  note_title: string;
  original_filename: string;
  mime_type: string;
  size_bytes: number;
  created_at: string;
}

export interface SidebarCounts {
  all: number;
  pinned: number;
  archived: number;
  unfiled: number;
  trash: number;
  files: number;
  notebooks: Record<string, number>;
  spaces: Record<string, number>;
  tags: Record<string, number>;
}

export interface NoteInfo {
  id: string;
  title: string;
  notebook_name: string | null;
  space_name: string | null;
  tags: string[];
  created_at: string;
  updated_at: string;
  archived_at: string | null;
  is_pinned: boolean;
  source: string | null;
  word_count: number;
  character_count: number;
  attachment_count: number;
  attachment_bytes: number;
  link_count: number;
  backlink_count: number;
  open_task_count: number;
  reminder_count: number;
  revision_count: number;
}

export interface AppInfo {
  version: string;
  schema_version: number;
  data_directory: string;
  database_bytes: number;
}
