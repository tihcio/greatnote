//! File scelti dall'utente con il selettore di sistema.
//!
//! Sul desktop il selettore restituisce un percorso e il core lavora
//! direttamente su quello. Su Android restituisce un *content URI*, che non è
//! un percorso: il contenuto viene copiato in una cartella temporanea privata
//! dell'app (`<cache>/transfer/<area>`) e il core lavora sulla copia. Per i
//! file da salvare vale il contrario: il core scrive nella cartella
//! temporanea e [`OutputFile::finish`] copia il risultato nella destinazione.
//!
//! Le copie temporanee servono fino alla conferma dell'operazione (anteprima
//! d'importazione, ripristino di un backup): ogni area viene svuotata alla
//! selezione successiva, alla fine dell'operazione e all'avvio dell'app.

use std::path::{Path, PathBuf};

use tauri::AppHandle;
use tauri_plugin_dialog::FileDialogBuilder;

type CommandResult<T> = Result<T, String>;
type Dialog = FileDialogBuilder<tauri::Wry>;

/// Aree della cartella temporanea, una per tipo di operazione in corso.
#[derive(Debug, Clone, Copy)]
pub enum Area {
    Import,
    Backup,
    /// Usata solo dove l'esportazione passa da una copia temporanea.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    Export,
}

/// Filtro per estensione. Su Android il selettore filtra per tipo MIME e non
/// conosce estensioni come `.enex` o `.age`: lì si può scegliere qualsiasi
/// file e il controllo avviene sul contenuto.
pub fn filtered(builder: Dialog, name: &str, extensions: &[&str]) -> Dialog {
    #[cfg(not(target_os = "android"))]
    {
        builder.add_filter(name, extensions)
    }
    #[cfg(target_os = "android")]
    {
        let _ = (name, extensions);
        builder
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let matches = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension));
    if matches {
        path
    } else {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(extension);
        path.with_file_name(name)
    }
}

/// File in cui il core scrive un'esportazione.
///
/// Su Android il selettore crea il documento di destinazione già al momento
/// della scelta: se l'esportazione non arriva a [`OutputFile::finish`] con
/// successo, il documento rimasto vuoto (o scritto a metà) viene eliminato
/// quando l'`OutputFile` esce di scena.
pub struct OutputFile {
    path: PathBuf,
    #[cfg(target_os = "android")]
    destination: tauri_plugin_dialog::FilePath,
    #[cfg(target_os = "android")]
    app: AppHandle,
    #[cfg(target_os = "android")]
    completed: bool,
}

impl OutputFile {
    /// Percorso su cui scrivere.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(not(target_os = "android"))]
mod platform {
    use super::*;
    use tauri_plugin_dialog::FilePath;

    fn into_path(file: FilePath) -> CommandResult<PathBuf> {
        file.into_path().map_err(|error| error.to_string())
    }

    pub fn pick_input(
        _app: &AppHandle,
        builder: Dialog,
        _area: Area,
    ) -> CommandResult<Option<PathBuf>> {
        builder.blocking_pick_file().map(into_path).transpose()
    }

    pub fn pick_inputs(
        _app: &AppHandle,
        builder: Dialog,
        _area: Area,
    ) -> CommandResult<Option<Vec<PathBuf>>> {
        builder
            .blocking_pick_files()
            .map(|files| files.into_iter().map(into_path).collect())
            .transpose()
    }

    pub fn pick_output(
        _app: &AppHandle,
        builder: Dialog,
        extension: &str,
    ) -> CommandResult<Option<OutputFile>> {
        let Some(path) = builder.blocking_save_file().map(into_path).transpose()? else {
            return Ok(None);
        };
        Ok(Some(OutputFile {
            path: with_extension(path, extension),
        }))
    }

    impl OutputFile {
        /// Conclude il salvataggio e restituisce il nome del file scritto.
        pub fn finish(self, _app: &AppHandle) -> CommandResult<String> {
            Ok(file_name(&self.path))
        }
    }

    pub fn clear(_app: &AppHandle, _area: Area) {}

    pub fn clear_all(_app: &AppHandle) {}

    /// La condivisione con altre app esiste solo su mobile: sul desktop la
    /// nota si copia negli appunti o si esporta.
    pub fn share_text(_app: &AppHandle, _title: &str, _text: &str) -> CommandResult<()> {
        Err(greatnote_core::tr!(
            "La condivisione con altre app non è disponibile su questo sistema",
            "Sharing with other apps is not available on this system"
        ))
    }

    /// Installazione di un aggiornamento scaricato: solo Android (sul
    /// desktop gli aggiornamenti passano dal plugin updater).
    #[cfg(mobile)]
    pub fn install_package(_app: &AppHandle, _path: &Path) -> CommandResult<()> {
        Err(greatnote_core::tr!(
            "Gli aggiornamenti dall'app non sono disponibili su questo dispositivo",
            "In-app updates are not available on this device"
        ))
    }

    /// Apre un file con l'applicazione predefinita del sistema.
    pub fn open_file(app: &AppHandle, path: &Path, _name: &str, _mime: &str) -> CommandResult<()> {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "android")]
mod platform {
    use std::{fs, io};

