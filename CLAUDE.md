# CLAUDE.md

Questo file è letto automaticamente da **Claude Code** all'inizio di ogni sessione. Contiene il contesto del progetto e le convenzioni di lavoro. È la prima cosa da leggere.

## Cos'è Argus

Profiler GPU-accelerato per Windows. Si attacca a un processo in esecuzione (senza injection, senza modificarlo) e mostra in real-time CPU, RAM, I/O, thread, hot path delle funzioni, lock contention. Obiettivi non negoziabili: **portatile, affidabile, bello + leggibile**.

## Stato attuale

**Fase 1 (MVP polling) completata.** Scaffold a layer (lib + bin), metriche polling-based real-time, dashboard egui, lista processi via `NtQuerySystemInformation` (raggruppata utente/sistema, ordinabile per CPU/RAM/Nome), sampler thread lock-free (`arc-swap`), error handling `ArgusError`, logging `tracing`, test d'integrazione con binario fixture. Binario release 10.62 MB.

**Fase 2 (ETW + flame graph) implementata e cattura live VERIFICATA come admin.** Flame graph (`aggregation/flame.rs`), symbol resolution DbgHelp (`capture/symbols.rs`), parser eventi + sessione ETW kernel (`capture/etw.rs`), aggregatore ETW→simboli→flame (`capture/profiling.rs`), tab "Flame graph" col Painter di egui (`ui/flame.rs`: zoom/drill/ricerca **regex**/hover). La cattura ETW richiede **admin** (serve `SeSystemProfilePrivilege` abilitato, D20): senza, degrado graceful (banner, polling-only). Verificata end-to-end con `tests/etw_live.rs` (run elevato): 7690 stack reali dal fixture, ordine leaf-first. Resta: nomi funzione del target (approccio on-disk, D14), overhead, resa flame a video.

**Fase 4 (recording + diff + export) completata.** Formato `.argus` binario versionato (`persist.rs`), record/replay (stato `Replay`), diff tra due capture con grafici sovrapposti + "movers" (`diff.rs`, tab Diff), export CSV/folded-stacks/SVG (`export.rs`). UI: Salva/Apri/Esporta/Confronta nella top bar. Tutto coperto da test (round-trip, export, diff).

**Fase 3 (timeline + lock contention): live e verificata.** Parser `CSwitch` (`capture/cswitch.rs`), timeline stati-thread (`aggregation/timeline.rs`) con Running/Ready/Waiting, cattura `CSwitch` via ETW filtrata sui TID del target, tab "Timeline" Gantt (`ui/timeline_view.rs`). **Lock contention** fatta: la causa d'attesa `KWAIT_REASON` è propagata ai segmenti Waiting e categorizzata (Lock/IO/Idle/Preempted, `wait_category`/`wait_breakdown`); la UI mostra "Attese per causa" + tooltip per segmento. Verificata live (admin, `tests/etw_live`: 1535 CSwitch, 4 thread).

**Memoria (Fase 3): hard page fault + VirtualAlloc tracking, validati live.** Provider `PageFault` sullo stesso kernel logger (`capture/memevents.rs` parser puri, `aggregation/memstats.rs`): hard fault (filtrati sui TID del target) e VirtualAlloc/VirtualFree (filtrati sul PID nel payload), sezione "Memoria (ETW)" in Dashboard. **Validato live** (`tests/etw_live`: 19 VirtualAlloc = 304 MB, granularità 16 MB = churn del fixture). È la tracciatura allocazioni **compatibile col vincolo no-injection**; il tracking a livello `HeapAlloc` resta fuori scope perché richiederebbe l'opt-in del target al lancio (IFEO/relaunch) — vedi `07-roadmap.md`.

**Disk I/O detail (Fase 2/3): fatto e validato live.** Provider `DiskIo` sullo stesso kernel logger (`capture/diskio.rs` parser puro, `aggregation/diskstats.rs` aggregazione per direzione/disco), sezione "Disco fisico (ETW)" in Dashboard. Layout `DiskIo_TypedData` **validato live** (`tests/etw_live`: 16.4 MB di scrittura = probe da 16 MB). File-name per operazione + percentili di latenza calibrati: rinviati.

**Overhead misurato (Fase 4/affidabilità).** Harness `tests/overhead.rs` (lavoro fisso con/senza ETW) + `fixture bench`: **2.21%** su un loop CPU-bound stretto (caso peggiore del sampling; il target <1% è per carichi reali con attese). Export **JSON**, **CI** GitHub Actions (windows) e **cargo-deny** aggiunti. Tema scuro Argus (`ui/theme.rs`) applicato.

Comodità CLI: `argus.exe --attach <PID> [--tab flame|timeline|metriche|diff]`. Guida d'uso: [`docs/10-uso.md`](docs/10-uso.md).

Stato test: **54 unit + 3 integration + 2 ignored (admin: `etw_live`, `overhead`) verdi**, clippy/fmt puliti, release ~10.6 MB. Decisioni D1–D25 in [`docs/09-decisions.md`](docs/09-decisions.md); fasi e **lavoro residuo/verifica** in [`docs/07-roadmap.md`](docs/07-roadmap.md); backlog in [`docs/11-backlog.md`](docs/11-backlog.md).

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
