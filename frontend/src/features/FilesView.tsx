import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { openContextMenu } from "../lib/contextMenu";
import { errorMessage, formatDate, formatFileSize } from "../lib/format";
import type { FileEntry } from "../types";

const PAGE = 100;

interface FilesViewProps {
  refreshKey: number;
  onOpenNote: (noteId: string) => void;
}

/** Tutti gli allegati delle note attive, con ricerca e apertura. */
export default function FilesView({ refreshKey, onOpenNote }: FilesViewProps) {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [search, setSearch] = useState("");
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    async (offset: number) => {
      try {
        const page = await invoke<FileEntry[]>("list_files", {
          search: search.trim() || null,
          limit: PAGE,
          offset,
        });
        setFiles((current) => (offset === 0 ? page : [...current, ...page]));
        setHasMore(page.length === PAGE);
        setError(null);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    [search],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => void load(0), search ? 200 : 0);
    return () => window.clearTimeout(timer);
  }, [load, refreshKey, search]);

  const open = async (file: FileEntry) => {
    const confirmed = await askConfirmation({
      title: t.titleOpenAttachment,
      message: t.confirmOpenAttachment(file.original_filename),
      confirmLabel: t.actionOpen,
    });
    if (!confirmed) return;
    try {
      await invoke("open_note_attachment", { noteId: file.note_id, attachmentId: file.id });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <section className="workspace-panel" aria-labelledby="files-title">
      <header className="workspace-header">
        <div>
          <h1 id="files-title">{t.filesNav}</h1>
          <p className="workspace-subtitle">{t.filesSubtitle}</p>
        </div>
      </header>
      <div className="workspace-toolbar">
        <label className="search-field workspace-search">
          <span aria-hidden="true" className="search-icon">
            ⌕
          </span>
          <input
            aria-label={t.searchFiles}
            maxLength={200}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t.searchFiles}
            type="search"
            value={search}
          />
        </label>
      </div>
      <div className="workspace-scroll">
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        {files.length === 0 ? (
          <p className="workspace-empty">{search ? t.noMatches : t.noFiles}</p>
        ) : (
          <table className="files-table">
            <thead>
              <tr>
                <th scope="col">{t.fileName}</th>
                <th scope="col">{t.fileNote}</th>
                <th scope="col">{t.fileType}</th>
                <th scope="col">{t.fileSize}</th>
                <th scope="col">{t.fileAdded}</th>
                <th scope="col">
                  <span className="visually-hidden">{t.fileActions}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {files.map((file) => (
                <tr
                  key={file.id}
                  onContextMenu={(event) =>
                    openContextMenu(event, file.original_filename, [
                      { id: "ctx-open-file", label: t.openAttachment, run: () => void open(file) },
                      { id: "ctx-open-note", label: t.openLinkedNote, run: () => onOpenNote(file.note_id) },
                      {
                        id: "ctx-copy-name",
                        label: t.copyFileName,
                        run: () =>
                          void invoke("write_clipboard_text", { text: file.original_filename }).catch((cause: unknown) =>
                            setError(errorMessage(cause)),
                          ),
                      },
                    ])
                  }
                >
                  <td>
                    <button className="text-button" onClick={() => void open(file)} type="button">
                      {file.original_filename}
                    </button>
                  </td>
                  <td>
                    <button className="text-button" onClick={() => onOpenNote(file.note_id)} type="button">
                      {file.note_title || t.untitled}
                    </button>
                  </td>
                  <td>{file.mime_type}</td>
                  <td>{formatFileSize(file.size_bytes)}</td>
                  <td>{formatDate(file.created_at)}</td>
                  <td>
                    <button className="text-button" onClick={() => void open(file)} type="button">
                      {t.actionOpen}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {hasMore && (
          <button className="load-more-button" onClick={() => void load(files.length)} type="button">
            {t.loadMore}
          </button>
        )}
      </div>
    </section>
  );
}
