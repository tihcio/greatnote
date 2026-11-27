//! Trascrizione con whisper.cpp (ADR 0036).
//!
//! `whisper-cli`, distribuito con l'app, viene avviato una volta per
//! registrazione: legge un WAV a 16 kHz, toglie i silenzi con il VAD Silero,
//! trascrive nella lingua indicata e scrive le frasi con i loro tempi in un
//! file JSON. Niente porte né chiavi; annullare significa chiudere il
//! processo, che come i server di llama.cpp si chiude anche se l'app termina
//! in modo anomalo.

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use serde::Deserialize;

use super::AiError;
use crate::meeting::TranscriptSegment;

/// Nome dell'eseguibile distribuito con l'app.
pub const EXECUTABLE: &str = if cfg!(windows) {
    "whisper-cli.exe"
} else {
    "whisper-cli"
};
/// Nomi e termini passati come suggerimento (Whisper ne usa al massimo metà
/// del suo contesto testuale).
pub const MAX_TERMS_CHARS: usize = 400;
const POLL: Duration = Duration::from_millis(200);
/// Oltre questo numero di thread Whisper non accelera più, mentre il resto
/// del computer (interfaccia compresa) resta senza processore.
const MAX_THREADS: usize = 8;

/// Thread per Whisper: metà dei core logici (i thread "hyper" aiutano poco),
/// lasciandone almeno due al sistema, al massimo [`MAX_THREADS`].
pub fn threads_for(cores: usize) -> usize {
    (cores / 2)
        .max(cores.saturating_sub(2).min(2))
        .clamp(1, MAX_THREADS)
}

/// Lingua della riunione.
pub fn valid_language(language: &str) -> bool {
    matches!(language, "it" | "en" | "auto")
}

/// Suggerimento iniziale: dice a Whisper che testo aspettarsi. Senza, su
/// italiano scrive spesso senza punteggiatura né maiuscole; i nomi delle
/// persone e i termini tecnici citati aiutano a scriverli giusti.
pub fn prompt(language: &str, terms: &str) -> String {
    let base = match language {
        "en" => "Transcript of a meeting in English, with correct punctuation and capital letters.",
        _ => {
            "Trascrizione di una riunione in italiano, con punteggiatura corretta, maiuscole e accenti."
        }
    };
    let terms = clean_terms(terms);
    if terms.is_empty() {
        base.to_owned()
    } else if language == "en" {
        format!("{base} Names and terms: {terms}.")
    } else {
        format!("{base} Nomi e termini: {terms}.")
    }
}

/// Nomi e termini su una riga, senza caratteri di controllo, entro il
/// limite.
pub fn clean_terms(terms: &str) -> String {
    terms
        .split(['\n', ',', ';'])
        .map(|term| {
            term.chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
        .chars()
        .take(MAX_TERMS_CHARS)
        .collect()
}

/// Una trascrizione da eseguire.
pub struct WhisperJob<'a> {
    pub executable: &'a Path,
    pub model: &'a Path,
    pub vad_model: &'a Path,
    /// WAV a 16 kHz, mono, 16 bit.
    pub audio: &'a Path,
    /// Percorso del risultato senza estensione (viene scritto `.json`).
    pub output: &'a Path,
    pub language: &'a str,
    pub prompt: &'a str,
    pub threads: usize,
}

impl WhisperJob<'_> {
    pub fn arguments(&self) -> Vec<String> {
        let path = |path: &Path| path.display().to_string();
        vec![
            "--model".to_owned(),
            path(self.model),
            "--file".to_owned(),
            path(self.audio),
            "--language".to_owned(),
            self.language.to_owned(),
            "--threads".to_owned(),
            self.threads.clamp(1, MAX_THREADS).to_string(),
            "--prompt".to_owned(),
            self.prompt.to_owned(),
            // Il VAD toglie i silenzi: senza, Whisper inventa frasi («Grazie a
            // tutti») dove nessuno parla.
            "--vad".to_owned(),
            "--vad-model".to_owned(),
            path(self.vad_model),
            "--output-json".to_owned(),
            "--output-file".to_owned(),
            path(self.output),
            "--print-progress".to_owned(),
            "--no-prints".to_owned(),
        ]
    }

    fn json_path(&self) -> PathBuf {
        let mut name = self.output.as_os_str().to_owned();
        name.push(".json");
        PathBuf::from(name)
    }
}

