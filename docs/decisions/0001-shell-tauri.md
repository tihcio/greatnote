# ADR 0001: shell desktop con Tauri 2

- **Stato:** accettata
- **Data:** 2026-10-13

## Contesto

Il progetto richiede un'applicazione Rust desktop Linux inizialmente e una
possibile portabilità Windows, con frontend web accessibile e confini di
sicurezza espliciti.

## Decisione

Usare Tauri 2 con React e TypeScript. La logica di dominio e lo storage restano
in un crate Rust indipendente. Il frontend usa HTML/CSS standard e la finestra
mantiene le decorazioni del window manager; non vengono usate API KDE o
dipendenze da un desktop environment specifico.

## Conseguenze

- Il layout React è indipendente da KDE/GNOME; la shell usa WebKitGTK su Linux
  e il motore supportato su Windows.
- La build Linux richiede le librerie native GTK/WebKitGTK.
- Le funzioni native future vanno implementate dietro servizi Rust dedicati,
  con permessi minimi.
