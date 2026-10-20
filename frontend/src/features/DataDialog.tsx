import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { errorMessage, formatDateTime, formatFileSize, isoDay } from "../lib/format";
import { askConfirmation } from "../lib/confirm";
import type {
  BackupInfo,
  BackupKind,
  BackupSettings,
  BackupVerification,
  ExportSummary,
  ExternalBackup,
  ImportDestination,
  ImportItemStatus,
  ImportReport,
  IntegrityReport,
  Notebook,
  PreparedImport,
} from "../types";

type Tab = "import" | "export" | "backup" | "integrity";
type Message = { kind: "success" | "error"; text: string };
type DestinationKind = ImportDestination["kind"];
type ImportSource = "files" | "folder" | "archive" | "evernote" | "joplin" | "obsidian";

const MAX_VISIBLE_ITEMS = 200;
/** Deve coincidere con `MIN_BACKUP_PASSPHRASE_CHARS` nel core Rust. */
const MIN_PASSPHRASE_CHARS = 10;

const statusLabels: Record<ImportItemStatus, string> = {
  new: t.statusNew,
  duplicate: t.statusDuplicate,
  error: t.statusError,
  imported: t.statusImported,
  skipped: t.statusSkipped,
  failed: t.statusFailed,
};

const backupKindLabels: Record<BackupKind, string> = {
  manual: t.backupKindManual,
  automatic: t.backupKindAutomatic,
  "pre-migration": t.backupKindPreMigration,
  "pre-restore": t.backupKindPreRestore,
};

interface DataDialogProps {
  notebooks: Notebook[];
  initialTab?: Tab;
  onClose: () => void;
  /** Chiamato dopo un'importazione riuscita, con l'eventuale taccuino creato. */
  onImported: (notebookId: string | null) => void;
}

