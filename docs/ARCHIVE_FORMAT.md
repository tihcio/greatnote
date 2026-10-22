# Formati di esportazione e backup

Tutti i formati sono ZIP standard con voci in UTF-8. I nomi interni sono
generati dall'app; in lettura vengono convalidati e non sono mai usati
direttamente come percorsi.

## Archivio completo `gna-archive` v1

Prodotto da *Dati e backup → Esporta → Esporta archivio completo* e
reimportabile con *Importa → Archivio Greatnote*.

```text
manifest.json
notes/<uuid-nota>.json
attachments/<uuid-allegato>.<png|jpg|gif|pdf|txt>
```

`manifest.json`:

| Campo | Descrizione |
| --- | --- |
| `format` | sempre `"gna-archive"` |
| `version` | versione del formato, attualmente `1` |
| `exported_at` | timestamp UTC ISO 8601 |
| `schema_version` | versione dello schema SQLite di origine |
| `app_version` | versione dell'app |
| `spaces`, `notebooks`, `tags` | elenchi completi con `id`, nomi, descrizioni, ordinamento e date |
| `notes`, `attachments` | voci `{ "path", "sha256", "size" }` per ogni file |
| `planner` | facoltativo: `tasks`, `events`, `reminders` (vedi sotto) |

Nel `planner`, ogni attività può avere `recurrence`: la regola di
ripetizione in JSON (`frequency` `daily`/`weekly`/`monthly`/`yearly`,
`interval` 1–99, `weekdays` 1–7 per le settimanali, `month_day` per mensili
e annuali, `until` data di fine facoltativa). In importazione una regola non
valida per la scadenza dell'attività viene scartata; l'attività resta.

Ogni `notes/<id>.json` contiene: `id`, `notebook_id`, `title`, `content`
(documento JSON Tiptap v1; le immagini sono nodi `image` con `attachmentId`
riferito a un allegato della stessa nota, `width` facoltativa e
`annotations`: forme `pen`, `highlighter`, `arrow`, `rect`, `ellipse`,
`text` con `color` esadecimale, `size` relativa alla larghezza e `points`
in coordinate relative 0–1; i testi cifrati sono nodi `encryptedText` con
`ciphertext` nel formato testuale di age e `hint` facoltativo), `created_at`, `updated_at`, `is_pinned`,
`archived_at`, `tag_ids`, `links` (`target_note_id`, `label`) e
`attachments` (`id`, `original_filename`, `mime_type`, `size_bytes`,
`sha256`, `created_at`, `path`).

Regole di importazione:

- dimensione e SHA-256 di ogni voce devono coincidere con il manifest;
- note con ID già presente (anche nel cestino) vengono saltate;
- spazi e taccuini esistenti con lo stesso ID vengono riusati; un tag con lo
  stesso nome (senza distinzione di maiuscole) viene riusato;
- i collegamenti vengono creati solo se entrambe le note esistono;
- le note nel cestino non sono esportate.

## ZIP Markdown

Prodotto da *Esporta → Esporta ZIP Markdown* per un taccuino o per tutte le
note:

```text
<Taccuino>/<Titolo>.md
<Taccuino>/<Titolo> (2).md
<Taccuino>/attachments/<Titolo>/<nome-originale>
```

Ogni file inizia con un front matter i cui valori sono JSON (quindi anche YAML
valido):

```markdown
---
title: "Piano annuale"
created: "2026-01-02T03:04:05.678Z"
updated: "2026-02-03T04:05:06.000Z"
notebook: "Lavoro"
tags: ["progetto","budget"]
attachments: ["attachments/Piano annuale/budget.pdf"]
---

# Piano annuale

Contenuto in CommonMark…
```

Grassetto, corsivo, barrato, codice, link, elenchi, citazioni, blocchi di
codice e separatori usano la sintassi CommonMark/GFM; il sottolineato usa
`<u>…</u>`. I nomi file sono resi portabili tra Linux e Windows. Estraendo lo
ZIP e importando la cartella, Greatnote ripristina titoli, date, tag e allegati
referenziati.

## Backup `gna-backup` v1

Creato in `backups/` nella cartella dati dell'app con nome
`greatnote-<manual|auto|premigration|prerestore>-<AAAAMMGGTHHMMSSZ>-<id>.zip`
(i backup creati prima del cambio di nome usano il prefisso `gna-` e restano
riconosciuti). Gli identificatori `gna-archive`, `gna-backup` e il file
`gna.sqlite3` sono stabili e non cambiano con il nome dell'app.

```text
manifest.json
database/gna.sqlite3
attachments/<uuid>.<estensione>
```

Il manifest contiene `format` (`"gna-backup"`), `version`, `created_at`,
`kind`, `schema_version`, `app_version` e `files` con percorso, SHA-256 e
dimensione. Il database è una copia coerente creata con `VACUUM INTO`.
I backup nella cartella dati non sono cifrati.

### Copia cifrata (`.zip.age`)

Una copia esportata con password è lo stesso ZIP di backup cifrato nel
formato [age v1](https://age-encryption.org/v1) con un unico destinatario
scrypt (password). Il file inizia con `age-encryption.org/v1` ed è
decifrabile con qualunque implementazione conforme, per esempio:

```sh
age -d greatnote-manual-….zip.age > backup.zip
```

L'app riconosce il formato dall'header, indipendentemente dall'estensione.
Sono accettati solo file cifrati con password (non con chiavi X25519/SSH).
