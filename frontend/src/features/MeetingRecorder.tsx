import { useCallback, useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { errorMessage, formatDateTime } from "../lib/format";

/** Registrazione salvata sul dispositivo (`greatnote_core::meeting`). */
export interface Recording {
  id: string;
  note_id: string | null;
  started_at: string;
  duration_seconds: number;
  transcribed: boolean;
}

export interface TranscriptSegment {
  start_seconds: number;
  end_seconds: number;
  text: string;
}

interface TranscriptionProgress {
  done: number;
  total: number;
  segment: TranscriptSegment | null;
}

const SAMPLE_RATE = 16_000;
/** Ogni quanto i campioni vengono salvati su disco. */
const FLUSH_MS = 3_000;

type Phase = "setup" | "recording" | "transcribing" | "done" | "error";

/** mm:ss oppure h:mm:ss. */
export function clock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function toBase64(samples: Int16Array): string {
  const bytes = new Uint8Array(samples.buffer, samples.byteOffset, samples.byteLength);
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}

/** Campioni a 16 kHz in 16 bit, da qualunque frequenza della scheda audio. */
function convert(input: Float32Array, rate: number): Int16Array {
  const ratio = rate / SAMPLE_RATE;
  const length = Math.floor(input.length / ratio);
  const output = new Int16Array(length);
  for (let index = 0; index < length; index += 1) {
    const value = input[Math.min(input.length - 1, Math.floor(index * ratio))];
    output[index] = Math.max(-1, Math.min(1, value)) * 0x7fff;
  }
  return output;
}

function newRequestId() {
  return `rec-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

interface MeetingRecorderProps {
  noteId: string | null;
  /** Inserisce la trascrizione nella nota (aprendola se serve). */
  onTranscript: (noteId: string | null, startedAt: string, segments: TranscriptSegment[]) => Promise<void>;
  /** Riassunto o azioni sulla trascrizione appena inserita. */
  onAi: (kind: "summarize" | "extract-actions") => void;
  onRecordingChange: (seconds: number | null) => void;
  onClose: () => void;
}

/**
 * Registrazione di una riunione (ADR 0035). Pannello non modale: si possono
 * prendere appunti mentre registra. Il microfono si apre solo qui, dopo la
 * conferma di aver informato i partecipanti; l'indicatore resta visibile
 * finché la registrazione è in corso e il pannello non si chiude.
 */
export default function MeetingRecorder({ noteId, onTranscript, onAi, onRecordingChange, onClose }: MeetingRecorderProps) {
  const [phase, setPhase] = useState<Phase>("setup");
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [deviceId, setDeviceId] = useState("");
  const [level, setLevel] = useState(0);
  const [consent, setConsent] = useState(false);
  const [paused, setPaused] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [saved, setSaved] = useState<Recording[]>([]);
  const [progress, setProgress] = useState<{ done: number; total: number }>({ done: 0, total: 0 });
  const [preview, setPreview] = useState<TranscriptSegment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const streamRef = useRef<MediaStream | null>(null);
  const contextRef = useRef<AudioContext | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const pendingRef = useRef<Int16Array[]>([]);
  const writingRef = useRef<Promise<void>>(Promise.resolve());
  const pausedRef = useRef(false);
  const recordingRef = useRef<Recording | null>(null);
  const requestRef = useRef<string | null>(null);
  const meterRef = useRef<number | null>(null);

  const loadSaved = useCallback(async () => {
    try {
      setSaved(await invoke<Recording[]>("list_recordings"));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  const releaseMicrophone = useCallback(() => {
    if (meterRef.current !== null) cancelAnimationFrame(meterRef.current);
    meterRef.current = null;
    processorRef.current?.disconnect();
    processorRef.current = null;
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    void contextRef.current?.close().catch(() => undefined);
    contextRef.current = null;
    void invoke("allow_microphone", { allowed: false }).catch(() => undefined);
  }, []);

  /** Apre il microfono scelto e mostra il livello (prova del microfono). */
  const openMicrophone = useCallback(
    async (id: string) => {
      releaseMicrophone();
      setError(null);
      try {
        if (!navigator.mediaDevices?.getUserMedia) throw new Error(t.recorderNoMicrophoneApi);
        await invoke("allow_microphone", { allowed: true });
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: id ? { deviceId: { exact: id }, echoCancellation: true, noiseSuppression: true } : true,
          video: false,
        });
        streamRef.current = stream;
        let context: AudioContext;
        try {
          context = new AudioContext({ sampleRate: SAMPLE_RATE });
        } catch {
          context = new AudioContext();
        }
        contextRef.current = context;
        // Alcune webview creano il contesto sospeso fino a un gesto.
        void context.resume().catch(() => undefined);
        const source = context.createMediaStreamSource(stream);
        const processor = context.createScriptProcessor(4096, 1, 1);
        const mute = context.createGain();
        mute.gain.value = 0;
        let peak = 0;
        processor.onaudioprocess = (event) => {
          const input = event.inputBuffer.getChannelData(0);
          let sum = 0;
          for (let index = 0; index < input.length; index += 1) sum += input[index] * input[index];
          peak = Math.max(peak * 0.85, Math.sqrt(sum / input.length));
          if (recordingRef.current && !pausedRef.current) {
            pendingRef.current.push(convert(input, context.sampleRate));
          }
        };
        source.connect(processor);
        processor.connect(mute);
        mute.connect(context.destination);
        processorRef.current = processor;
        const tick = () => {
          setLevel(Math.min(1, peak * 6));
          meterRef.current = requestAnimationFrame(tick);
        };
        tick();
        const inputs = (await navigator.mediaDevices.enumerateDevices()).filter((device) => device.kind === "audioinput");
        setDevices(inputs);
        if (!id) setDeviceId(stream.getAudioTracks()[0]?.getSettings().deviceId ?? "");
      } catch (cause) {
        releaseMicrophone();
        setError(t.recorderMicrophoneFailed(errorMessage(cause)));
      }
    },
    [releaseMicrophone],
  );

  useEffect(() => {
    void loadSaved();
    void openMicrophone("");
    return () => {
      releaseMicrophone();
      onRecordingChange(null);
    };
    // Solo all'apertura e alla chiusura del pannello.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Salva i campioni raccolti, uno scrittura alla volta. */
  const flush = useCallback(() => {
    const current = recordingRef.current;
    const chunks = pendingRef.current;
    if (!current || chunks.length === 0) return writingRef.current;
    pendingRef.current = [];
    const length = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    const joined = new Int16Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      joined.set(chunk, offset);
      offset += chunk.length;
    }
    writingRef.current = writingRef.current
      .then(async () => {
        const duration = await invoke<number>("append_recording", { id: current.id, data: toBase64(joined) });
        setSeconds(duration);
        onRecordingChange(duration);
      })
      .catch((cause: unknown) => setError(t.recorderSaveFailed(errorMessage(cause))));
    return writingRef.current;
  }, [onRecordingChange]);

  useEffect(() => {
    if (phase !== "recording") return;
    const interval = window.setInterval(() => void flush(), FLUSH_MS);
    return () => window.clearInterval(interval);
  }, [flush, phase]);

  const start = async () => {
    if (!streamRef.current) {
      await openMicrophone(deviceId);
      if (!streamRef.current) return;
    }
    try {
      const created = await invoke<Recording>("start_recording", { noteId });
      recordingRef.current = created;
      setRecording(created);
      pausedRef.current = false;
      setPaused(false);
      setSeconds(0);
      onRecordingChange(0);
      setPhase("recording");
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const togglePause = () => {
    pausedRef.current = !pausedRef.current;
    setPaused(pausedRef.current);
  };

  const transcribe = async (target: Recording) => {
    const requestId = newRequestId();
    requestRef.current = requestId;
    setPhase("transcribing");
    setError(null);
    setNotice(null);
    setPreview([]);
    setProgress({ done: 0, total: 0 });
    const channel = new Channel<TranscriptionProgress>();
    channel.onmessage = (message) => {
      setProgress({ done: message.done, total: message.total });
      if (message.segment) setPreview((current) => [...current, message.segment as TranscriptSegment]);
    };
    try {
      const segments = await invoke<TranscriptSegment[]>("transcribe_recording", {
        requestId,
        id: target.id,
        onProgress: channel,
      });
      await onTranscript(target.note_id, target.started_at, segments);
      recordingRef.current = target;
      setRecording(target);
      setPhase("done");
      setNotice(t.recorderInserted(segments.length));
    } catch (cause) {
      setPhase("error");
      setError(errorMessage(cause));
    } finally {
      requestRef.current = null;
      void loadSaved();
    }
  };

  const stop = async () => {
    const current = recordingRef.current;
    if (!current) return;
    setBusy(true);
    await flush();
    await writingRef.current;
    releaseMicrophone();
    onRecordingChange(null);
    recordingRef.current = null;
    setBusy(false);
    const finished = { ...current, duration_seconds: seconds };
    await transcribe(finished);
  };

  const cancelTranscription = () => {
    if (requestRef.current) void invoke("cancel_ai_request", { requestId: requestRef.current });
  };

  const exportAudio = async (target: Recording) => {
    try {
      const name = await invoke<string | null>("export_recording", {
        id: target.id,
        title: t.recorderExportTitle,
        fileName: t.recorderFileName(formatDateTime(target.started_at)),
      });
      if (name) setNotice(t.recorderExported(name));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const deleteAudio = async (target: Recording) => {
    const confirmed = await askConfirmation({
      title: t.recorderDeleteTitle,
      message: t.recorderDeleteMessage,
      confirmLabel: t.recorderDelete,
    });
    if (!confirmed) return;
    try {
      await invoke("delete_recording", { id: target.id });
      if (recording?.id === target.id) setRecording(null);
      setNotice(t.recorderDeleted);
      void loadSaved();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const recordingNow = phase === "recording";
  const others = saved.filter((item) => item.id !== recording?.id);

  return (
    <aside aria-label={t.recorderTitle} className={`meeting-recorder${recordingNow ? " live" : ""}`}>
      <header>
        <strong>
          {recordingNow ? (
            <span aria-live="polite" className="recording-badge" role="status">
              <span aria-hidden="true" className="recording-dot" />
              {paused ? t.recorderPaused : t.recorderRecording} {clock(seconds)}
            </span>
          ) : (
            t.recorderTitle
          )}
        </strong>
        <button
          aria-label={t.close}
          className="text-button"
          disabled={recordingNow || phase === "transcribing" || busy}
          onClick={() => {
            releaseMicrophone();
            onClose();
          }}
          type="button"
        >
          ×
        </button>
      </header>

      {phase === "setup" && (
        <>
          <p className="data-hint">{t.recorderIntro}</p>
          <label className="dialog-field">
            <span>{t.recorderDevice}</span>
            <select
              onChange={(event) => {
                setDeviceId(event.target.value);
                void openMicrophone(event.target.value);
              }}
              value={deviceId}
            >
              {devices.length === 0 && <option value="">{t.recorderDefaultDevice}</option>}
              {devices.map((device, index) => (
                <option key={device.deviceId || index} value={device.deviceId}>
                  {device.label || t.recorderDeviceNumber(index + 1)}
                </option>
              ))}
            </select>
          </label>
          <div aria-label={t.recorderLevel} className="recorder-level" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(level * 100)}>
            <span style={{ width: `${Math.round(level * 100)}%` }} />
          </div>
          <label className="checkbox-field">
            <input checked={consent} onChange={(event) => setConsent(event.target.checked)} type="checkbox" />
            <span>{t.recorderConsent}</span>
          </label>
          <div className="ai-inline-actions">
            <button className="data-action primary" disabled={!consent || busy} onClick={() => void start()} type="button">
              {t.recorderStart}
            </button>
          </div>
        </>
      )}

      {recordingNow && (
        <>
          <div aria-label={t.recorderLevel} className="recorder-level" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(level * 100)}>
            <span style={{ width: `${paused ? 0 : Math.round(level * 100)}%` }} />
          </div>
          <p className="data-hint">{t.recorderRecordingHint}</p>
          <div className="ai-inline-actions">
            <button className="data-action" disabled={busy} onClick={togglePause} type="button">
              {paused ? t.recorderResume : t.recorderPause}
            </button>
            <button className="data-action primary" disabled={busy} onClick={() => void stop()} type="button">
              {t.recorderStop}
            </button>
          </div>
        </>
      )}

      {phase === "transcribing" && (
        <>
          <p className="data-hint" role="status">
            {progress.total > 0 ? t.recorderTranscribing(progress.done, progress.total) : t.recorderPreparing}
          </p>
          <progress max={Math.max(1, progress.total)} value={progress.done} />
          {preview.length > 0 && (
            <div className="recorder-preview">
              {preview.slice(-3).map((segment) => (
                <p key={segment.start_seconds}>
                  <span className="recorder-time">[{clock(segment.start_seconds)}]</span> {segment.text}
                </p>
              ))}
            </div>
          )}
          <div className="ai-inline-actions">
            <button className="data-action" onClick={cancelTranscription} type="button">
              {t.cancel}
            </button>
          </div>
        </>
      )}

      {(phase === "done" || phase === "error") && recording && (
        <>
          {phase === "done" && (
            <div className="ai-inline-actions">
              <button className="data-action" onClick={() => onAi("summarize")} type="button">
                {t.recorderSummarize}
              </button>
              <button className="data-action" onClick={() => onAi("extract-actions")} type="button">
                {t.recorderExtractActions}
              </button>
            </div>
          )}
          <p className="data-hint">{phase === "done" ? t.recorderAudioKept : t.recorderRetryHint}</p>
          <div className="ai-inline-actions">
            {phase === "error" && (
              <button className="data-action primary" onClick={() => void transcribe(recording)} type="button">
                {t.recorderRetry}
              </button>
            )}
            <button className="data-action" onClick={() => void exportAudio(recording)} type="button">
              {t.recorderExport}
            </button>
            <button className="data-action destructive" onClick={() => void deleteAudio(recording)} type="button">
              {t.recorderDelete}
            </button>
          </div>
        </>
      )}

      {phase === "setup" && others.length > 0 && (
        <details className="recorder-saved">
          <summary>{t.recorderSaved(others.length)}</summary>
          <ul>
            {others.map((item) => (
              <li key={item.id}>
                <span>
                  {formatDateTime(item.started_at)} · {clock(item.duration_seconds)}
                  {item.transcribed ? ` · ${t.recorderTranscribedBadge}` : ""}
                </span>
                <span className="ai-inline-actions">
                  <button className="text-button" onClick={() => void transcribe(item)} type="button">
                    {t.recorderTranscribe}
                  </button>
                  <button className="text-button" onClick={() => void exportAudio(item)} type="button">
                    {t.recorderExport}
                  </button>
                  <button className="text-button" onClick={() => void deleteAudio(item)} type="button">
                    {t.recorderDelete}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}

      {notice && <p className="data-hint">{notice}</p>}
      {error && (
        <p className="dialog-error" role="alert">
          {error}
        </p>
      )}
    </aside>
  );
}
