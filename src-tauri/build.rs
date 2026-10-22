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
    // Su Linux tauri.linux.conf.json include il motore llama.cpp, preparato
    // da scripts/fetch-llama.sh prima di `tauri build`/`tauri dev`. Per le
    // compilazioni che non lo scaricano (cargo check/clippy/test, CI) basta
    // un segnaposto: l'app cercherà `llama-server` nel PATH (ADR 0024).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let llama = std::path::Path::new("resources/llama");
        if !llama.exists() {
            let _ = std::fs::create_dir_all(llama);
            let _ = std::fs::write(
                llama.join("README.txt"),
                "Motore non incluso: eseguire scripts/fetch-llama.sh\n",
            );
        }
        println!("cargo:rerun-if-changed=resources/llama/VERSION.txt");
    }
    tauri_build::build()
}
