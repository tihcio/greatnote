//! Esportazione PDF della nota visualizzata, senza dialog di stampa.
//!
//! Il PDF è prodotto dal motore della webview con il foglio di stile di
//! stampa dell'app (ADR 0005): WebKitGTK stampa sulla stampante virtuale
//! «Stampa su file» di GTK, WebView2 usa `PrintToPdf`. In entrambi i casi il
//! file viene scritto accanto alla destinazione con un nome temporaneo e
//! rinominato solo a scrittura completata.

use std::path::{Path, PathBuf};
#[cfg(any(windows, all(desktop, target_os = "linux")))]
use std::{sync::mpsc, time::Duration};

use greatnote_core::transfer::sanitize_file_name;
use tauri::{AppHandle, WebviewWindow};

use crate::{
    data_commands::{blocking, dialog},
    file_access::{self, filtered},
};

type CommandResult<T> = Result<T, String>;

/// Tempo massimo concesso al motore per scrivere il PDF.
#[cfg(any(windows, all(desktop, target_os = "linux")))]
const PDF_TIMEOUT: Duration = Duration::from_secs(120);

/// Chiede dove salvare e scrive il PDF della finestra che ha invocato il
/// comando. Restituisce il nome del file, o `None` se l'utente annulla.
#[tauri::command]
pub async fn export_note_pdf(
    app: AppHandle,
    window: WebviewWindow,
    note_title: String,
    title: String,
) -> CommandResult<Option<String>> {
    blocking(app, move |app| {
        let file_name = format!("{}.pdf", sanitize_file_name(&note_title, "note"));
        let builder = filtered(
            dialog(app, &title).set_file_name(&file_name),
            "PDF",
            &["pdf"],
        );
        let Some(output) = file_access::pick_output(app, builder, "pdf")? else {
            return Ok(None);
        };
        let temporary = temporary_path(output.path());
        let result = print_to_pdf(&window, &temporary).and_then(|()| {
            check_written(&temporary)?;
            std::fs::rename(&temporary, output.path()).map_err(|error| error.to_string())
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        output.finish(app).map(Some)
    })
    .await
}

/// File temporaneo nella stessa cartella della destinazione, così la
/// rinomina finale resta atomica.
fn temporary_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export.pdf".to_owned());
    destination.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()))
}

/// Il motore segnala la fine anche quando non ha scritto niente: un PDF
/// valido inizia sempre con `%PDF-`.
fn check_written(path: &Path) -> CommandResult<()> {
    use std::io::Read;
    let mut header = [0_u8; 5];
    let read = std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok();
    if read && &header == b"%PDF-" {
        Ok(())
    } else {
        Err(greatnote_core::tr!(
            "Il PDF non è stato generato",
            "The PDF was not generated"
        ))
    }
}

#[cfg(any(windows, all(desktop, target_os = "linux")))]
fn wait_for(receiver: mpsc::Receiver<CommandResult<()>>) -> CommandResult<()> {
    match receiver.recv_timeout(PDF_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(greatnote_core::tr!(
            "La generazione del PDF non è terminata in tempo",
            "PDF generation did not finish in time"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(greatnote_core::tr!(
            "La generazione del PDF si è interrotta",
            "PDF generation was interrupted"
        )),
    }
}

#[cfg(all(desktop, target_os = "linux"))]
fn print_to_pdf(window: &WebviewWindow, path: &Path) -> CommandResult<()> {
    use std::{cell::RefCell, rc::Rc};

    use webkit2gtk::{PrintOperationExt, glib};

    let uri = glib::filename_to_uri(path, None)
        .map_err(|error| error.to_string())?
        .to_string();
    let (sender, receiver) = mpsc::channel::<CommandResult<()>>();
    window
        .with_webview(move |webview| {
            // La stampante virtuale di GTK ha un nome tradotto nella lingua
            // del sistema: WebKit la cerca per nome e, se non la trova,
            // fallisce senza ripiegare su una stampante reale.
            let printer = glib::dgettext(Some("gtk30"), "Print to File");
            let settings = gtk::PrintSettings::new();
            settings.set_printer(printer.as_str());
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));
            let operation = webkit2gtk::PrintOperation::new(&webview.inner());
            operation.set_print_settings(&settings);
            // Il riferimento tiene viva l'operazione fino al termine e viene
            // rilasciato al primo esito, senza cicli di riferimenti.
            let keep_alive = Rc::new(RefCell::new(Some(operation.clone())));
            let failed_sender = sender.clone();
            let failed_keep = keep_alive.clone();
            operation.connect_failed(move |_, error| {
                let _ = failed_sender.send(Err(error.to_string()));
                failed_keep.borrow_mut().take();
            });
            operation.connect_finished(move |_| {
                let _ = sender.send(Ok(()));
                keep_alive.borrow_mut().take();
            });
            operation.print();
        })
        .map_err(|error| error.to_string())?;
    wait_for(receiver)
}

#[cfg(windows)]
fn print_to_pdf(window: &WebviewWindow, path: &Path) -> CommandResult<()> {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::{ICoreWebView2_7, ICoreWebView2PrintSettings},
        PrintToPdfCompletedHandler,
    };
    use windows::core::{HSTRING, Interface, PCWSTR};

    let target = HSTRING::from(path.as_os_str());
    let (sender, receiver) = mpsc::channel::<CommandResult<()>>();
    let start_sender = sender.clone();
    window
        .with_webview(move |webview| {
            let started = (|| -> windows::core::Result<()> {
                let core =
                    unsafe { webview.controller().CoreWebView2()? }.cast::<ICoreWebView2_7>()?;
                let handler =
                    PrintToPdfCompletedHandler::create(Box::new(move |result, success| {
                        let outcome = match result {
                            Ok(()) if success => Ok(()),
                            Ok(()) => Err(greatnote_core::tr!(
                                "Il PDF non è stato generato",
                                "The PDF was not generated"
                            )),
                            Err(error) => Err(error.message()),
                        };
                        let _ = sender.send(outcome);
                        Ok(())
                    }));
                unsafe {
                    core.PrintToPdf(
                        PCWSTR(target.as_ptr()),
                        None::<&ICoreWebView2PrintSettings>,
                        &handler,
                    )
                }
            })();
            if let Err(error) = started {
                let _ = start_sender.send(Err(error.message()));
            }
        })
        .map_err(|error| error.to_string())?;
    wait_for(receiver)
}

#[cfg(not(any(windows, all(desktop, target_os = "linux"))))]
fn print_to_pdf(window: &WebviewWindow, path: &Path) -> CommandResult<()> {
    let _ = (window, path);
    Err(greatnote_core::tr!(
        "L'esportazione PDF non è disponibile su questo dispositivo",
        "PDF export is not available on this device"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_file_is_hidden_next_to_destination() {
        let destination = Path::new("/tmp/esportazioni/Nota.pdf");
        let temporary = temporary_path(destination);
        assert_eq!(temporary.parent(), destination.parent());
        let name = temporary
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert!(name.starts_with(".Nota.pdf."));
        assert!(name.ends_with(".tmp"));
    }

    #[test]
    fn rejects_files_that_are_not_pdf() {
        let directory = tempfile::tempdir().expect("cartella temporanea");
        let empty = directory.path().join("vuoto.pdf");
        std::fs::write(&empty, b"").expect("scrittura");
        assert!(check_written(&empty).is_err());
        assert!(check_written(&directory.path().join("assente.pdf")).is_err());
        let valid = directory.path().join("valido.pdf");
        std::fs::write(&valid, b"%PDF-1.7\n").expect("scrittura");
        assert!(check_written(&valid).is_ok());
    }
}
