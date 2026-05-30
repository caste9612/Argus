# 07 – Roadmap

Fasi sequenziali. Ogni fase ha **obiettivo**, **deliverable**, **definition of done**.

## Fase 0 — Foundation ✅ completata

**Obiettivo**: documentazione completa, repository inizializzato, baseline pronta per implementazione.

**Deliverable**:
- ✅ `README.md` con vision e link
- ✅ `CLAUDE.md` per sessioni Claude Code
- ✅ Documenti in `docs/` (01-08)
- ✅ Repository git inizializzato
- ✅ Repo GitHub con primo commit (github.com/caste9612/Argus)

**DoD**: l'utente può clonare il repo e capire dove iniziare. Una nuova sessione di Claude Code può leggere `CLAUDE.md` + `docs/` e iniziare la Fase 1 senza altra context.

---

## Fase 1 — MVP polling ✅ completata

**Obiettivo**: attaccarsi a un processo Win64 e visualizzare le metriche polling-based in real-time.

**Deliverable**:
- ✅ Setup progetto Rust (lib + bin, layout moduli da `02-architecture.md`)
- ✅ Process list + ricerca + ordinamento (via `NtQuerySystemInformation`, non ToolHelp — vedi `09-decisions.md` D7)
- ✅ Attach by PID con error handling completo (access denied → suggerimento admin)
- ✅ Sampler thread @ 10 Hz, lock-free verso UI (`arc-swap`)
- ✅ Tutte le 7 metriche (CPU, working set, private bytes, I/O R/W, threads, handles)
- ✅ Dashboard con KPI cards + grafici time-series (egui_plot)
- ✅ Detach + handle cleanup pulito (RAII)
- ✅ Release single-file via `cargo build --release`
- ✅ Logging tracing su file (rotazione giornaliera, filtrato per target)
- ✅ Test d'integrazione con binario fixture deterministico (`src/bin/fixture.rs` + `tests/integration.rs`)

**DoD**:
- ✅ Attacchi un processo e vedi le metriche aggiornarsi in real-time
- ⚠️ RAM: ~304 MB a riposo (include overhead driver GPU/wgpu; strutture dati di Argus <1 MB). Budget <200 MB da rivedere — vedi `09-decisions.md`
- ✅ `cargo clippy --all-targets -- -D warnings` pulito; `cargo test` verde
- ✅ Binario release **10.62 MB** (< 15 MB target)
- Edge case di `06-reliability.md`: target exit, access denied, PID inesistente, apertura di sé → coperti; GPU device lost / low-memory ancora da testare

---

## Fase 2 — ETW + flame graph 🔨 in corso (implementata, cattura live da verificare admin)

**Obiettivo**: aggiungere cattura kernel-level e visualizzazione del hot path. Questo è il pezzo che **insegna ottimizzazione**.

**Deliverable**:
- ✅ ETW session manager (NT Kernel Logger, `EVENT_TRACE_FLAG_PROFILE`) — `capture/etw.rs`
- ✅ Stack walk capture (`TraceSetInformation(TraceStackTracingInfo)` su SampleProfile)
- ✅ Symbol resolution (DbgHelp wrapper RAII) con cache (2 generazioni) — `capture/symbols.rs`
- ✅ Flame graph aggregator (tree con count incrementale) — `aggregation/flame.rs`
- ✅ Flame graph renderer — **con il Painter di egui**, non wgpu custom (D17); sufficiente e verificabile
- ✅ Search box (regex case-insensitive), zoom, click-to-drill — `ui/flame.rs`
- ✅ Tab "Flame" nella dashboard
- ⬜ Provider `Thread` per context switch — rinviato (è preparazione Fase 3)

**Stato verifica**: tutto compila, clippy/fmt/test verdi (17 unit + 3 integration), e l'app
gira mostrando il flame graph e il degrado graceful "ETW non disponibile" senza admin
(verificato a video). La **cattura ETW reale richiede admin** e va collaudata con un run
elevato — è l'unica parte non verificabile in un ambiente non elevato.

