# CLAUDE.md

Questo file è letto automaticamente da **Claude Code** all'inizio di ogni sessione. Contiene il contesto del progetto e le convenzioni di lavoro. È la prima cosa da leggere.

## Cos'è Argus

Profiler GPU-accelerato per Windows. Si attacca a un processo in esecuzione (senza injection, senza modificarlo) e mostra in real-time CPU, RAM, I/O, thread, hot path delle funzioni, lock contention. Obiettivi non negoziabili: **portatile, affidabile, bello + leggibile**.

## Stato attuale

**Fase 1 (MVP polling) completata.** Scaffold a layer (lib + bin), metriche polling-based real-time, dashboard egui, lista processi via `NtQuerySystemInformation` (raggruppata utente/sistema, ordinabile per CPU/RAM/Nome), sampler thread lock-free (`arc-swap`), error handling `ArgusError`, logging `tracing`, test d'integrazione con binario fixture. Binario release 10.62 MB.

**Fase 2 (ETW + flame graph) implementata, cattura live da verificare come admin.** Aggiunti: flame graph (`aggregation/flame.rs`), symbol resolution DbgHelp con cache (`capture/symbols.rs`), parser eventi + sessione ETW kernel (`capture/etw.rs`), aggregatore che lega ETW→simboli→flame (`capture/profiling.rs`), tab "Flame graph" interattiva renderizzata col Painter di egui (`ui/flame.rs`: zoom/drill/ricerca/hover). 17 unit + 3 integration test verdi, clippy/fmt puliti. La cattura ETW reale richiede **privilegi di amministratore**: senza, l'app degrada con grazia (banner "ETW non disponibile", polling-only). **Verifica end-to-end come admin ancora da fare** — vedi DoD Fase 2.

Per il recap completo delle decisioni (D1–D17) e lo stato vedi [`docs/09-decisions.md`](docs/09-decisions.md); per le fasi [`docs/07-roadmap.md`](docs/07-roadmap.md).

## Documenti da leggere prima di toccare codice

Quando il task tocca una di queste aree, **leggi il doc rilevante PRIMA** di proporre/scrivere codice:

| Area del task | Documento |
|---|---|
| Decisione architetturale | `docs/02-architecture.md` |
| Aggiungere/modificare una metrica | `docs/04-metrics.md` |
| UI / componenti grafici | `docs/05-ui-design.md` |
| Error handling, edge cases | `docs/06-reliability.md` |
| Quale fase è in corso | `docs/07-roadmap.md` |
| Setup, build, test | `docs/08-development.md` |

Se i docs sono ambigui o incompleti per il task, **chiedi** invece di indovinare. Aggiorna il doc rilevante prima di scrivere codice.

## Stack — riferimento veloce

- **Linguaggio**: Rust 2021, MSRV 1.92
- **UI**: `eframe` con backend wgpu (NON glow)
- **API Windows**: `windows` crate (NON `winapi`)
- **Sync**: `parking_lot`, `arc-swap`, `crossbeam-channel`
- **ETW** (Fase 2+): `ferrisetw` + fallback `windows-rs` raw per gap
- **Async**: `tokio` solo se inevitabile — preferire thread + canali

## Regole di codice

### Sicurezza
- **No `unsafe`** salvo nei wrapper Win32. Isolare in funzioni piccole con commento `// SAFETY: ...` che spieghi l'invariante
- **No `.unwrap()`** in codice di produzione su `Result`/`Option` da API esterne — usa `?` o gestisci esplicitamente. Eccezione: setup iniziale pre-main loop, con commento
- **No `panic!()` reachable**. Vedi no-panic policy in `docs/06-reliability.md`

### Performance
- **Allocazioni in hot path**: evitarle. Riusa buffer, `VecDeque` con capacità prefissata, `SmallVec` per piccoli array
- **Locking**: preferire lock-free (channel, atomic, `arc-swap`) per la pipeline sampler→UI
- **Budget**: overhead < 1% sul target, 60 fps stabili, < 300 MB RAM stato stazionario, < 500 ms startup

### Stile
- Identifier in inglese (idiom Rust), doc-comment e commenti in italiano OK
- Funzioni > 60 righe → estrai. File > 500 righe → split
- Errori utente con contesto: se attach fallisce, l'utente deve sapere *perché* e cosa fare

### Quality gates pre-commit
- `cargo clippy --all-targets -- -D warnings` deve passare
- `cargo fmt --check` deve passare
- `cargo test` deve passare
- Commit message: imperative, < 72 char prima riga, prefisso tipo `feat(capture):`, `fix(ui):`, `docs:`

## Cosa NON fare

- ❌ Aggiungere dipendenze "comode" senza necessità (ogni crate aumenta superficie, build time, binario, supply chain)
- ❌ Astrarre prematuramente — 3 righe simili vanno meglio di un trait
- ❌ Lavorare sull'UI prima che la metrica sottostante sia stabile
- ❌ Aggiungere `#[allow(...)]` senza commento che spieghi perché
- ❌ Cambiare lo stack tecnologico (egui→altro, wgpu→altro) senza discussione esplicita
- ❌ Mockare ciò che si può testare con un binario reale di test (vedi `docs/06-reliability.md`)

## Workflow quando inizi un task

1. Leggi questo file
2. Leggi i doc in `docs/` rilevanti all'area
3. Se serve, fai domande di chiarimento — non indovinare
4. Proponi un approccio breve PRIMA di scrivere molto codice
5. Implementa in piccoli step, verificando ad ogni step (compile + test)
6. Aggiorna il doc se hai imparato qualcosa di nuovo o se hai cambiato una decisione

## Comandi quick reference

```powershell
cargo build --release       # build ottimizzato (quando esisterà)
cargo run --release         # esegui
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo test
```

## Nota al futuro Claude

Il progetto è scritto per **te** in futuro, oltre che per Wcast. Se trovi qualcosa di poco chiaro qui o nei `docs/`, **aggiorna il documento** invece di ricostruire il contesto dalla conversazione. Questi file sono la **fonte di verità** — non la chat history.
