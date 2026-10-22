import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import {
  ENCRYPTED_TEXT_EVENT,
  MAX_HINT_CHARS,
  MAX_SECRET_BYTES,
  MIN_PASSPHRASE_CHARS,
  cleanHint,
} from "../editor/encryptedText";
import type { EncryptedTextRequest } from "../editor/encryptedText";
import PlannerDialog from "./PlannerDialog";

/** Messaggio per i codici d'errore dei comandi `*_note_text`. */
function secretError(cause: unknown): string {
  switch (String(cause)) {
    case "wrong-passphrase":
      return t.secretWrongPassphrase;
    case "weak-passphrase":
      return t.secretPassphraseTooShort(MIN_PASSPHRASE_CHARS);
    case "empty":
      return t.secretEmpty;
    case "too-long":
      return t.secretTooLong;
    case "excessive-work":
      return t.secretExcessiveWork;
    default:
      return t.secretInvalid;
  }
}

const encoder = new TextEncoder();

interface HostProps {
  /** Nota aperta: serve a eliminarne la cronologia dopo la cifratura. */
  noteId: string | null;
  /** Salva subito la nota (prima di eliminare la cronologia). */
  flush: () => Promise<boolean>;
}

type CreateRequest = Extract<EncryptedTextRequest, { mode: "create" }>;
type OpenRequest = Extract<EncryptedTextRequest, { mode: "open" }>;