**DoD**:
- ✅ Attacchi a un processo CPU-bound, vedi il flame popolarsi — **verificato** (run elevato, `tests/etw_live.rs`): 7690 stack reali dal fixture, profondità fino a 99, ordine leaf-first confermato → flame orientato bene (vedi D20)
- ✅/⏳ Click su un frame zooma — logica layout/focus unit-testata; resa visiva con dati reali da provare a video da admin
- 🔶 Symbol resolution: moduli di sistema risolti coi nomi (ntdll/kernel32); **nomi funzione del target** da migliorare con l'approccio on-disk (D14) — ora `modulo!0xADDR`
- ⏳ Overhead totale sul target < 1 % — *da misurare come admin*
- ✅ Edge cases ETW di `06-reliability.md`: "ETW fails (permessi) → polling-only + banner" verificato (no-admin) **e** cattura live verificata (admin)

**Rifiniture rimaste**: nomi funzione del target via risoluzione *on-disk* (eventi Image/Load,
D14); misura overhead; resa visiva del flame con dati reali a video.

---

## Fase 3 — Allocations + locks + timeline 🔨 fondamenta pronte

**Obiettivo**: visualizzare comportamento dinamico (allocazioni, contesa, thread states).

**Deliverable**:
- ✅ Parser eventi `CSwitch` (provider `Thread`) — puro e testato (`capture/cswitch.rs`)
- ✅ Struttura dati timeline stati thread — intervalli Running per thread, testata (`aggregation/timeline.rs`)
- ⬜ Cattura `CSwitch` live (estendere la sessione ETW con `EVENT_TRACE_FLAG_CSWITCH`) — *admin-gated*
- ⬜ Mappatura TID→PID per filtrare i thread del target (Toolhelp `TH32CS_SNAPTHREAD` o eventi Thread)
- ⬜ Timeline (Gantt) renderizzata con il Painter di egui (come il flame, D17)
- ⬜ Heap allocation tracking (provider `HeapTrace`/`Kernel-Memory`) + allocation flame graph
- ⬜ Lock contention detection (analisi `CSwitch` su wait object) + pannello "Locks"

**Stato**: le fondamenta pure (parser + struttura dati) sono fatte e testate. Il resto è
in larga parte *admin-gated* (ETW kernel) come la Fase 2 → vedi "Lavoro residuo" sotto.

**DoD**:
- ⏳ Vedi un thread bloccato su lock con indicazione visiva chiara
- ⏳ Identifichi allocazioni hot path con stack trace
- ⏳ Cumulative allocations per stack visibile
- ⏳ Timeline scrub fluido anche con 30 thread × 60 s

---

## Fase 4 — Recording + diff + export ✅ completata

**Obiettivo**: trasformare Argus da live-only a strumento di analisi post-mortem.

