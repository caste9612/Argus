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
- ✅ Search box (substring, non ancora regex), zoom, click-to-drill — `ui/flame.rs`
- ✅ Tab "Flame" nella dashboard
- ⬜ Provider `Thread` per context switch — rinviato (è preparazione Fase 3)

**Stato verifica**: tutto compila, clippy/fmt/test verdi (17 unit + 3 integration), e l'app
gira mostrando il flame graph e il degrado graceful "ETW non disponibile" senza admin
(verificato a video). La **cattura ETW reale richiede admin** e va collaudata con un run
elevato — è l'unica parte non verificabile in un ambiente non elevato.

**DoD**:
- ⏳ Attacchi a un processo CPU-bound, vedi il flame graph popolarsi entro 10 s — *da verificare come admin*
- ✅/⏳ Click su un frame zooma correttamente — logica di layout/focus unit-testata; resa visiva da verificare con dati reali
- ⏳ Symbol resolution funziona per binari con `.pdb` (locale o symbol server) — *da verificare come admin*
- ⏳ Overhead totale sul target < 1 % — *da misurare come admin*
- 🔶 Edge cases ETW di `06-reliability.md`: "ETW fails (permessi) → polling-only + banner" ✅ verificato; gli altri da verificare come admin

**Rifiniture rimaste per chiudere la fase**: ricerca regex (ora substring), risoluzione
simboli del target *on-disk* via eventi Image/Load (ora best-effort su handle vivo, vedi D14),
e la verifica end-to-end come amministratore.

---

## Fase 3 — Allocations + locks + timeline (~3-4 settimane)

**Obiettivo**: visualizzare comportamento dinamico (allocazioni, contesa, thread states).

**Deliverable**:
- Sottoscrizione provider `Thread` (CSwitch già fatto in Fase 2) + `PageFault`
- Heap allocation tracking (provider `HeapTrace` per processi opt-in)
- Thread states timeline (Gantt) renderizzato con wgpu custom
- Lock contention detection (analisi CSwitch su mutex/wait object)
- Allocation flame graph (chi alloca, quanto, dove)
- Pannello "Locks" con waiter analysis

**DoD**:
- Vedi un thread bloccato su lock con indicazione visiva chiara
- Identifichi allocazioni hot path con stack trace
- Cumulative allocations per stack visibile
- Timeline scrub fluido anche con 30 thread × 60 s

---

## Fase 4 — Recording + diff + export (~2 settimane)

**Obiettivo**: trasformare Argus da live-only a strumento di analisi post-mortem.

**Deliverable**:
- Formato file `.argus` proprietario (binario, compresso `zstd`, versioned con magic header)
- Record + replay dell'intera sessione
- Modalità "diff" tra due capture (grafici sovrapposti, delta evidenziato)
- Export selettivo (CSV time series, SVG flame, PNG screenshot, JSON metadata)
- Snapshot manuali (premi un tasto, salva stato corrente)
- Sharing-friendly: link/embed di flame graph statici come HTML

**DoD**:
- Catturi 5 minuti di un'app, salvi, riapri, vedi tutto come live
- Confronti due capture (prima/dopo ottimizzazione) con grafici sovrapposti
- Export SVG di un flame graph apribile in browser

---

## Fase 5 — Hardware counters + GPU profiling (esplorativa)

**Obiettivo**: scendere ai contatori hardware e profiler GPU.

**Deliverable** (provisori, da rivalutare prima di iniziare):
- Wrapper per PMU counters (cache miss, branch miss) — richiede driver o SDK vendor
- GPU profiling via NVML + ETW DXGI events
- Pannello dedicato "Hardware"
- Eventualmente: integrazione con NVIDIA Nsight Aftermath API

**DoD**: TBD — dipende da quanto driver lavoro è realmente fattibile in user mode.

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
