//! Registrazione e trascrizione delle riunioni (ADR 0035).
//!
//! - La registrazione parte solo da un'azione esplicita nell'interfaccia,
//!   che mostra un indicatore finché è in corso. Qui arrivano i campioni già
//!   convertiti (PCM 16 bit, mono, 16 kHz), scritti man mano in
//!   `recordings/<id>.pcm` nella cartella dati: una chiusura inattesa non
//!   perde l'audio, che resta elencato per essere trascritto o eliminato.
//! - La trascrizione divide l'audio in tratti di al massimo 30 secondi,
//!   tagliati nelle pause, salta i tratti di solo silenzio (i modelli
//!   inventano testo dal silenzio) e li manda uno alla volta al modello che
//!   capisce l'audio, con avanzamento e annullamento.
//! - L'audio non lascia il dispositivo se il provider è locale; con un
//!   provider remoto valgono il consenso e le esclusioni delle impostazioni
//!   AI. L'utente può eliminare l'audio in ogni momento.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{
    ai::{AiError, provider::AiProvider},
    storage::StorageError,
};

pub const SAMPLE_RATE: u32 = 16_000;
const BYTES_PER_SECOND: u64 = SAMPLE_RATE as u64 * 2;
const DIRECTORY: &str = "recordings";
/// Durata massima di una registrazione (circa 460 MB).
pub const MAX_SECONDS: u64 = 4 * 60 * 60;
/// Blocco massimo ricevuto in una volta dall'interfaccia (circa 2 minuti).
pub const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Tratti inviati al modello: al massimo 30 secondi, tagliati nella pausa
/// più marcata dopo i primi 20.
const SEGMENT_MAX_SECONDS: usize = 30;
const SEGMENT_MIN_SECONDS: usize = 20;
const WINDOW_SAMPLES: usize = SAMPLE_RATE as usize / 10;
/// Sotto questo livello medio (RMS su 16 bit) un tratto è silenzio.
const SILENCE_RMS: f64 = 120.0;
/// Tratti di zeri esatti più lunghi di così non sono audio ma buchi della
/// cattura (128 ms): un microfono vero ha sempre un po' di rumore.
const DROPOUT_SAMPLES: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum MeetingError {
    #[error("{}", crate::tr!("Registrazione non trovata", "Recording not found"))]
    NotFound,
    #[error("{}", crate::tr!("Dati audio non validi", "Invalid audio data"))]
    InvalidAudio,
    #[error("{}", crate::tr!("La registrazione ha raggiunto la durata massima di 4 ore", "The recording reached the maximum length of 4 hours"))]
    TooLong,
    #[error("{}", crate::tr!("La registrazione non contiene parlato", "The recording contains no speech"))]
    Silent,
    #[error("{}", crate::tr!("Accesso ai file delle registrazioni non riuscito: {0}", "Could not access the recording files: {0}", .0))]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Ai(#[from] AiError),
}

/// Registrazione salvata sul dispositivo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    /// Nota in cui inserire la trascrizione.
    pub note_id: Option<String>,
    pub started_at: String,
    pub duration_seconds: f64,
    /// Trascrizione già fatta (l'audio resta finché l'utente non lo elimina).
    #[serde(default)]
    pub transcribed: bool,
    /// Lingua della riunione per Whisper (`it`, `en`, `auto`).
    #[serde(default = "default_language")]
    pub language: String,
    /// Nomi e termini citati, suggeriti a Whisper.
    #[serde(default)]
    pub terms: String,
}

fn default_language() -> String {
    "it".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    note_id: Option<String>,
    started_at: String,
    #[serde(default)]
    transcribed: bool,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    terms: String,
}

fn directory(data_directory: &Path) -> Result<PathBuf, MeetingError> {
    let path = data_directory.join(DIRECTORY);
    fs::create_dir_all(&path)?;
    crate::backup::private_directory(&path)?;
    Ok(path)
}

fn valid_id(id: &str) -> Result<(), MeetingError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| MeetingError::NotFound)
}

fn paths(data_directory: &Path, id: &str) -> Result<(PathBuf, PathBuf), MeetingError> {
    valid_id(id)?;
    let directory = directory(data_directory)?;
    Ok((
        directory.join(format!("{id}.pcm")),
        directory.join(format!("{id}.json")),
    ))
}

