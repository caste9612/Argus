# 08 – Sviluppo

Come si lavora sul progetto. Riferimento operativo quotidiano.

## Setup

### Prerequisiti

- Windows 10 1903+ o Windows 11
- Rust 1.92+ (`rustup install stable`)
- (Opzionale) Git, `gh` CLI per push/PR
- (Opzionale) VS Code con `rust-analyzer`

### Clone

```powershell
git clone https://github.com/<user>/argus.git
cd argus
```

### Build (quando esisterà il codice)

```powershell
cargo build              # debug build (veloce, simboli completi)
cargo build --release    # release build (lento, ottimizzato)
cargo run --release      # esegui release
```

## Comandi quotidiani

```powershell
cargo build              # build debug
cargo build --release    # build release
cargo run --release      # esegui
cargo test               # unit + integration test
cargo clippy --all-targets -- -D warnings   # lint strict
cargo fmt --check        # check format
cargo fmt                # auto-format
cargo bloat              # analizza dimensione binario
```

## Workflow git

### Branch convention

- `main` — sempre buildabile e funzionante
- `feat/<name>` — nuove feature
- `fix/<name>` — bug fix
- `docs/<name>` — solo documentazione
- `refactor/<name>` — ristrutturazione senza cambio comportamento

### Commit message

Imperativo, < 72 char prima riga, prefisso area:

```
feat(capture): add ETW session manager for PerfInfo provider

Multi-line body se serve, spiegando *why* non *what*.

Risolve #N.
```

Prefissi standard: `feat`, `fix`, `docs`, `refactor`, `test`, `perf`, `chore`.

### PR

Quando esisterà un workflow PR:
- Linka l'issue
- Descrizione: cosa cambia + perché + come testarla
- Checklist: clippy ok, fmt ok, test ok, doc aggiornata se serve

## Code style

### Rust

- `cargo fmt` con `rustfmt.toml` default
- Naming idiomatic Rust (no `m_` prefix, no Hungarian)
- Funzioni > 60 righe → estrai
- File > 500 righe → split in moduli

### Unsafe

- Confinato in `util/win.rs` e `capture/etw.rs`
- Ogni blocco `unsafe` con commento `// SAFETY: …` che spieghi l'invariante
- Wrapper RAII per ogni risorsa Win32 (HANDLE, ETW session, ecc.)

### Commenti

- **Why** non **what** — il codice spiega cosa, il commento spiega perché
- Codice non-ovvio: 1 riga di motivazione
- API pubbliche: doc comment `///` con esempi se utile
- Linguaggio: italiano OK per commenti interni; **inglese** per doc comment di API pubbliche (se mai aggiungiamo)

## Test

### Eseguire

```powershell
cargo test                       # tutti
cargo test capture::polling      # solo modulo capture::polling
cargo test --release             # in release (più veloce, più simile a prod)
```

### Scrivere

- **Unit test** in `#[cfg(test)] mod tests` in fondo al file (vicino al codice)
- **Integration test** in `tests/`
- **Target app fixture** per test integration: `tests/fixtures/target_app/` (un piccolo binario nostro con comportamenti deterministici)

### Coverage (Fase 3+)

`cargo llvm-cov`, soglia target 70 % sui moduli `capture/` e `aggregation/`.

## Lavorare con Claude Code

Vedi `CLAUDE.md` in radice per le convenzioni dettagliate.

### Regola d'oro

Quando Claude (o un contributor umano) propone un cambio di architettura, **aggiorna il documento rilevante in `docs/` PRIMA** di scrivere codice. I docs sono la **fonte di verità**.

Se i docs sono ambigui o incompleti per il task corrente, **chiarisci con l'utente** invece di indovinare.

### Sessione tipica con Claude Code

1. Apri Claude Code nella root del repo
2. Claude legge `CLAUDE.md` automaticamente
3. Spieghi il task in una frase
4. Claude legge i `docs/` rilevanti
5. Claude propone approccio breve
6. Approvi, Claude implementa in piccoli step (compile + test ad ogni step)
7. Se cambia una decisione architetturale, Claude aggiorna `docs/` prima

## Release process (Fase 3+)

1. Bump version in `Cargo.toml`
2. Aggiorna `CHANGELOG.md`
3. Tag git: `git tag v0.x.y`
4. GitHub Actions builda artifact `.zip`
5. Crea GitHub Release con allegato + checksums

## Debug di Argus stesso

Se Argus crasha:
- File log: `%LOCALAPPDATA%\Argus\argus.log`
- Backtrace: `$env:RUST_BACKTRACE='1'; cargo run --release`
- WER (Windows Error Reporting) ok per crash dump

Se Argus è lento:
- Profilarlo con se stesso (meta-profiling, divertente)
- O con WPA come ground truth

## Documentazione: come si scrive

Quando aggiungi/modifichi un `docs/`:

- **Brevità > esaustività**: meglio 100 righe chiare di 500 righe esaustive
- **Esempi concreti**: una snippet vale 10 righe di prosa
- **Tabella se le opzioni sono > 3** (più scannable)
- **Aggiorna l'indice in `README.md`** se aggiungi un nuovo doc
- **Niente date hardcoded** (eccetto in `CHANGELOG.md`)
