#!/bin/sh
# Installa (o rimuove con --remove) una voce .desktop per la build di sviluppo,
# nella cartella dati dell'utente. Su Wayland KDE e GNOME ricavano icona e
# nome della finestra dal file .desktop con lo stesso app_id della finestra
# (dev.greatnote.notes): senza, mostrano un'icona generica.
# La voce è nascosta dal menu (NoDisplay) e non richiede privilegi.
set -eu

APP_ID=dev.greatnote.notes
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
DESKTOP_FILE="$DATA/applications/$APP_ID.desktop"
ICON_FILE="$DATA/icons/hicolor/512x512/apps/$APP_ID.png"

refresh() {
    update-desktop-database "$DATA/applications" 2>/dev/null || true
    gtk-update-icon-cache -q -t "$DATA/icons/hicolor" 2>/dev/null || true
    kbuildsycoca6 --noincremental >/dev/null 2>&1 || kbuildsycoca5 >/dev/null 2>&1 || true
}

if [ "${1:-}" = "--remove" ]; then
    rm -f "$DESKTOP_FILE" "$ICON_FILE"
    refresh
    echo "Voce di sviluppo rimossa."
    exit 0
fi

install -Dm644 "$ROOT/src-tauri/icons/icon.png" "$ICON_FILE"
mkdir -p "$DATA/applications"
cat > "$DESKTOP_FILE" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Greatnote
Comment=Note locali (build di sviluppo)
Exec="$ROOT/target/debug/greatnote-desktop"
Icon=$APP_ID
StartupWMClass=$APP_ID
Terminal=false
NoDisplay=true
Categories=Office;
DESKTOP
refresh
echo "Installati $DESKTOP_FILE e l'icona $ICON_FILE."
echo "Riavvia 'npm run tauri:dev' per vedere l'icona nella barra delle applicazioni."
