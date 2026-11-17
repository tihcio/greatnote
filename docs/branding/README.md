# Logo e icone

- `../../src-tauri/icons/icon.png` (512×512): sorgente di tutte le icone.
  Il disegno occupa quasi tutto il riquadro.
- `logo.png` (1024×1024): versione con ampi margini trasparenti, non
  adatta come sorgente delle icone.
- `android-foreground.png`: primo piano dell'icona adattiva Android, con il
  logo ridotto al 71% e centrato. Android ritaglia l'icona con una maschera
  (cerchio, goccia, quadrato arrotondato) che mostra solo la parte centrale:
  così il logo non viene tagliato.
- `icons.json`: manifest per `tauri icon`.

## Rigenerare le icone

Le icone desktop sono già in `src-tauri/icons`. Per Android e per i
loghi del pacchetto MSIX:

``` powershell
npx tauri icon docs/branding/icons.json -o $env:TEMP\greatnote-icons
# Android: copia le cartelle generate nelle risorse dell'app
Copy-Item -Recurse -Force $env:TEMP\greatnote-icons\android\* src-tauri\gen\android\app\src\main\res\
```

Non generare le icone direttamente in `src-tauri/icons` senza
controllarle: `tauri icon` sovrascrive anche quelle desktop.
