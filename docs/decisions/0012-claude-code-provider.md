# ADR 0012: Claude Code come provider AI

- **Stato:** accettata
- **Data:** 2026-10-19

## Contesto

L'utente usa Claude Code (CLI) sul proprio computer e vuole usarlo come
provider dell'assistente, con l'account già configurato, senza gestire una
chiave API in Greatnote.

## Decisione

- Nuovo tipo di provider nelle impostazioni (`provider: "claude-code"`),
  accanto ai server compatibili OpenAI. Implementazione in
  `src/ai/claude_code.rs` dietro lo stesso trait `AiProvider` (ADR 0010).
- Ogni richiesta avvia `claude -p` con output `stream-json` e messaggi
  parziali per lo streaming, e con queste restrizioni:
  - `--tools ""`: nessuno strumento (niente lettura di file, comandi o web).
    È la difesa principale: anche un'istruzione nascosta in una nota non può
    far agire Claude Code sul computer.
  - `--safe-mode`, `--strict-mcp-config`, `--disable-slash-commands`: niente
    hook, plugin, server MCP, CLAUDE.md o comandi personalizzati.
  - `--no-session-persistence`: la conversazione non resta nella cronologia
    di Claude Code.
  - `--system-prompt`: le istruzioni di Greatnote sostituiscono quelle di
    Claude Code.
  - `--permission-prompts none`: nulla resta in attesa di conferma.
  - Cartella di lavoro temporanea vuota (0700), rimossa alla fine.
- Il testo delle note arriva dallo standard input, mai dalla riga di
  comando (visibile agli altri processi). La cronologia della chat diventa
  una trascrizione prima dell'ultima domanda.
- I dati vanno ad Anthropic secondo l'account dell'utente: il provider è
  **remoto** e richiede il consenso esplicito come gli altri server
  remoti; le anteprime di invio restano attive.
- "Verifica Claude Code" usa `claude --version` e `claude auth status`:
  nessuna richiesta al modello; dell'accesso si mostra solo il metodo, non
  email o organizzazione.
- Modelli: alias accettati da `--model` (`sonnet`, `opus`, `haiku`, `fable`)
  o il predefinito dell'account.
- Annullamento e tempo massimo terminano il processo.
- L'eseguibile si cerca nel `PATH` e nelle cartelle d'installazione
  abituali, perché le app avviate dal desktop possono avere un `PATH`
  ridotto; in alternativa si indica un percorso assoluto.

## Conseguenze

- Claude Code non fornisce embedding: con questo provider la ricerca per
  significato non è disponibile e l'assistente recupera le note con la
  ricerca testuale.
- Ogni richiesta avvia un processo (circa 1–3 s in più rispetto a un server
  HTTP).
- Il formato `stream-json` è un'interfaccia della CLI: se cambiasse, gli
  errori sarebbero mostrati come tali. I test usano un eseguibile finto;
  `tests/ai_live.rs` contiene una prova reale facoltativa.
- Su Windows i wrapper `claude.cmd` potrebbero richiedere `cmd /c`: da
  verificare con il porting.