fn read_meta(path: &Path) -> Result<Meta, MeetingError> {
    let text = fs::read_to_string(path).map_err(|_| MeetingError::NotFound)?;
    serde_json::from_str(&text).map_err(|_| MeetingError::NotFound)
}

fn info(data_directory: &Path, id: &str) -> Result<Recording, MeetingError> {
    let (audio, meta_path) = paths(data_directory, id)?;
    let meta = read_meta(&meta_path)?;
    let bytes = fs::metadata(&audio).map(|m| m.len()).unwrap_or(0);
    Ok(Recording {
        id: id.to_owned(),
        note_id: meta.note_id,
        started_at: meta.started_at,
        duration_seconds: bytes as f64 / BYTES_PER_SECOND as f64,
        transcribed: meta.transcribed,
        language: meta.language,
        terms: meta.terms,
    })
}

/// Nuova registrazione vuota.
pub fn start(
    data_directory: &Path,
    note_id: Option<&str>,
    language: &str,
    terms: &str,
) -> Result<Recording, MeetingError> {
    if let Some(note_id) = note_id {
        valid_id(note_id)?;
    }
    if !crate::ai::whisper::valid_language(language) {
        return Err(MeetingError::InvalidAudio);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (audio, meta_path) = paths(data_directory, &id)?;
    fs::File::create(&audio)?;
    let meta = Meta {
        note_id: note_id.map(str::to_owned),
        started_at: crate::time::format_utc(crate::time::now()),
        transcribed: false,
        language: language.to_owned(),
        terms: crate::ai::whisper::clean_terms(terms),
    };
    fs::write(
        &meta_path,
        serde_json::to_vec(&meta).map_err(StorageError::from)?,
    )?;
    info(data_directory, &id)
}

/// Aggiunge campioni PCM 16 bit little endian. Restituisce la durata.
pub fn append(data_directory: &Path, id: &str, pcm: &[u8]) -> Result<f64, MeetingError> {
    if pcm.len() > MAX_CHUNK_BYTES || !pcm.len().is_multiple_of(2) {
        return Err(MeetingError::InvalidAudio);
    }
    let (audio, meta_path) = paths(data_directory, id)?;
    if !meta_path.is_file() {
        return Err(MeetingError::NotFound);
    }
    let mut file = OpenOptions::new()
        .append(true)
        .open(&audio)
        .map_err(|_| MeetingError::NotFound)?;
    let length = file.metadata()?.len();
    if length + pcm.len() as u64 > MAX_SECONDS * BYTES_PER_SECOND {
        return Err(MeetingError::TooLong);
    }
    file.write_all(pcm)?;
    file.sync_data()?;
    Ok((length + pcm.len() as u64) as f64 / BYTES_PER_SECOND as f64)
}

/// Registrazioni presenti, dalla più recente.
pub fn list(data_directory: &Path) -> Result<Vec<Recording>, MeetingError> {
    let mut recordings: Vec<Recording> = fs::read_dir(directory(data_directory)?)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let id = name.strip_suffix(".json")?;
            info(data_directory, id).ok()
        })
        .collect();
    recordings.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(recordings)
}

/// Elimina audio e dati della registrazione.
pub fn delete(data_directory: &Path, id: &str) -> Result<(), MeetingError> {
    let (audio, meta_path) = paths(data_directory, id)?;
    let directory = directory(data_directory)?;
    // Anche i file temporanei di una trascrizione interrotta.
    let temporary = [
        directory.join(format!("{id}.whisper.wav")),
        directory.join(format!("{id}.whisper.json")),
    ];
    for path in [audio, meta_path].into_iter().chain(temporary) {
        match fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    Ok(())
}

pub fn mark_transcribed(data_directory: &Path, id: &str) -> Result<(), MeetingError> {
    let (_, meta_path) = paths(data_directory, id)?;
    let mut meta = read_meta(&meta_path)?;
    meta.transcribed = true;
    fs::write(
        &meta_path,
        serde_json::to_vec(&meta).map_err(StorageError::from)?,
    )?;
    Ok(())
}

fn samples(data_directory: &Path, id: &str) -> Result<Vec<i16>, MeetingError> {
    let (audio, meta_path) = paths(data_directory, id)?;
    if !meta_path.is_file() {
        return Err(MeetingError::NotFound);
    }
    let mut bytes = Vec::new();
    fs::File::open(&audio)
        .map_err(|_| MeetingError::NotFound)?
        .read_to_end(&mut bytes)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect())
}

