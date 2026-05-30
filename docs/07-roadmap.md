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

## Fase 1 — MVP polling (in corso)

**Obiettivo**: attaccarsi a un processo Win64 e visualizzare le metriche polling-based in real-time.

**Stato**: scaffold a layer, attach/detach, sampler 10 Hz lock-free, tutte le 7
metriche, dashboard egui, logging e error handling **fatti e funzionanti**.
Resta da fare: test integration con binario fixture deterministico.

**Deliverable**:
- Setup progetto Rust (`Cargo.toml`, layout moduli da `02-architecture.md`)
- Process list (Toolhelp) + search + filtri
- Attach by PID con error handling completo (vedi `06-reliability.md` tabella edge cases)
- Sampler thread @ 10 Hz, lock-free verso UI
- Tutte le metriche Fase 1 da `04-metrics.md` (CPU, working set, private bytes, I/O R/W, threads, handles)
- Dashboard con KPI cards + 6 line chart (egui_plot inizialmente)
- Detach + handle cleanup pulito
- Build script che produce `.exe` single-file release
- Logging tracing su file rotato
- Test integration con `tests/fixtures/target_app.exe` (binario nostro)

**DoD**:
- Attacchi `notepad.exe` o un binario di test, vedi metriche aggiornarsi in real-time
- Argus sta in < 200 MB RAM, < 5 % CPU @ idle, < 10 % @ active
- Tutti gli edge case Fase 1 di `06-reliability.md` testati e passanti
- `cargo clippy --all-targets -- -D warnings` pulito
- Binario release < 15 MB

---

## Fase 2 — ETW + flame graph (~2-3 settimane)

**Obiettivo**: aggiungere cattura kernel-level e visualizzazione del hot path. Questo è il pezzo che **insegna ottimizzazione**.

**Deliverable**:
- ETW session manager (sottoscrizione provider `PerfInfo`)
- Stack walk capture con `EVENT_TRACE_FLAG_PROFILE`
- Symbol resolution (DbgHelp wrapper RAII) con cache LRU
- Flame graph aggregator (struttura tree con count incrementale)
- Flame graph renderer wgpu **custom** (un quad per frame, color coded, GPU-accelerated)
- Search box (regex), zoom, click-to-drill
- Tab "Flame" nella dashboard
- Provider `Thread` per context switch (preparazione Fase 3)

**DoD**:
- Attacchi a un processo CPU-bound, vedi il flame graph popolarsi entro 10 s
- Click su un frame zooma correttamente
- Symbol resolution funziona per binari con `.pdb` disponibile (locale o symbol server)
- Overhead totale sul target ancora < 1 %
- Edge cases ETW Fase 2 di `06-reliability.md` testati

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