**Deliverable**:
- ✅ Formato file `.argus` proprietario (binario, versioned con magic header) — `persist.rs`. *zstd rinviato* (D18)
- ✅ Record + replay dell'intera sessione (Salva/Apri, stato `Replay`)
- ✅ Modalità "diff" tra due capture (grafici sovrapposti + funzioni "movers") — `diff.rs`, tab Diff
- ✅ Export (CSV time series, folded stacks per speedscope, SVG flame) — `export.rs`. *PNG/JSON non fatti*
- ⬜ Snapshot manuali / sharing HTML (l'SVG è già condivisibile; resto non fatto)

**DoD**:
- ✅ Catturi una sessione, salvi, riapri, vedi tutto come live (replay)
- ✅ Confronti due capture con grafici sovrapposti (tab Diff)
- ✅ Export SVG di un flame graph apribile in browser

**Verifica**: tutto coperto da unit test (round-trip formato, export, diff) + UI verificata
a video. La UI di replay/diff con **dati di flame reali** dipende dalla cattura ETW (admin).

---

## Fase 5 — Hardware counters + GPU profiling (esplorativa)

**Obiettivo**: scendere ai contatori hardware e profiler GPU.

**Deliverable** (provisori, da rivalutare prima di iniziare):
- Wrapper per PMU counters (cache miss, branch miss) — richiede driver o SDK vendor
- GPU profiling via NVML + ETW DXGI events
- Pannello dedicato "Hardware"
- Eventualmente: integrazione con NVIDIA Nsight Aftermath API

**DoD**: TBD — dipende da quanto driver lavoro è realmente fattibile in user mode.
**Stato**: non iniziata. Richiede driver/SDK vendor (NVML, PMU) e privilegi → fuori
dalla portata di un ambiente non elevato; resta nel "lavoro residuo" sotto.

---

## Lavoro residuo e verifica (handoff)

Stato sintetico a fine del lavoro autonomo. **Verde = fatto e verificato**
(build+clippy+fmt+test, e UI provata a video dove applicabile).

### ✅ Fatto e verificato
- **Fase 1** completa (polling, dashboard, lista processi).
- **Fase 2** implementata: flame graph, symbol resolution, parser+sessione ETW,
  aggregatore, tab Flame (zoom/drill/ricerca regex/hover). Degrado graceful senza admin.
- **Fase 4** completa: formato `.argus`, record/replay, diff, export (CSV/folded/SVG).
- **Fase 3** fondamenta: parser `CSwitch` + struttura dati timeline.
- 40 unit + 3 integration test verdi, clippy/fmt puliti, release 10.62 MB.

### ✅ Cattura ETW live — VERIFICATA come amministratore
Con il fix del privilegio (D20) la pipeline ETW è stata collaudata end-to-end con un run
elevato (`tests/etw_live.rs`, ri-eseguibile: `cargo test --test etw_live -- --ignored`):
7690 stack reali dal fixture, tutti del target, ordine leaf-first confermato → flame
orientato bene. Resta da provare **a video** la resa del flame nella GUI con un processo
reale (apri la tab Flame come admin) e da **misurare l'overhead** (< 1 %). I **nomi
funzione del target** richiedono ancora l'approccio on-disk (D14): ora si vede
`modulo!0xADDR`.

### ⬜ Da implementare per chiudere le fasi (con indicazioni)
- **Fase 2 rifinitura**: risoluzione simboli del target *on-disk* via eventi ETW
  Image/Load (più robusta dell'handle vivo, vedi D14).
- **Fase 3 timeline live**: estendere `EtwProfiler` con `EVENT_TRACE_FLAG_CSWITCH`,
  instradare i `CSwitch` (già parsabili) verso un `ThreadTimeline` condiviso, filtrare i
  TID del target (Toolhelp `TH32CS_SNAPTHREAD`), e una tab Gantt (Painter egui, come il
  flame). Le fondamenta pure sono già pronte e testate.
- **Fase 3 allocazioni/lock**: provider `HeapTrace`/`Kernel-Memory` per le allocazioni
  (+ allocation flame graph) e analisi `CSwitch` su wait object per la contesa lock.
- **Fase 5**: PMU/GPU — richiede driver/SDK vendor; rivalutare la fattibilità in user mode.
- **Nice-to-have**: compressione zstd del formato `.argus`; export PNG/JSON; file dialog
  nativo (ora auto-path + lista in-app, D19); regex→fuzzy nella ricerca flame.

---

## Decisioni rinviate (esplicite)

Cose deliberatamente **non decise ora** che valuteremo a tempo debito:

- **Nome finale**: "Argus" è provvisorio? Sì, se troviamo qualcosa di meglio prima della Fase 2.
- **Licenza**: MIT default. Se diventa progetto serio si valuta dual MIT/Apache-2.0.
- **Cross-platform**: NO per Fase 1-3, riconsiderare Fase 4+.
- **Plugin system**: NO mai (rompe portabilità e affidabilità).
- **Cloud / sync / telemetry**: NO mai (rompe privacy).
- **Multi-process attach**: rinviata a Fase 4+.
- **Distributed tracing** (multi-machine): fuori scope.

## Note al passaggio di fase

Fra una fase e la successiva:

1. Aggiorna `docs/` se hai imparato cose
2. Tag git: `v0.X.0` per ogni fase completata
3. Aggiorna `README.md` con lo stato corrente
4. Crea milestone GitHub per la fase successiva