/// File WAV completo della registrazione (per esportarla).
pub fn wav_file(data_directory: &Path, id: &str, target: &Path) -> Result<(), MeetingError> {
    let (audio, meta_path) = paths(data_directory, id)?;
    if !meta_path.is_file() {
        return Err(MeetingError::NotFound);
    }
    let length = fs::metadata(&audio)
        .map_err(|_| MeetingError::NotFound)?
        .len();
    let mut source = fs::File::open(&audio)?;
    let mut output = fs::File::create(target)?;
    output.write_all(&wav_header(
        u32::try_from(length).map_err(|_| MeetingError::TooLong)?,
    ))?;
    source.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut source, &mut output)?;
    output.sync_all()?;
    Ok(())
}

fn wav_header(data_bytes: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&1u16.to_le_bytes()); // mono
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    header
}

/// WAV in memoria di un tratto di campioni.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = wav_header((samples.len() * 2) as u32);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// Toglie i tratti di zeri esatti lasciati dalla cattura. WebKitGTK, con il
/// contesto audio a 16 kHz, alternava ogni 256 ms un blocco di voce e uno di
/// zeri: la voce arrivava spezzettata e la trascrizione era piena di errori.
/// Ripara anche le registrazioni fatte prima della correzione.
pub fn remove_dropouts(samples: &[i16]) -> Vec<i16> {
    let mut output = Vec::with_capacity(samples.len());
    let mut index = 0;
    while index < samples.len() {
        if samples[index] == 0 {
            let start = index;
            while index < samples.len() && samples[index] == 0 {
                index += 1;
            }
            if index - start < DROPOUT_SAMPLES {
                output.extend_from_slice(&samples[start..index]);
            }
        } else {
            output.push(samples[index]);
            index += 1;
        }
    }
    output
}

fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Tratti `[inizio, fine)` (in campioni): al massimo 30 secondi, tagliati
/// nella finestra più silenziosa tra i 20 e i 30 secondi.
pub fn segments(samples: &[i16]) -> Vec<(usize, usize)> {
    let rate = SAMPLE_RATE as usize;
    let mut result = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        let remaining = samples.len() - start;
        if remaining <= SEGMENT_MAX_SECONDS * rate {
            result.push((start, samples.len()));
            break;
        }
        let from = start + SEGMENT_MIN_SECONDS * rate;
        let to = start + SEGMENT_MAX_SECONDS * rate;
        let mut best = to;
        let mut quietest = f64::MAX;
        let mut window = from;
        while window + WINDOW_SAMPLES <= to {
            let level = rms(&samples[window..window + WINDOW_SAMPLES]);
            if level < quietest {
                quietest = level;
                best = window + WINDOW_SAMPLES / 2;
            }
            window += WINDOW_SAMPLES;
        }
        result.push((start, best));
        start = best;
    }
    result
}

/// Tratto di trascrizione con il suo tempo nella registrazione.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

/// Istruzioni per il modello.
pub fn prompt(english: bool) -> (&'static str, &'static str) {
    if english {
        (
            "You are a transcriber. Write down exactly what is said in the audio, in the language it is spoken in. Do not translate, summarize, explain or add comments. If nobody speaks, answer with nothing.",
            "Transcribe this audio.",
        )
    } else {
        (
            "Sei un trascrittore. Scrivi esattamente ciò che viene detto nell'audio, nella lingua in cui è pronunciato. Non tradurre, non riassumere, non spiegare e non aggiungere commenti. Se nessuno parla, non rispondere nulla.",
            "Trascrivi questo audio.",
        )
    }
}