/// Abbassa la priorità del processo figlio (solo tra fork ed exec).
#[cfg(target_os = "linux")]
fn libc_nice() {
    // SAFETY: chiamata di sistema senza puntatori.
    unsafe {
        libc::nice(10);
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn libc_nice() {}

/// Percentuale da una riga di `--print-progress`.
pub fn parse_progress(line: &str) -> Option<u8> {
    let (_, rest) = line.split_once("progress =")?;
    let value: u32 = rest.trim().trim_end_matches('%').trim().parse().ok()?;
    Some(value.min(100) as u8)
}

#[derive(Deserialize)]
struct Output {
    transcription: Vec<OutputSegment>,
}

#[derive(Deserialize)]
struct OutputSegment {
    offsets: Offsets,
    text: String,
}

#[derive(Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

/// Frasi del JSON di `whisper-cli`, con i tempi in secondi.
pub fn parse_output(json: &str) -> Result<Vec<TranscriptSegment>, AiError> {
    let output: Output = serde_json::from_str(json).map_err(|error| {
        AiError::InvalidResponse(crate::tr!(
            "risultato di Whisper non leggibile: {error}",
            "unreadable Whisper result: {error}"
        ))
    })?;
    Ok(output
        .transcription
        .into_iter()
        .filter_map(|segment| {
            let text = segment.text.trim().to_owned();
            (!text.is_empty()).then(|| TranscriptSegment {
                start_seconds: segment.offsets.from as f64 / 1000.0,
                end_seconds: segment.offsets.to as f64 / 1000.0,
                text,
            })
        })
        .collect())
}

/// Esegue la trascrizione. `on_progress` riceve la percentuale.
pub fn run(
    job: &WhisperJob<'_>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u8),
) -> Result<Vec<TranscriptSegment>, AiError> {
    let json = job.json_path();
    let _ = fs::remove_file(&json);
    let mut command = Command::new(job.executable);
    command
        .args(job.arguments())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(directory) = job.executable.parent() {
        // Le librerie del motore sono accanto all'eseguibile.
        command.current_dir(directory);
    }
    // Priorità bassa: una trascrizione lunga non deve rallentare
    // l'interfaccia né il resto del sistema.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `nice` è async-signal-safe e non alloca.
        unsafe {
            command.pre_exec(|| {
                libc_nice();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    let mut child = super::builtin::server::supervised::spawn(command).map_err(|error| {
        AiError::Builtin(crate::tr!(
            "avvio di Whisper non riuscito: {error}",
            "Whisper startup failed: {error}"
        ))
    })?;
    // Lo standard error viene letto in un thread: avanzamento e, in caso
    // di errore, le ultime righe da mostrare.
    let (sender, receiver) = mpsc::channel::<String>();
    let reader = child.stderr.take().map(|stderr| {
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        })
    });
    let mut tail: Vec<String> = Vec::new();
    let mut handle = |line: String| match parse_progress(&line) {
        Some(percent) => on_progress(percent),
        None if !line.trim().is_empty() => {
            tail.push(line);
            if tail.len() > 6 {
                tail.remove(0);
            }
        }
        None => {}
    };
    let status = loop {
        while let Ok(line) = receiver.try_recv() {
            handle(line);
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&json);
            return Err(AiError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(POLL),
            Err(error) => {
                return Err(AiError::Builtin(error.to_string()));
            }
        }
    };
    // Il processo può finire prima che le sue ultime righe siano state lette
    // (errore immediato, ultima percentuale): si aspetta il lettore, che
    // termina alla chiusura dello standard error, e si elabora il resto.
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    for line in receiver.try_iter() {
        handle(line);
    }
    if !status.success() {
        return Err(AiError::Builtin(crate::tr!(
            "Whisper si è fermato con un errore ({status}): {}",
            "Whisper stopped with an error ({status}): {}",
            tail.join(" · ")
        )));
    }
    let text = fs::read_to_string(&json).map_err(|error| {
        AiError::Builtin(crate::tr!(
            "risultato di Whisper mancante: {error}",
            "missing Whisper result: {error}"
        ))
    })?;
    let _ = fs::remove_file(&json);
    on_progress(100);
    parse_output(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_terms_and_arguments() {
        assert!(prompt("it", "").starts_with("Trascrizione di una riunione in italiano"));
        assert_eq!(
            clean_terms("Anna Rossi\nSentinel One;  VLAN ,\u{7}\n\n"),
            "Anna Rossi, Sentinel One, VLAN"
        );
        assert!(prompt("it", "Anna Rossi").ends_with("Nomi e termini: Anna Rossi."));
        assert!(prompt("en", "VLAN").ends_with("Names and terms: VLAN."));
        assert_eq!(
            clean_terms(&"x".repeat(1000)).chars().count(),
            MAX_TERMS_CHARS
        );
        assert!(valid_language("auto") && !valid_language("fr; rm -rf"));

        let job = WhisperJob {
            executable: Path::new("/opt/whisper/whisper-cli"),
            model: Path::new("/m/ggml-large-v3-turbo-q5_0.bin"),
            vad_model: Path::new("/m/ggml-silero-v6.2.0.bin"),
            audio: Path::new("/r/audio.wav"),
            output: Path::new("/r/risultato"),
            language: "it",
            prompt: "Riunione.",
            threads: 64,
        };
        let arguments = job.arguments();
        let value = |flag: &str| {
            arguments
                .iter()
                .position(|argument| argument == flag)
                .map(|index| arguments[index + 1].clone())
        };
        assert_eq!(value("--language").as_deref(), Some("it"));
        assert_eq!(value("--threads").as_deref(), Some("8"));
        // Metà dei core logici, almeno due liberi, mai oltre 8.
        assert_eq!(threads_for(20), 8);
        assert_eq!(threads_for(8), 4);
        assert_eq!(threads_for(4), 2);
        assert_eq!(threads_for(2), 1);
        assert_eq!(threads_for(1), 1);
        assert_eq!(
            value("--vad-model").as_deref(),
            Some("/m/ggml-silero-v6.2.0.bin")
        );
        assert!(arguments.contains(&"--vad".to_owned()));
        assert_eq!(job.json_path(), PathBuf::from("/r/risultato.json"));
    }

    #[test]
    fn progress_and_output_are_parsed() {
        assert_eq!(
            parse_progress("whisper_print_progress_callback: progress =  45%"),
            Some(45)
        );
        assert_eq!(parse_progress("progress = 140%"), Some(100));
        assert_eq!(parse_progress("read_audio_data: reading"), None);
        let json = r#"{"result":{"language":"it"},"transcription":[
            {"offsets":{"from":30,"to":6140},"text":" Buongiorno a tutti."},
            {"offsets":{"from":6140,"to":7000},"text":"   "},
            {"offsets":{"from":7000,"to":12500},"text":" Il budget resta invariato."}]}"#;
        let segments = parse_output(json).expect("parse");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Buongiorno a tutti.");
        assert!((segments[1].start_seconds - 7.0).abs() < 1e-9);
        assert!((segments[1].end_seconds - 12.5).abs() < 1e-9);
        assert!(parse_output("non json").is_err());
    }

    /// Un `whisper-cli` finto (script di shell) che scrive avanzamento e
    /// risultato come quello vero.
    #[cfg(unix)]
    #[test]
    fn a_fake_engine_is_run_cancelled_and_its_errors_reported() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().expect("dir");
        let script = directory.path().join("whisper-cli");
        fs::write(
            &script,
            "#!/bin/sh\n\
             out=''\n\
             while [ $# -gt 0 ]; do [ \"$1\" = --output-file ] && out=\"$2\"; shift; done\n\
             case \"$out\" in *lenta*) sleep 30;; *rotta*) echo 'modello non valido' >&2; exit 3;; esac\n\
             echo 'whisper_print_progress_callback: progress = 50%' >&2\n\
             printf '{\"transcription\":[{\"offsets\":{\"from\":0,\"to\":2000},\"text\":\" Ciao.\"}]}' > \"$out.json\"\n",
        )
        .expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        let job = |output: &'static str| WhisperJob {
            executable: &script,
            model: Path::new("m.bin"),
            vad_model: Path::new("v.bin"),
            audio: Path::new("a.wav"),
            output: Box::leak(directory.path().join(output).into_boxed_path()),
            language: "it",
            prompt: "Riunione.",
            threads: 4,
        };
        let mut progress = Vec::new();
        let segments = run(&job("buona"), &AtomicBool::new(false), &mut |p| {
            progress.push(p)
        })
        .expect("run");
        assert_eq!(segments[0].text, "Ciao.");
        assert_eq!(progress, vec![50, 100]);
        assert!(
            !directory.path().join("buona.json").exists(),
            "risultato rimasto"
        );

        let error = run(&job("rotta"), &AtomicBool::new(false), &mut |_| {}).expect_err("error");
        assert!(error.to_string().contains("modello non valido"), "{error}");

        let cancel = AtomicBool::new(true);
        let started = std::time::Instant::now();
        assert!(matches!(
            run(&job("lenta"), &cancel, &mut |_| {}),
            Err(AiError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
