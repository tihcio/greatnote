#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = greatnote_desktop_lib::run() {
        eprintln!("Impossibile avviare Greatnote: {error}");
        std::process::exit(1);
    }
}
