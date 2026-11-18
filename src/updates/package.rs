//! Aggiornamenti dove il plugin updater di Tauri non esiste (Android).
//!
//! Il controllo legge lo stesso `latest.json` delle release usato dal
//! desktop (ADR 0023): da lì vengono versione e note. Il pacchetto è un
//! file con nome fisso allegato alla stessa release, per esempio
//! `releases/download/v0.2.8/Greatnote.arm64.apk`.
//!
//! L'autenticità non dipende da questo modulo: Android installa un APK come
//! aggiornamento solo se è firmato con la stessa chiave dell'app già
//! installata. Qui si controlla che la versione sia davvero più recente, che
//! il traffico resti su HTTPS e che il file non superi una dimensione
//! ragionevole.

use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    time::Duration,
};

use serde::Deserialize;

/// Dimensione massima accettata per `latest.json`.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Dimensione massima del pacchetto (l'APK arm64 pesa circa 25 MB).
pub const MAX_PACKAGE_BYTES: u64 = 300 * 1024 * 1024;
/// Parte finale dell'indirizzo di `latest.json` nell'ultima release.
const LATEST_MANIFEST_SUFFIX: &str = "latest/download/latest.json";

#[derive(Debug, thiserror::Error)]
pub enum PackageUpdateError {
    #[error("{}", crate::tr!("Impossibile contattare il server degli aggiornamenti: {}", "Unable to reach the update server: {}", .0))]
    Network(String),
    #[error("{}", crate::tr!("Il server degli aggiornamenti ha risposto con il codice {}", "The update server answered with status {}", .0))]
    Status(u16),
    #[error("{}", crate::tr!("La versione {} non ha un pacchetto per questo dispositivo", "Version {} has no package for this device", .0))]
    PackageMissing(String),
    #[error("{}", crate::tr!("Informazioni sulla release non valide: {}", "Invalid release information: {}", .0))]
    InvalidManifest(String),
    #[error("{}", crate::tr!("Il pacchetto supera la dimensione massima consentita", "The package exceeds the maximum allowed size"))]
    TooLarge,
    #[error("{}", crate::tr!("Impossibile salvare il pacchetto: {}", "Unable to save the package: {}", .0))]
    Io(#[from] io::Error),
}

/// Release descritta da `latest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
}

/// Interpreta `latest.json`. La versione può avere la `v` iniziale.
pub fn parse_manifest(bytes: &[u8]) -> Result<Release, PackageUpdateError> {
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| PackageUpdateError::InvalidManifest(error.to_string()))?;
    let version = manifest.version.trim().trim_start_matches('v').to_owned();
    if parse_version(&version).is_none() {
        return Err(PackageUpdateError::InvalidManifest(version));
    }
    let notes = manifest
        .notes
        .map(|notes| notes.trim().to_owned())
        .filter(|notes| !notes.is_empty());
    Ok(Release { version, notes })
}

/// `maggiore.minore.patch`; una versione con suffisso (`-beta`) o non
/// numerica non viene considerata.
fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Vero solo se `candidate` è una versione valida e più recente di
/// `current`: mai un ritorno a una versione precedente.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Indirizzo del pacchetto `file_name` della release `version`, ricavato da
/// quello di `latest.json` (`…/releases/latest/download/latest.json`).
pub fn package_url(manifest_url: &str, version: &str, file_name: &str) -> Option<String> {
    let base = manifest_url.strip_suffix(LATEST_MANIFEST_SUFFIX)?;
    if !base.starts_with("https://") || !base.ends_with("/releases/") {
        return None;
    }
    Some(format!("{base}download/v{version}/{file_name}"))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // I file delle release passano da GitHub a un host di download: i
        // reindirizzamenti servono, ma solo verso HTTPS.
        .https_only(true)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(timeout))
        .user_agent(concat!("Greatnote/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// Legge l'ultima release pubblicata.
pub fn fetch_latest(manifest_url: &str) -> Result<Release, PackageUpdateError> {
    let response = agent(Duration::from_secs(30))
        .get(manifest_url)
        .call()
        .map_err(|error| PackageUpdateError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(PackageUpdateError::Status(status));
    }
    let bytes = response
        .into_body()
        .into_with_config()
        .limit(MAX_MANIFEST_BYTES)
        .read_to_vec()
        .map_err(|error| PackageUpdateError::Network(error.to_string()))?;
    parse_manifest(&bytes)
}

/// Scarica il pacchetto in `destination`, passando per un file temporaneo
/// accanto: un download interrotto non lascia un pacchetto incompleto.
/// `progress` riceve i byte scaricati e il totale, se noto.
pub fn download_package(
    url: &str,
    version: &str,
    destination: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64, PackageUpdateError> {
    let response = agent(Duration::from_secs(60))
        .get(url)
        .call()
        .map_err(|error| PackageUpdateError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if status == 404 {
        return Err(PackageUpdateError::PackageMissing(version.to_owned()));
    }
    if status != 200 {
        return Err(PackageUpdateError::Status(status));
    }
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if total.is_some_and(|total| total > MAX_PACKAGE_BYTES) {
        return Err(PackageUpdateError::TooLarge);
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = destination.with_extension("part");
    let result = (|| {
        let mut reader = response.into_body().into_reader();
        let mut file = fs::File::create(&partial)?;
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut downloaded = 0_u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| PackageUpdateError::Network(error.to_string()))?;
            if read == 0 {
                break;
            }
            downloaded += read as u64;
            if downloaded > MAX_PACKAGE_BYTES {
                return Err(PackageUpdateError::TooLarge);
            }
            file.write_all(&buffer[..read])?;
            progress(downloaded, total);
        }
        if total.is_some_and(|total| total != downloaded) {
            return Err(PackageUpdateError::Network(crate::tr!(
                "download incompleto",
                "incomplete download"
            )));
        }
        file.sync_all()?;
        Ok(downloaded)
    })();
    match result {
        Ok(size) => {
            fs::rename(&partial, destination)?;
            Ok(size)
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str =
        "https://github.com/tihcio/greatnote/releases/latest/download/latest.json";

    #[test]
    fn manifest_gives_version_and_notes() {
        let release =
            parse_manifest(br#"{"version":"v0.2.8","notes":" Novita \n","platforms":{}}"#)
                .expect("manifest");
        assert_eq!(release.version, "0.2.8");
        assert_eq!(release.notes.as_deref(), Some("Novita"));
        let release = parse_manifest(br#"{"version":"0.3.0","notes":""}"#).expect("manifest");
        assert_eq!(release.notes, None);
        assert!(parse_manifest(br#"{"version":"latest"}"#).is_err());
        assert!(parse_manifest(b"non json").is_err());
    }

    #[test]
    fn only_strictly_newer_versions_are_offered() {
        assert!(is_newer("0.2.8", "0.2.7"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.7", "0.2.7"));
        assert!(!is_newer("0.2.6", "0.2.7"));
        assert!(!is_newer("0.2.8-beta", "0.2.7"));
        assert!(!is_newer("0.2", "0.1.0"));
    }

    #[test]
    fn package_url_points_to_the_same_release() {
        assert_eq!(
            package_url(ENDPOINT, "0.2.8", "Greatnote.arm64.apk").as_deref(),
            Some(
                "https://github.com/tihcio/greatnote/releases/download/v0.2.8/Greatnote.arm64.apk"
            )
        );
        assert_eq!(
            package_url(
                "http://github.com/x/releases/latest/download/latest.json",
                "1.0.0",
                "a"
            ),
            None
        );
        assert_eq!(
            package_url("https://example.com/latest.json", "1.0.0", "a"),
            None
        );
    }
}