/// Trascrive una registrazione con Whisper (ADR 0036): un'unica passata
/// sull'audio ripulito dai buchi della cattura, con VAD, lingua e nomi della
/// registrazione. `on_progress` riceve la percentuale.
#[allow(clippy::too_many_arguments)]
pub fn transcribe_with_whisper(
    data_directory: &Path,
    id: &str,
    executable: &Path,
    model: &Path,
    vad_model: &Path,
    threads: usize,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u8),
) -> Result<Vec<TranscriptSegment>, MeetingError> {
    let recording = info(data_directory, id)?;
    let samples = remove_dropouts(&samples(data_directory, id)?);
    if samples.is_empty() || rms(&samples) < SILENCE_RMS / 4.0 {
        return Err(MeetingError::Silent);
    }
    let directory = directory(data_directory)?;
    let audio = directory.join(format!("{id}.whisper.wav"));
    let output = directory.join(format!("{id}.whisper"));
    fs::write(&audio, wav_bytes(&samples))?;
    let prompt = crate::ai::whisper::prompt(&recording.language, &recording.terms);
    let job = crate::ai::whisper::WhisperJob {
        executable,
        model,
        vad_model,
        audio: &audio,
        output: &output,
        language: &recording.language,
        prompt: &prompt,
        threads,
    };
    let result = crate::ai::whisper::run(&job, cancel, on_progress);
    // Il WAV temporaneo non resta in ogni caso.
    let _ = fs::remove_file(&audio);
    let segments = result?;
    mark_transcribed(data_directory, id)?;
    Ok(segments)
}

/// Testo che un modello restituisce quando non c'è parlato.
fn is_empty_answer(text: &str) -> bool {
    let normalized: String = text
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace())
        .to_lowercase();
    normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "silenzio" | "silence" | "nessun parlato" | "no speech" | "musica" | "music"
        )
}