    use greatnote_core::transfer::sanitize_file_name;
    use tauri::Manager;
    use tauri_plugin_dialog::FilePath;
    use tauri_plugin_fs::{FsExt, OpenOptions};

    use super::*;

    const TRANSFER_DIRECTORY: &str = "transfer";
    /// Cartella delle copie condivise con altre app: deve coincidere con
    /// quella accettata da `FilesPlugin.kt`.
    const OPEN_DIRECTORY: &str = "open";

    /// Componente Kotlin dell'app (`FilesPlugin.kt`) che apre un file con
    /// un'altra app concedendole la sola lettura.
    struct FilesPlugin(tauri::plugin::PluginHandle<tauri::Wry>);

    pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
        tauri::plugin::Builder::new("greatnote-files")
            .setup(|app, api| {
                let handle = api.register_android_plugin("dev.greatnote.notes", "FilesPlugin")?;
                app.manage(FilesPlugin(handle));
                Ok(())
            })
            .build()
    }

    /// Apre un file dell'app con un'altra app. Il file viene prima copiato
    /// in una cartella della cache con il suo nome originale: l'altra app
    /// vede solo quella copia, non la cartella dati.
    pub fn open_file(app: &AppHandle, path: &Path, name: &str, mime: &str) -> CommandResult<()> {
        let directory = app
            .path()
            .app_cache_dir()
            .map_err(message)?
            .join(OPEN_DIRECTORY);
        if let Err(error) = fs::remove_dir_all(&directory)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("copie condivise precedenti non rimosse: {error}");
        }
        fs::create_dir_all(&directory).map_err(message)?;
        let copy = directory.join(sanitize_file_name(name, "file"));
        fs::copy(path, &copy).map_err(message)?;
        app.state::<FilesPlugin>()
            .0
            .run_mobile_plugin::<serde_json::Value>(
                "openFile",
                serde_json::json!({ "path": copy, "mimeType": mime }),
            )
            .map(|_| ())
            .map_err(|error| {
                if error.to_string().contains("no-app") {
                    "Nessuna app installata può aprire questo tipo di file".to_owned()
                } else {
                    format!("Impossibile aprire il file: {error}")
                }
            })
    }

    /// Apre l'installatore di Android con l'APK scaricato in
    /// `<cache>/update/`. Se l'utente non ha ancora permesso a Greatnote di
    /// installare app, `FilesPlugin.kt` apre l'impostazione di sistema e
    /// restituisce `install-permission`.
    pub fn install_package(app: &AppHandle, path: &Path) -> CommandResult<()> {
        app.state::<FilesPlugin>()
            .0
            .run_mobile_plugin::<serde_json::Value>(
                "installPackage",
                serde_json::json!({ "path": path }),
            )
            .map(|_| ())
            .map_err(|error| {
                if error.to_string().contains("install-permission") {
                    greatnote_core::tr!(
                        "Consenti a Greatnote di installare app nella schermata che si è aperta, poi torna qui e premi di nuovo «Aggiorna»",
                        "Allow Greatnote to install apps on the screen that just opened, then come back and tap “Update” again"
                    )
                } else {
                    greatnote_core::tr!(
                        "Impossibile aprire l'installatore: {error}",
                        "Unable to open the installer: {error}"
                    )
                }
            })
    }

    fn message(error: impl ToString) -> String {
        error.to_string()
    }

    fn root(app: &AppHandle) -> CommandResult<PathBuf> {
        Ok(app
            .path()
            .app_cache_dir()
            .map_err(message)?
            .join(TRANSFER_DIRECTORY))
    }

    fn area_directory(app: &AppHandle, area: Area) -> CommandResult<PathBuf> {
        Ok(root(app)?.join(match area {
            Area::Import => "import",
            Area::Backup => "backup",
            Area::Export => "export",
        }))
    }

    pub fn clear(app: &AppHandle, area: Area) {
        if let Ok(directory) = area_directory(app, area)
            && let Err(error) = fs::remove_dir_all(&directory)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("copie temporanee non rimosse: {error}");
        }
    }

    pub fn clear_all(app: &AppHandle) {
        for area in [Area::Import, Area::Backup, Area::Export] {
            clear(app, area);
        }
        if let Ok(cache) = app.path().app_cache_dir() {
            let _ = fs::remove_dir_all(cache.join(OPEN_DIRECTORY));
        }
    }

    /// Nome mostrato dal fornitore del documento, reso sicuro come nome di
    /// file locale.
    fn display_name(app: &AppHandle, file: &FilePath, fallback: &str) -> String {
        let name = app.path().file_name(&file.to_string());
        sanitize_file_name(name.as_deref().unwrap_or(fallback), fallback)
    }

    /// Copia il documento scelto in una sottocartella dedicata, così due
    /// documenti con lo stesso nome non si sovrascrivono.
    fn copy_in(
        app: &AppHandle,
        file: FilePath,
        directory: &Path,
        index: usize,
    ) -> CommandResult<PathBuf> {
        let folder = directory.join(index.to_string());
        fs::create_dir_all(&folder).map_err(message)?;
        let target = folder.join(display_name(app, &file, "file"));
        let mut options = OpenOptions::new();
        options.read(true);
        let mut source = app.fs().open(file, options).map_err(|error| {
            greatnote_core::tr!(
                "Impossibile leggere il file scelto: {error}",
                "Unable to read the selected file: {error}"
            )
        })?;
        let mut copy = fs::File::create(&target).map_err(message)?;
        io::copy(&mut source, &mut copy).map_err(|error| {
            greatnote_core::tr!(
                "Impossibile leggere il file scelto: {error}",
                "Unable to read the selected file: {error}"
            )
        })?;
        Ok(target)
    }

    /// Il testo passa da un processo all'altro in un solo messaggio, che
    /// Android limita a circa 1 MB.
    const MAX_SHARED_CHARS: usize = 100_000;

    /// Mostra il pannello «Condividi» di Android con il testo dato.
    pub fn share_text(app: &AppHandle, title: &str, text: &str) -> CommandResult<()> {
        if text.chars().count() > MAX_SHARED_CHARS {
            return Err(greatnote_core::tr!(
                "La nota è troppo lunga per essere condivisa come testo: esportala in un file",
                "The note is too long to be shared as text: export it to a file"
            ));
        }
        app.state::<FilesPlugin>()
            .0
            .run_mobile_plugin::<serde_json::Value>(
                "shareText",
                serde_json::json!({ "title": title, "text": text }),
            )
            .map(|_| ())
            .map_err(|error| {
                greatnote_core::tr!(
                    "Impossibile condividere: {error}",
                    "Unable to share: {error}"
                )
            })
    }

    pub fn pick_input(
        app: &AppHandle,
        builder: Dialog,
        area: Area,
    ) -> CommandResult<Option<PathBuf>> {
        let Some(file) = builder.blocking_pick_file() else {
            return Ok(None);
        };
        clear(app, area);
        copy_in(app, file, &area_directory(app, area)?, 0).map(Some)
    }

    pub fn pick_inputs(
        app: &AppHandle,
        builder: Dialog,
        area: Area,
    ) -> CommandResult<Option<Vec<PathBuf>>> {
        let Some(files) = builder.blocking_pick_files() else {
            return Ok(None);
        };
        clear(app, area);
        let directory = area_directory(app, area)?;
        files
            .into_iter()
            .enumerate()
            .map(|(index, file)| copy_in(app, file, &directory, index))
            .collect::<CommandResult<Vec<_>>>()
            .map(Some)
    }

    pub fn pick_output(
        app: &AppHandle,
        builder: Dialog,
        extension: &str,
    ) -> CommandResult<Option<OutputFile>> {
        // Il selettore crea subito il documento di destinazione (vuoto).
        let Some(destination) = builder.blocking_save_file() else {
            return Ok(None);
        };
        clear(app, Area::Export);
        let directory = area_directory(app, Area::Export)?;
        fs::create_dir_all(&directory).map_err(message)?;
        let name = display_name(app, &destination, "export");
        Ok(Some(OutputFile {
            path: with_extension(directory.join(name), extension),
            destination,
            app: app.clone(),
            completed: false,
        }))
    }

    impl Drop for OutputFile {
        fn drop(&mut self) {
            if self.completed {
                return;
            }
            clear(&self.app, Area::Export);
            let uri = self.destination.to_string();
            let removed = self
                .app
                .state::<FilesPlugin>()
                .0
                .run_mobile_plugin::<serde_json::Value>(
                    "deleteDocument",
                    serde_json::json!({ "uri": uri }),
                );
            if let Err(error) = removed {
                tracing::warn!("documento di esportazione incompleto non rimosso: {error}");
            }
        }
    }

    impl OutputFile {
        /// Copia il file scritto dal core nel documento scelto dall'utente e
        /// restituisce il nome mostrato dal fornitore del documento.
        pub fn finish(mut self, app: &AppHandle) -> CommandResult<String> {
            let shown = app
                .path()
                .file_name(&self.destination.to_string())
                .unwrap_or_else(|| file_name(&self.path));
            let result = (|| {
                let mut source = fs::File::open(&self.path)?;
                let mut options = OpenOptions::new();
                options.read(false).write(true).truncate(true);
                let mut target = app.fs().open(self.destination.clone(), options)?;
                io::copy(&mut source, &mut target)?;
                target.sync_all()
            })()
            .map_err(|error: io::Error| {
                greatnote_core::tr!(
                    "Impossibile scrivere il file scelto: {error}",
                    "Unable to write the selected file: {error}"
                )
            });
            clear(app, Area::Export);
            self.completed = result.is_ok();
            result.map(|()| shown)
        }
    }
}

#[cfg(target_os = "android")]
pub use platform::init;
#[cfg(mobile)]
pub use platform::install_package;
pub use platform::{clear, clear_all, open_file, pick_input, pick_inputs, pick_output, share_text};
