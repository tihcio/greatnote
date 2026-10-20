fn main() {
    // Registrazioni OAuth dell'editore per i calendari esterni, incorporate
    // in fase di compilazione e mai salvate nel repository (ADR 0015).
    for name in [
        "GREATNOTE_GOOGLE_CLIENT_ID",
        "GREATNOTE_GOOGLE_CLIENT_SECRET",
        "GREATNOTE_MICROSOFT_CLIENT_ID",
        "GREATNOTE_DROPBOX_APP_KEY",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    tauri_build::build()
}