/// Trascrive una registrazione tratto per tratto. `on_segment` riceve
/// (tratti fatti, tratti totali, ultimo tratto se con testo).
pub fn transcribe(
    data_directory: &Path,
    id: &str,
    provider: &dyn AiProvider,
    model: &str,
    english: bool,
    cancel: &AtomicBool,
    on_segment: &mut dyn FnMut(usize, usize, Option<&TranscriptSegment>),
) -> Result<Vec<TranscriptSegment>, MeetingError> {
    let samples = remove_dropouts(&samples(data_directory, id)?);
    let rate = f64::from(SAMPLE_RATE);
    let spoken: Vec<(usize, usize)> = segments(&samples)
        .into_iter()
        .filter(|(start, end)| rms(&samples[*start..*end]) >= SILENCE_RMS)
        .collect();
    if spoken.is_empty() {
        return Err(MeetingError::Silent);
    }
    let (instructions, request) = prompt(english);
    let total = spoken.len();
    on_segment(0, total, None);
    let mut transcript = Vec::new();
    for (index, (start, end)) in spoken.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled.into());
        }
        let text = provider.transcribe_audio(
            model,
            instructions,
            request,
            &wav_bytes(&samples[start..end]),
            cancel,
        )?;
        let segment = TranscriptSegment {
            start_seconds: start as f64 / rate,
            end_seconds: end as f64 / rate,
            text: text.trim().to_owned(),
        };
        if is_empty_answer(&segment.text) {
            on_segment(index + 1, total, None);
        } else {
            on_segment(index + 1, total, Some(&segment));
            transcript.push(segment);
        }
    }
    mark_transcribed(data_directory, id)?;
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Parlato finto: un tono, con pause di silenzio.
    fn tone(seconds: f64) -> Vec<i16> {
        (0..(seconds * f64::from(SAMPLE_RATE)) as usize)
            .map(|i| ((i as f64 * 0.05).sin() * 8000.0) as i16)
            .collect()
    }

    fn silence(seconds: f64) -> Vec<i16> {
        vec![0; (seconds * f64::from(SAMPLE_RATE)) as usize]
    }

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    struct FakeAudio {
        calls: Mutex<Vec<usize>>,
    }

    impl AiProvider for FakeAudio {
        fn list_models(&self) -> Result<Vec<String>, AiError> {
            Ok(Vec::new())
        }
        fn chat(
            &self,
            _: &str,
            _: &[crate::ai::provider::ChatMessage],
            _: f32,
            _: &AtomicBool,
            _: &mut dyn FnMut(&str),
        ) -> Result<String, AiError> {
            unreachable!()
        }
        fn embed(&self, _: &str, _: &[String], _: &AtomicBool) -> Result<Vec<Vec<f32>>, AiError> {
            unreachable!()
        }
        fn provenance(&self, model: &str) -> crate::ai::provider::Provenance {
            crate::ai::provider::Provenance {
                host: "finto".to_owned(),
                model: model.to_owned(),
                local: true,
            }
        }
        fn transcribe_audio(
            &self,
            _: &str,
            _: &str,
            _: &str,
            wav: &[u8],
            _: &AtomicBool,
        ) -> Result<String, AiError> {
            assert!(wav.starts_with(b"RIFF") && &wav[8..12] == b"WAVE");
            let mut calls = self.calls.lock().expect("lock");
            calls.push(wav.len());
            Ok(if calls.len() == 2 {
                "[silenzio]".to_owned()
            } else {
                format!("frase {}", calls.len())
            })
        }
    }

    /// `whisper-cli` finto: controlla gli argomenti e scrive il risultato.
    #[cfg(unix)]
    #[test]
    fn whisper_gets_language_terms_and_clean_audio() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().expect("dir");
        let script = directory.path().join("whisper-cli");
        let arguments = directory.path().join("argomenti.txt");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n\
                 out=''; audio=''\n\
                 while [ $# -gt 0 ]; do case \"$1\" in --output-file) out=\"$2\";; --file) audio=\"$2\";; esac; shift; done\n\
                 wc -c < \"$audio\" > \"$out.size\"\n\
                 printf '{{\"transcription\":[{{\"offsets\":{{\"from\":0,\"to\":1500}},\"text\":\" Ciao a tutti.\"}}]}}' > \"$out.json\"\n",
                arguments.display()
            ),
        )
        .expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");

        let recording = start(directory.path(), None, "en", "Anna Rossi\nVLAN").expect("start");
        assert_eq!(recording.language, "en");
        assert_eq!(recording.terms, "Anna Rossi, VLAN");
        assert!(start(directory.path(), None, "fr", "").is_err());
        // Voce con i buchi di zeri della cattura: a Whisper arriva solo la voce.
        let voice = tone(2.0);
        let mut broken = Vec::new();
        for chunk in voice.chunks(4096) {
            broken.extend_from_slice(chunk);
            broken.extend(std::iter::repeat_n(0, 4096));
        }
        append(directory.path(), &recording.id, &bytes(&broken)).expect("append");
        let mut progress = Vec::new();
        let segments = transcribe_with_whisper(
            directory.path(),
            &recording.id,
            &script,
            Path::new("modello.bin"),
            Path::new("vad.bin"),
            4,
            &AtomicBool::new(false),
            &mut |p| progress.push(p),
        )
        .expect("whisper");
        assert_eq!(segments[0].text, "Ciao a tutti.");
        assert_eq!(progress.last(), Some(&100));
        let passed = fs::read_to_string(&arguments).expect("arguments");
        assert!(passed.contains("--language\nen\n"), "{passed}");
        assert!(
            passed.contains("Names and terms: Anna Rossi, VLAN."),
            "{passed}"
        );
        let size_file = directory
            .path()
            .join("recordings")
            .join(format!("{}.whisper.size", recording.id));
        let size: usize = fs::read_to_string(&size_file)
            .expect("size")
            .trim()
            .parse()
            .expect("n");
        assert_eq!(size, 44 + voice.len() * 2);
        // Nessun file temporaneo rimasto, registrazione segnata.
        assert!(
            !directory
                .path()
                .join("recordings")
                .join(format!("{}.whisper.wav", recording.id))
                .exists()
        );
        assert!(list(directory.path()).expect("list")[0].transcribed);
    }

    #[test]
    fn capture_dropouts_are_removed_before_transcribing() {
        // Voce e blocchi di zeri esatti alternati ogni 4096 campioni, come
        // registrava WebKitGTK; i brevi zeri dentro la voce restano.
        let voice = tone(4.0);
        let mut broken = Vec::new();
        for chunk in voice.chunks(4096) {
            broken.extend_from_slice(chunk);
            broken.extend(std::iter::repeat_n(0, 4096));
        }
        let repaired = remove_dropouts(&broken);
        assert_eq!(repaired, voice);
        let short = [vec![100i16; 10], vec![0; 100], vec![100; 10]].concat();
        assert_eq!(remove_dropouts(&short), short);

        let directory = tempfile::tempdir().expect("dir");
        let recording = start(directory.path(), None, "it", "").expect("start");
        append(directory.path(), &recording.id, &bytes(&broken)).expect("append");
        let provider = FakeAudio {
            calls: Mutex::new(Vec::new()),
        };
        transcribe(
            directory.path(),
            &recording.id,
            &provider,
            "audio",
            false,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        )
        .expect("transcribe");
        // Al modello arrivano solo i 4 secondi di voce, senza buchi.
        assert_eq!(
            provider.calls.lock().expect("lock")[0],
            44 + voice.len() * 2
        );
    }

    #[test]
    fn recordings_are_written_listed_and_deleted() {
        let directory = tempfile::tempdir().expect("dir");
        let note = uuid::Uuid::new_v4().to_string();
        let recording = start(directory.path(), Some(&note), "it", "").expect("start");
        assert_eq!(recording.note_id.as_deref(), Some(note.as_str()));
        let duration = append(directory.path(), &recording.id, &bytes(&tone(1.5))).expect("append");
        assert!((duration - 1.5).abs() < 0.01);
        assert!(append(directory.path(), &recording.id, &[1, 2, 3]).is_err());
        assert!(append(directory.path(), "../../etc/passwd", &[0, 0]).is_err());
        assert!(append(directory.path(), &uuid::Uuid::new_v4().to_string(), &[0, 0]).is_err());
        let listed = list(directory.path()).expect("list");
        assert_eq!(listed.len(), 1);
        assert!((listed[0].duration_seconds - 1.5).abs() < 0.01);

        let target = directory.path().join("export.wav");
        wav_file(directory.path(), &recording.id, &target).expect("wav");
        let wav = fs::read(&target).expect("read");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 48_000);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            SAMPLE_RATE
        );

        delete(directory.path(), &recording.id).expect("delete");
        assert!(list(directory.path()).expect("list").is_empty());
        delete(directory.path(), &recording.id).expect("already gone");
    }

    #[test]
    fn segments_are_cut_in_pauses_and_silence_is_skipped() {
        // 25 s di parlato, pausa, 20 s di parlato, 40 s di silenzio, 5 s.
        let audio = [
            tone(25.0),
            silence(1.0),
            tone(20.0),
            silence(40.0),
            tone(5.0),
        ]
        .concat();
        let cuts = segments(&audio);
        let rate = SAMPLE_RATE as usize;
        assert!(cuts.iter().all(|(start, end)| end - start <= 30 * rate));
        // Il primo taglio cade nella pausa dopo 25 secondi.
        let first = cuts[0].1 as f64 / f64::from(SAMPLE_RATE);
        assert!((25.0..26.0).contains(&first), "{first}");
        assert_eq!(cuts.last().map(|cut| cut.1), Some(audio.len()));

        let directory = tempfile::tempdir().expect("dir");
        let recording = start(directory.path(), None, "it", "").expect("start");
        append(directory.path(), &recording.id, &bytes(&audio)).expect("append");
        let provider = FakeAudio {
            calls: Mutex::new(Vec::new()),
        };
        let mut progress = Vec::new();
        let transcript = transcribe(
            directory.path(),
            &recording.id,
            &provider,
            "audio",
            false,
            &AtomicBool::new(false),
            &mut |done, total, segment| progress.push((done, total, segment.is_some())),
        )
        .expect("transcribe");
        let sent = provider.calls.lock().expect("lock").len();
        // I tratti di solo silenzio non vengono inviati.
        assert!(sent < cuts.len(), "{sent} su {}", cuts.len());
        // La risposta «[silenzio]» non entra nella trascrizione.
        assert_eq!(transcript.len(), sent - 1);
        assert_eq!(transcript[0].text, "frase 1");
        assert_eq!(transcript[0].start_seconds, 0.0);
        assert!(
            transcript
                .windows(2)
                .all(|pair| pair[0].end_seconds <= pair[1].start_seconds)
        );
        assert_eq!(progress.first(), Some(&(0, sent, false)));
        assert_eq!(progress.last().map(|p| p.0), Some(sent));
        assert!(list(directory.path()).expect("list")[0].transcribed);

        // Solo silenzio: nessuna richiesta.
        let quiet = start(directory.path(), None, "it", "").expect("start");
        append(directory.path(), &quiet.id, &bytes(&silence(5.0))).expect("append");
        assert!(matches!(
            transcribe(
                directory.path(),
                &quiet.id,
                &provider,
                "audio",
                false,
                &AtomicBool::new(false),
                &mut |_, _, _| {}
            ),
            Err(MeetingError::Silent)
        ));
        // Annullata prima di iniziare.
        assert!(matches!(
            transcribe(
                directory.path(),
                &recording.id,
                &provider,
                "audio",
                false,
                &AtomicBool::new(true),
                &mut |_, _, _| {}
            ),
            Err(MeetingError::Ai(AiError::Cancelled))
        ));
    }
}