export default function DataDialog({ notebooks, initialTab = "import", onClose, onImported }: DataDialogProps) {
  const [tab, setTab] = useState<Tab>(initialTab);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<Message | null>(null);

  const [prepared, setPrepared] = useState<PreparedImport | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [destinationKind, setDestinationKind] = useState<DestinationKind>("new-notebook");
  const [destinationNotebook, setDestinationNotebook] = useState("");
  const [newNotebookName, setNewNotebookName] = useState(() =>
    t.defaultImportNotebook(isoDay()),
  );
  const [useSpace, setUseSpace] = useState(true);
  const [spaceName, setSpaceName] = useState("");

  const [exportNotebook, setExportNotebook] = useState("");

  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [settings, setSettings] = useState<BackupSettings | null>(null);
  const [externalBackup, setExternalBackup] = useState<ExternalBackup | null>(null);
  const [encryptTarget, setEncryptTarget] = useState<BackupInfo | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [passphraseConfirm, setPassphraseConfirm] = useState("");
  const [unlockPassphrase, setUnlockPassphrase] = useState("");

  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null);
  const firstTab = useRef<HTMLButtonElement>(null);
  const preparedRef = useRef<PreparedImport | null>(null);
  preparedRef.current = prepared;
  const externalRef = useRef<ExternalBackup | null>(null);
  externalRef.current = externalBackup;

  useEffect(() => {
    firstTab.current?.focus();
    return () => {
      const pending = preparedRef.current;
      if (pending) void invoke("cancel_import", { token: pending.token });
      // Elimina l'eventuale copia decifrata in attesa di ripristino.
      const external = externalRef.current;
      if (external) void invoke("cancel_external_backup", { token: external.token });
    };
  }, []);

  const run = useCallback(
    async <T,>(label: string, task: () => Promise<T>): Promise<T | undefined> => {
      setBusy(label);
      setMessage(null);
      try {
        return await task();
      } catch (cause) {
        setMessage({ kind: "error", text: errorMessage(cause) });
        return undefined;
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const loadBackups = useCallback(async () => {
    const [loadedBackups, loadedSettings] = await Promise.all([
      invoke<BackupInfo[]>("list_backups"),
      invoke<BackupSettings>("get_backup_settings"),
    ]);
    setBackups(loadedBackups);
    setSettings(loadedSettings);
  }, []);

  useEffect(() => {
    if (tab === "backup") {
      loadBackups().catch((cause) => setMessage({ kind: "error", text: errorMessage(cause) }));
    }
  }, [loadBackups, tab]);

  const close = () => {
    if (!busy) onClose();
  };

  const chooseImport = async (source: ImportSource) => {
    if (prepared) await invoke("cancel_import", { token: prepared.token });
    setPrepared(null);
    setReport(null);
    const title = {
      files: t.chooseFilesTitle,
      folder: t.chooseFolderTitle,
      archive: t.chooseArchiveTitle,
      evernote: t.chooseEnexTitle,
      joplin: t.chooseJexTitle,
      obsidian: t.chooseVaultTitle,
    }[source];
    const result = await run(t.analyzing, () =>
      invoke<PreparedImport | null>("prepare_import", { source, title }),
    );
    if (!result) return;
    setPrepared(result);
    // Le raccolte da altre app mantengono i taccuini d'origine, in uno spazio.
    if (result.preview.suggested_space !== null) {
      setDestinationKind("keep-structure");
      setUseSpace(true);
      setSpaceName(result.preview.suggested_space);
    } else if (destinationKind === "keep-structure") {
      setDestinationKind("new-notebook");
    }
  };

  const confirmImport = async () => {
    if (!prepared) return;
    const destination: ImportDestination =
      destinationKind === "new-notebook"
        ? { kind: "new-notebook", name: newNotebookName }
        : destinationKind === "notebook"
          ? { kind: "notebook", id: destinationNotebook }
          : destinationKind === "keep-structure"
            ? { kind: "keep-structure", space_name: useSpace ? spaceName.trim() : null }
            : { kind: "unfiled" };
    const result = await run(t.importing, () =>
      invoke<ImportReport>("commit_import", { token: prepared.token, destination }),
    );
    if (!result) return;
    setPrepared(null);
    setReport(result);
    setMessage({
      kind: "success",
      text:
        t.importDone(result.imported, result.attachments, result.skipped, result.failed) +
        (result.notebooks > 0 || result.links > 0 ? ` ${t.importStructureDone(result.notebooks, result.links)}` : ""),
    });
    onImported(result.notebook_id);
  };

  const exportMarkdown = async () => {
    const notebook = notebooks.find((item) => item.id === exportNotebook);
    const summary = await run(t.exporting, () =>
      invoke<ExportSummary | null>("export_markdown_zip", {
        notebookId: notebook?.id ?? null,
        defaultName: notebook ? notebook.name : `greatnote-${isoDay()}`,
        unfiledFolder: t.unfiledFolder,
        title: t.exportZipTitle,
      }),
    );
    if (summary) {
      setMessage({ kind: "success", text: t.exportDone(summary.note_count, summary.attachment_count) });
    }
  };

  const exportArchive = async () => {
    const summary = await run(t.exporting, () =>
      invoke<ExportSummary | null>("export_full_archive", {
        defaultName: t.defaultArchiveName(isoDay()),
        title: t.exportArchiveTitle,
      }),
    );
    if (summary) {
      setMessage({ kind: "success", text: t.exportDone(summary.note_count, summary.attachment_count) });
    }
  };

  const createBackup = async () => {
    const created = await run(t.creatingBackup, () => invoke<BackupInfo>("create_backup"));
    if (!created) return;
    await loadBackups();
    setMessage({ kind: "success", text: t.backupCreated });
  };

  const verifyBackup = async (backup: BackupInfo) => {
    const result = await run(t.checking, () =>
      invoke<BackupVerification>("verify_backup", { fileName: backup.file_name }),
    );
    if (result) {
      setMessage({
        kind: "success",
        text: t.backupVerified(result.note_count, result.attachment_count),
      });
    }
  };

  const finishRestore = () => {
    setMessage({ kind: "success", text: t.restoreDone });
    window.setTimeout(() => window.location.reload(), 1200);
  };

  const restoreBackup = async (backup: BackupInfo) => {
    if (!(await askConfirmation({ title: t.titleRestoreBackup, message: t.confirmRestoreBackup(formatDateTime(backup.created_at)), confirmLabel: t.actionRestore, destructive: true }))) return;
    const result = await run(t.restoring, () =>
      invoke<BackupInfo>("restore_backup", { fileName: backup.file_name }),
    );
    if (result) finishRestore();
  };

  const exportBackup = async (backup: BackupInfo) => {
    const saved = await run(t.exporting, () =>
      invoke<string | null>("export_backup", {
        fileName: backup.file_name,
        title: t.exportBackupTitle,
      }),
    );
    if (saved) setMessage({ kind: "success", text: t.backupCopied(saved) });
  };

  const openEncryptForm = (backup: BackupInfo) => {
    setEncryptTarget(backup);
    setPassphrase("");
    setPassphraseConfirm("");
    setMessage(null);
  };

  const closeEncryptForm = () => {
    setEncryptTarget(null);
    setPassphrase("");
    setPassphraseConfirm("");
  };

  const passphraseProblem =
    passphrase.length < MIN_PASSPHRASE_CHARS
      ? t.passphraseTooShort(MIN_PASSPHRASE_CHARS)
      : passphrase !== passphraseConfirm
        ? t.passphraseMismatch
        : null;

  const exportEncryptedBackup = async () => {
    if (!encryptTarget || passphraseProblem) return;
    const saved = await run(t.encrypting, () =>
      invoke<string | null>("export_backup", {
        fileName: encryptTarget.file_name,
        title: t.exportEncryptedTitle,
        passphrase,
      }),
    );
    if (saved) {
      closeEncryptForm();
      setMessage({ kind: "success", text: t.encryptedCopySaved(saved) });
    }
  };

  const discardExternalBackup = () => {
    if (externalBackup) void invoke("cancel_external_backup", { token: externalBackup.token });
    setExternalBackup(null);
    setUnlockPassphrase("");
  };

  const inspectExternalBackup = async () => {
    discardExternalBackup();
    const result = await run(t.checking, () =>
      invoke<ExternalBackup | null>("inspect_external_backup", { title: t.chooseBackupTitle }),
    );
    if (!result) return;
    setExternalBackup(result);
    if (result.verification) {
      setMessage({
        kind: "success",
        text: t.backupVerified(result.verification.note_count, result.verification.attachment_count),
      });
    }
  };

  const unlockExternalBackup = async () => {
    if (!externalBackup || unlockPassphrase === "") return;
    const result = await run(t.unlocking, () =>
      invoke<ExternalBackup>("unlock_external_backup", {
        token: externalBackup.token,
        passphrase: unlockPassphrase,
      }),
    );
    if (!result?.verification) return;
    setUnlockPassphrase("");
    setExternalBackup(result);
    setMessage({
      kind: "success",
      text: t.backupVerified(result.verification.note_count, result.verification.attachment_count),
    });
  };

  const restoreExternalBackup = async () => {
    if (!externalBackup?.verification) return;
    if (
      !(await askConfirmation({ title: t.titleRestoreBackup, message: t.confirmRestoreBackup(formatDateTime(externalBackup.verification.created_at)), confirmLabel: t.actionRestore, destructive: true }))
    ) {
      return;
    }
    const result = await run(t.restoring, () =>
      invoke<BackupInfo>("restore_external_backup", { token: externalBackup.token }),
    );
    setExternalBackup(null);
    if (result) finishRestore();
  };

  const saveSettings = async () => {
    if (!settings) return;
    const saved = await run(t.savingChanges, () =>
      invoke<BackupSettings>("update_backup_settings", { settings }),
    );
    if (saved) {
      setSettings(saved);
      setMessage({ kind: "success", text: t.settingsSaved });
    }
  };

  const checkIntegrity = async () => {
    const result = await run(t.checking, () => invoke<IntegrityReport>("check_integrity"));
    if (result) setIntegrity(result);
  };

  const rebuildIndex = async () => {
    const count = await run(t.checking, () => invoke<number>("rebuild_search_index"));
    if (count !== undefined) {
      setMessage({ kind: "success", text: t.indexRebuilt(count) });
      setIntegrity(null);
    }
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: "import", label: t.importTab },
    { id: "export", label: t.exportTab },
    { id: "backup", label: t.backupTab },
    { id: "integrity", label: t.integrityTab },
  ];

  const destinationInvalid =
    (destinationKind === "new-notebook" && newNotebookName.trim() === "") ||
    (destinationKind === "notebook" && destinationNotebook === "") ||
    (destinationKind === "keep-structure" && useSpace && spaceName.trim() === "");
  const visibleItems = (prepared?.preview.items ?? report?.items ?? []).slice(0, MAX_VISIBLE_ITEMS);
  const totalItems = (prepared?.preview.items ?? report?.items ?? []).length;

  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        aria-busy={busy !== null}
        aria-labelledby="data-dialog-title"
        aria-modal="true"
        className="organization-dialog data-dialog"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            close();
          }
        }}
        role="dialog"
      >
        <header className="organization-dialog-header">
          <div className="dialog-mark" aria-hidden="true">
            ⇅
          </div>
          <div>
            <h2 id="data-dialog-title">{t.dataAndBackup}</h2>
            <p>{t.dataDialogSubtitle}</p>
          </div>
          <button
            aria-label={t.close}
            className="dialog-close-button"
            disabled={busy !== null}
            onClick={close}
            type="button"
          >
            ×
          </button>
        </header>

        <div className="data-tabs" role="tablist" aria-label={t.dataAndBackup}>
          {tabs.map((item, index) => (
            <button
              aria-controls={`data-panel-${item.id}`}
              aria-selected={tab === item.id}
              className={`data-tab${tab === item.id ? " selected" : ""}`}
              disabled={busy !== null}
              id={`data-tab-${item.id}`}
              key={item.id}
              onClick={() => {
                setTab(item.id);
                setMessage(null);
              }}
              onKeyDown={(event) => {
                if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
                event.preventDefault();
                const offset = event.key === "ArrowRight" ? 1 : tabs.length - 1;
                const next = tabs[(index + offset) % tabs.length];
                setTab(next.id);
                document.getElementById(`data-tab-${next.id}`)?.focus();
              }}
              ref={index === 0 ? firstTab : undefined}
              role="tab"
              tabIndex={tab === item.id ? 0 : -1}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </div>

        <div
          aria-labelledby={`data-tab-${tab}`}
          className="data-panel"
          id={`data-panel-${tab}`}
          role="tabpanel"
        >
          {busy && (
            <div className="data-progress" role="status">
              <span className="data-spinner" aria-hidden="true" />
              {busy}
            </div>
          )}
          {message && (
            <div
              className={message.kind === "error" ? "dialog-error" : "data-success"}
              role={message.kind === "error" ? "alert" : "status"}
            >
              {message.text}
            </div>
          )}

          {tab === "import" && (
            <>
              <p className="data-intro">{t.importIntro}</p>
              <div className="data-actions">
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("files")} type="button">
                  {t.importFiles}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("folder")} type="button">
                  {t.importFolder}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("archive")} type="button">
                  {t.importArchive}
                </button>
              </div>
              <h3 className="data-subheading">{t.importFromApps}</h3>
              <div className="data-actions">
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("evernote")} type="button">
                  {t.importEvernote}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("joplin")} type="button">
                  {t.importJoplin}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void chooseImport("obsidian")} type="button">
                  {t.importObsidian}
                </button>
              </div>
              <p className="data-hint">{t.importFromAppsHint}</p>

              {prepared && (
                <div className="import-preview">
                  <p className="data-summary">
                    {t.previewSummary(
                      prepared.preview.new_count,
                      prepared.preview.duplicate_count,
                      prepared.preview.error_count,
                    )}
                    {prepared.preview.attachment_count > 0 &&
                      ` · ${t.previewAttachments(prepared.preview.attachment_count)}`}
                    {prepared.preview.suggested_space !== null &&
                      ` · ${t.previewStructure(
                        prepared.preview.notebook_count,
                        prepared.preview.tag_count,
                        prepared.preview.link_count,
                      )}`}
                  </p>
                  {prepared.preview.kind === "archive" ? (
                    <p className="data-hint">{t.archivePreservesStructure}</p>
                  ) : (
                    <div className="import-destination">
                      <label className="dialog-field">
                        <span>{t.importDestination}</span>
                        <select
                          onChange={(event) => setDestinationKind(event.target.value as DestinationKind)}
                          value={destinationKind}
                        >
                          {prepared.preview.suggested_space !== null && (
                            <option value="keep-structure">{t.keepStructureDestination}</option>
                          )}
                          <option value="new-notebook">{t.newNotebookDestination}</option>
                          {notebooks.length > 0 && <option value="notebook">{t.noteNotebook}</option>}
                          <option value="unfiled">{t.unfiledDestination}</option>
                        </select>
                      </label>
                      {destinationKind === "keep-structure" && (
                        <>
                          <label className="checkbox-field">
                            <input
                              checked={useSpace}
                              onChange={(event) => setUseSpace(event.target.checked)}
                              type="checkbox"
                            />
                            <span>{t.importIntoNewSpace}</span>
                          </label>
                          {useSpace && (
                            <label className="dialog-field">
                              <span>{t.importSpaceName}</span>
                              <input
                                maxLength={200}
                                onChange={(event) => setSpaceName(event.target.value)}
                                value={spaceName}
                              />
                            </label>
                          )}
                          <p className="data-hint">{t.keepStructureHint}</p>
                        </>
                      )}
                      {destinationKind === "new-notebook" && (
                        <label className="dialog-field">
                          <span>{t.importNotebookName}</span>
                          <input
                            maxLength={200}
                            onChange={(event) => setNewNotebookName(event.target.value)}
                            value={newNotebookName}
                          />
                        </label>
                      )}
                      {destinationKind === "notebook" && (
                        <label className="dialog-field">
                          <span>{t.noteNotebook}</span>
                          <select
                            onChange={(event) => setDestinationNotebook(event.target.value)}
                            value={destinationNotebook}
                          >
                            <option value="">{t.chooseNotebook}</option>
                            {notebooks.map((notebook) => (
                              <option key={notebook.id} value={notebook.id}>
                                {notebook.name}
                              </option>
                            ))}
                          </select>
                        </label>
                      )}
                    </div>
                  )}
                </div>
              )}

              {visibleItems.length > 0 && (
                <ul className="import-items" aria-label={t.importTab}>
                  {visibleItems.map((item, index) => (
                    <li className={`import-item status-${item.status}`} key={`${item.source}-${index}`}>
                      <span className="import-status">{statusLabels[item.status]}</span>
                      <span className="import-name">
                        <strong>{item.title || item.source}</strong>
                        {item.title && <small>{item.source}</small>}
                        {item.message && <small className="import-message">{item.message}</small>}
                      </span>
                    </li>
                  ))}
                  {totalItems > visibleItems.length && (
                    <li className="import-item more">… {totalItems - visibleItems.length}</li>
                  )}
                </ul>
              )}

              {prepared && (
                <footer className="organization-dialog-actions">
                  <button
                    className="dialog-cancel-button"
                    disabled={busy !== null}
                    onClick={() => {
                      void invoke("cancel_import", { token: prepared.token });
                      setPrepared(null);
                    }}
                    type="button"
                  >
                    {t.cancel}
                  </button>
                  <button
                    className="dialog-submit-button"
                    disabled={busy !== null || prepared.preview.new_count === 0 || destinationInvalid}
                    onClick={() => void confirmImport()}
                    type="button"
                  >
                    {prepared.preview.new_count === 0
                      ? t.nothingToImport
                      : t.confirmImport(prepared.preview.new_count)}
                  </button>
                </footer>
              )}
            </>
          )}

          {tab === "export" && (
            <>
              <p className="data-intro">{t.exportIntro}</p>
              <section className="data-card">
                <label className="dialog-field">
                  <span>{t.exportNotebookLabel}</span>
                  <select onChange={(event) => setExportNotebook(event.target.value)} value={exportNotebook}>
                    <option value="">{t.allNotesOption}</option>
                    {notebooks.map((notebook) => (
                      <option key={notebook.id} value={notebook.id}>
                        {notebook.name}
                      </option>
                    ))}
                  </select>
                </label>
                <p className="data-hint">{t.exportMarkdownHint}</p>
                <button className="data-action" disabled={busy !== null} onClick={() => void exportMarkdown()} type="button">
                  {t.exportMarkdownZip}
                </button>
              </section>
              <section className="data-card">
                <p className="data-hint">{t.exportArchiveHint}</p>
                <button className="data-action" disabled={busy !== null} onClick={() => void exportArchive()} type="button">
                  {t.exportArchive}
                </button>
              </section>
            </>
          )}

          {tab === "backup" && (
            <>
              <p className="data-intro">{t.backupIntro}</p>
              <div className="data-actions">
                <button className="data-action primary" disabled={busy !== null} onClick={() => void createBackup()} type="button">
                  {t.createBackupNow}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void inspectExternalBackup()} type="button">
                  {t.restoreFromFile}
                </button>
              </div>
              {externalBackup && (
                <div className="data-card">
                  <p className="data-hint">
                    {externalBackup.file_name}
                    {externalBackup.verification &&
                      ` · ${formatDateTime(externalBackup.verification.created_at)}`}
                  </p>
                  {externalBackup.verification ? (
                    <div className="data-actions">
                      <button className="data-action" disabled={busy !== null} onClick={discardExternalBackup} type="button">
                        {t.cancel}
                      </button>
                      <button className="data-action destructive" disabled={busy !== null} onClick={() => void restoreExternalBackup()} type="button">
                        {t.restoreBackupAction}
                      </button>
                    </div>
                  ) : (
                    <form
                      className="passphrase-form"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void unlockExternalBackup();
                      }}
                    >
                      <p className="data-hint">{t.encryptedBackupHint}</p>
                      <label className="dialog-field">
                        <span>{t.passphraseLabel}</span>
                        <input
                          autoComplete="off"
                          autoFocus
                          onChange={(event) => setUnlockPassphrase(event.target.value)}
                          type="password"
                          value={unlockPassphrase}
                        />
                      </label>
                      <div className="data-actions">
                        <button className="data-action" disabled={busy !== null} onClick={discardExternalBackup} type="button">
                          {t.cancel}
                        </button>
                        <button className="data-action primary" disabled={busy !== null || unlockPassphrase === ""} type="submit">
                          {t.unlockBackup}
                        </button>
                      </div>
                    </form>
                  )}
                </div>
              )}
              {encryptTarget && (
                <form
                  aria-label={t.encryptedCopyHeading(formatDateTime(encryptTarget.created_at))}
                  className="data-card passphrase-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void exportEncryptedBackup();
                  }}
                >
                  <strong>{t.encryptedCopyHeading(formatDateTime(encryptTarget.created_at))}</strong>
                  <p className="data-hint">{t.encryptedCopyHint}</p>
                  <div className="settings-row">
                    <label className="dialog-field">
                      <span>{t.passphraseLabel}</span>
                      <input
                        autoComplete="new-password"
                        autoFocus
                        onChange={(event) => setPassphrase(event.target.value)}
                        type="password"
                        value={passphrase}
                      />
                    </label>
                    <label className="dialog-field">
                      <span>{t.passphraseConfirmLabel}</span>
                      <input
                        autoComplete="new-password"
                        onChange={(event) => setPassphraseConfirm(event.target.value)}
                        type="password"
                        value={passphraseConfirm}
                      />
                    </label>
                  </div>
                  {passphrase !== "" && passphraseProblem && (
                    <p className="data-hint" role="status">
                      {passphraseProblem}
                    </p>
                  )}
                  <div className="data-actions">
                    <button className="data-action" disabled={busy !== null} onClick={closeEncryptForm} type="button">
                      {t.cancel}
                    </button>
                    <button className="data-action primary" disabled={busy !== null || passphraseProblem !== null} type="submit">
                      {t.saveEncryptedCopy}
                    </button>
                  </div>
                </form>
              )}
              {backups.length === 0 ? (
                <p className="data-hint">{t.noBackups}</p>
              ) : (
                <ul className="backup-list">
                  {backups.map((backup) => (
                    <li className="backup-item" key={backup.file_name}>
                      <div>
                        <strong>{formatDateTime(backup.created_at)}</strong>
                        <small>
                          {backupKindLabels[backup.kind]} · {formatFileSize(backup.size_bytes)}
                        </small>
                      </div>
                      <div className="backup-buttons">
                        <button className="text-button" disabled={busy !== null} onClick={() => void verifyBackup(backup)} type="button">
                          {t.verifyBackup}
                        </button>
                        <button className="text-button" disabled={busy !== null} onClick={() => void exportBackup(backup)} type="button">
                          {t.exportBackupAction}
                        </button>
                        <button className="text-button" disabled={busy !== null} onClick={() => openEncryptForm(backup)} type="button">
                          {t.exportEncryptedAction}
                        </button>
                        <button className="text-button destructive-text" disabled={busy !== null} onClick={() => void restoreBackup(backup)} type="button">
                          {t.restoreBackupAction}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              {settings && (
                <section className="data-card backup-settings">
                  <label className="checkbox-field">
                    <input
                      checked={settings.auto_enabled}
                      onChange={(event) => setSettings({ ...settings, auto_enabled: event.target.checked })}
                      type="checkbox"
                    />
                    <span>{t.autoBackupLabel}</span>
                  </label>
                  <div className="settings-row">
                    <label className="dialog-field">
                      <span>{t.intervalLabel}</span>
                      <input
                        disabled={!settings.auto_enabled}
                        max={365}
                        min={1}
                        onChange={(event) =>
                          setSettings({ ...settings, interval_days: Number(event.target.value) })
                        }
                        type="number"
                        value={settings.interval_days}
                      />
                    </label>
                    <label className="dialog-field">
                      <span>{t.retentionLabel}</span>
                      <input
                        disabled={!settings.auto_enabled}
                        max={100}
                        min={1}
                        onChange={(event) =>
                          setSettings({ ...settings, retention: Number(event.target.value) })
                        }
                        type="number"
                        value={settings.retention}
                      />
                    </label>
                  </div>
                  <button className="data-action" disabled={busy !== null} onClick={() => void saveSettings()} type="button">
                    {t.saveSettings}
                  </button>
                </section>
              )}
            </>
          )}

          {tab === "integrity" && (
            <>
              <p className="data-intro">{t.integrityIntro}</p>
              <div className="data-actions">
                <button className="data-action primary" disabled={busy !== null} onClick={() => void checkIntegrity()} type="button">
                  {t.runIntegrityCheck}
                </button>
                <button className="data-action" disabled={busy !== null} onClick={() => void rebuildIndex()} type="button">
                  {t.rebuildIndex}
                </button>
              </div>
              {integrity && (
                <section className="data-card" aria-live="polite">
                  <p className={integrity.ok ? "data-success" : "dialog-error"}>
                    {integrity.ok ? t.integrityOk : t.integrityProblems}
                  </p>
                  <p className="data-hint">
                    {t.integrityDetails(integrity.note_count, integrity.attachment_count)}
                  </p>
                  <dl className="integrity-list">
                    <dt>{t.sqliteStatus}</dt>
                    <dd>
                      {integrity.sqlite_result === "ok" ? t.statusOk : integrity.sqlite_result}
                      {integrity.foreign_key_violations > 0 &&
                        ` · ${t.foreignKeyIssues(integrity.foreign_key_violations)}`}
                    </dd>
                    <dt>{t.searchIndexStatus}</dt>
                    <dd>
                      {integrity.search_index_ok && integrity.missing_search_rows === 0
                        ? t.statusOk
                        : t.missingSearchRows(integrity.missing_search_rows)}
                    </dd>
                    {integrity.attachment_issues.map((issue) => (
                      <div className="integrity-issue" key={`${issue.note_id}-${issue.filename}`}>
                        <dt>{issue.filename}</dt>
                        <dd>
                          {issue.note_title} ·{" "}
                          {issue.problem === "missing" ? t.attachmentMissing : t.attachmentCorrupted}
                        </dd>
                      </div>
                    ))}
                  </dl>
                  {integrity.orphan_files > 0 && (
                    <p className="data-hint">{t.orphanFiles(integrity.orphan_files)}</p>
                  )}
                </section>
              )}
            </>
          )}
        </div>
      </section>
    </div>
  );
}