function CreateDialog({ request, noteId, flush, onClose }: HostProps & { request: CreateRequest; onClose: () => void }) {
  const fromSelection = request.initialText.trim() !== "";
  const [text, setText] = useState(request.initialText);
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [hint, setHint] = useState("");
  const [clearHistory, setClearHistory] = useState(fromSelection);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const problem =
    text.trim() === ""
      ? t.secretEmpty
      : encoder.encode(text).length > MAX_SECRET_BYTES
        ? t.secretTooLong
        : passphrase.length < MIN_PASSPHRASE_CHARS
          ? t.secretPassphraseTooShort(MIN_PASSPHRASE_CHARS)
          : passphrase !== confirmation
            ? t.secretPassphraseMismatch
            : null;

  const submit = async () => {
    if (problem || busy) return;
    setBusy(true);
    setError(null);
    try {
      const ciphertext = await invoke<string>("encrypt_note_text", { text, passphrase });
      request.insert({ ciphertext, hint: cleanHint(hint) });
      if (fromSelection && clearHistory && noteId) {
        // Prima la nota con il testo cifrato, poi la cronologia: altrimenti
        // il salvataggio successivo registrerebbe la versione in chiaro.
        if (!(await flush())) throw new Error(t.secretHistoryNotCleared);
        await invoke<number>("delete_note_revisions", { noteId });
      }
      onClose();
    } catch (cause) {
      setBusy(false);
      setError(cause instanceof Error ? cause.message : secretError(cause));
    }
  };

  return (
    <PlannerDialog busy={busy} mark="🔒" onClose={onClose} title={t.secretCreateTitle}>
      <form
        className="organization-dialog-form secret-dialog"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <label className="dialog-field">
          <span>{t.secretTextLabel}</span>
          <textarea
            autoComplete="off"
            onChange={(event) => setText(event.target.value)}
            rows={5}
            spellCheck={false}
            value={text}
          />
        </label>
        <div className="settings-row">
          <label className="dialog-field">
            <span>{t.passphraseLabel}</span>
            <input
              autoComplete="new-password"
              onChange={(event) => setPassphrase(event.target.value)}
              type="password"
              value={passphrase}
            />
          </label>
          <label className="dialog-field">
            <span>{t.passphraseConfirmLabel}</span>
            <input
              autoComplete="new-password"
              onChange={(event) => setConfirmation(event.target.value)}
              type="password"
              value={confirmation}
            />
          </label>
        </div>
        <label className="dialog-field">
          <span>{t.secretHintLabel}</span>
          <input
            autoComplete="off"
            maxLength={MAX_HINT_CHARS}
            onChange={(event) => setHint(event.target.value)}
            placeholder={t.secretHintPlaceholder}
            value={hint}
          />
        </label>
        <p className="data-hint">{t.secretCreateHelp}</p>
        {fromSelection && noteId && (
          <label className="checkbox-field">
            <input checked={clearHistory} onChange={(event) => setClearHistory(event.target.checked)} type="checkbox" />
            <span>{t.secretClearHistory}</span>
          </label>
        )}
        {(passphrase !== "" || confirmation !== "") && problem && (
          <p className="data-hint" role="status">
            {problem}
          </p>
        )}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
            {t.cancel}
          </button>
          <button className="dialog-submit-button" disabled={busy || problem !== null} type="submit">
            {busy ? t.secretEncrypting : t.secretEncrypt}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}

function OpenDialog({ request, onClose }: { request: OpenRequest; onClose: () => void }) {
  const [passphrase, setPassphrase] = useState("");
  /** Testo decifrato: esiste solo finché il dialog è aperto. */
  const [plain, setPlain] = useState<string | null>(null);
  const [edited, setEdited] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const unlock = async () => {
    if (busy || passphrase === "") return;
    setBusy(true);
    setError(null);
    try {
      const text = await invoke<string>("decrypt_note_text", { ciphertext: request.attrs.ciphertext, passphrase });
      setPlain(text);
      setEdited(text);
    } catch (cause) {
      setError(secretError(cause));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (busy || plain === null) return;
    if (edited.trim() === "") {
      setError(t.secretEmpty);
      return;
    }
    if (encoder.encode(edited).length > MAX_SECRET_BYTES) {
      setError(t.secretTooLong);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // Stessa password con cui è stato aperto.
      const ciphertext = await invoke<string>("encrypt_note_text", { text: edited, passphrase });
      request.update({ ciphertext, hint: request.attrs.hint });
      onClose();
    } catch (cause) {
      setBusy(false);
      setError(secretError(cause));
    }
  };

  const removeEncryption = async () => {
    if (plain === null) return;
    const confirmed = await askConfirmation({
      title: t.secretRemoveTitle,
      message: t.secretRemoveMessage,
      confirmLabel: t.secretRemove,
      destructive: true,
    });
    if (!confirmed) return;
    request.decryptInPlace(edited);
    onClose();
  };

  const copy = async () => {
    if (plain === null) return;
    try {
      await invoke("write_clipboard_text", { text: edited });
      setCopied(true);
    } catch {
      setError(t.secretCopyFailed);
    }
  };

  return (
    <PlannerDialog busy={busy} mark="🔒" onClose={onClose} title={t.secretOpenTitle}>
      {plain === null ? (
        <form
          className="organization-dialog-form secret-dialog"
          onSubmit={(event) => {
            event.preventDefault();
            void unlock();
          }}
        >
          {request.attrs.hint && <p className="data-hint">{t.secretHint(request.attrs.hint)}</p>}
          <label className="dialog-field">
            <span>{t.passphraseLabel}</span>
            <input
              autoComplete="off"
              onChange={(event) => {
                setPassphrase(event.target.value);
                setError(null);
              }}
              type="password"
              value={passphrase}
            />
          </label>
          {error && (
            <p className="dialog-error" role="alert">
              {error}
            </p>
          )}
          <footer className="organization-dialog-actions">
            <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
              {t.cancel}
            </button>
            <button className="dialog-submit-button" disabled={busy || passphrase === ""} type="submit">
              {busy ? t.secretDecrypting : t.secretShow}
            </button>
          </footer>
        </form>
      ) : (
        <form
          className="organization-dialog-form secret-dialog"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <label className="dialog-field">
            <span>{t.secretTextLabel}</span>
            <textarea
              autoComplete="off"
              // Il campo password è sparito: il focus resta nel dialog (Esc).
              autoFocus
              onChange={(event) => {
                setEdited(event.target.value);
                setCopied(false);
              }}
              readOnly={!request.editable}
              rows={6}
              spellCheck={false}
              value={edited}
            />
          </label>
          <p className="data-hint">{request.editable ? t.secretEditHelp : t.secretReadOnlyHelp}</p>
          {copied && (
            <p className="data-hint" role="status">
              {t.secretCopied}
            </p>
          )}
          {error && (
            <p className="dialog-error" role="alert">
              {error}
            </p>
          )}
          <footer className="organization-dialog-actions">
            <button className="dialog-cancel-button" disabled={busy} onClick={() => void copy()} type="button">
              {t.secretCopy}
            </button>
            {request.editable && (
              <button className="dialog-cancel-button" disabled={busy} onClick={() => void removeEncryption()} type="button">
                {t.secretRemove}
              </button>
            )}
            <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
              {t.close}
            </button>
            {request.editable && (
              <button className="dialog-submit-button" disabled={busy || edited === plain} type="submit">
                {busy ? t.secretEncrypting : t.secretSaveChanges}
              </button>
            )}
          </footer>
        </form>
      )}
    </PlannerDialog>
  );
}

/** Dialog dei testi cifrati, aperto dai nodi `encryptedText` e dal comando. */
export default function EncryptedTextHost({ noteId, flush }: HostProps) {
  const [request, setRequest] = useState<EncryptedTextRequest | null>(null);
  const [key, setKey] = useState(0);
  useEffect(() => {
    const open = (event: Event) => {
      setRequest((event as CustomEvent<EncryptedTextRequest>).detail);
      setKey((current) => current + 1);
    };
    window.addEventListener(ENCRYPTED_TEXT_EVENT, open);
    return () => window.removeEventListener(ENCRYPTED_TEXT_EVENT, open);
  }, []);
  // Cambiando nota il dialog si chiude: il testo decifrato non resta in vista.
  useEffect(() => setRequest(null), [noteId]);
  if (!request) return null;
  const close = () => setRequest(null);
  return request.mode === "create" ? (
    <CreateDialog flush={flush} key={key} noteId={noteId} onClose={close} request={request} />
  ) : (
    <OpenDialog key={key} onClose={close} request={request} />
  );
}
