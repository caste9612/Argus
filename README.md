# Argus

**Profiler portatile GPU-accelerato per Windows.**
Vedi cosa fa davvero il tuo software, senza rallentarlo.

> ✅ **Stato:** Fase 1 (polling) ✓ · Fase 2 (ETW + **flame graph**) ✓ verificata come admin · Fase 4 (**record/diff/export**) ✓ · Fase 3 (**timeline** stati-thread) ✓ live; allocazioni e lock da fare. Profiling completo (flame + timeline) richiede l'esecuzione **come amministratore** (ETW). Come si usa: [guida](docs/10-uso.md); dettagli: [roadmap](docs/07-roadmap.md) e [decisioni](docs/09-decisions.md).

## Cos'è

Argus è uno strumento per **analizzare in dettaglio il comportamento di un processo Windows in esecuzione**: CPU, memoria, I/O, thread, hot path delle funzioni, lock contention, allocazioni — tutto in tempo reale con una dashboard moderna GPU-renderizzata che regge milioni di sample senza scatti.

L'obiettivo è triplo:

- **Portatile** — un singolo `.exe`, niente runtime, niente installer
- **Affidabile** — nessun crash, nessun panic, degradazione graduale se mancano permessi o dati
- **Bello + leggibile** — grafica curata, dati comprensibili a colpo d'occhio

## Per chi è

Sviluppatori (Rust / C++ / .NET / Go / …) che vogliono capire cosa sta facendo un loro programma in produzione o in test, **senza dover invocare `perfview` con 17 flag** o leggere stack trace di 200 righe.

## Quick start

```powershell
cargo run --release
```

Si apre la dashboard: seleziona un processo nella lista a sinistra e fai doppio
click (o «Collega») per vedere le sue metriche in tempo reale. Per i processi di
sistema o con privilegi elevati, lancia Argus come amministratore.

## Documentazione

Tutta la progettazione vive in [`docs/`](docs/) — leggili in ordine se vuoi capire il progetto:

1. [Visione](docs/01-vision.md) — il problema, gli obiettivi, i principi guida
2. [Architettura](docs/02-architecture.md) — com'è strutturato il sistema
3. [Tech stack](docs/03-tech-stack.md) — le scelte tecnologiche e perché
4. [Metriche](docs/04-metrics.md) — cosa misuriamo, da dove, cosa insegna
5. [UI design](docs/05-ui-design.md) — linguaggio visivo
6. [Affidabilità](docs/06-reliability.md) — garanzie e strategie
7. [Roadmap](docs/07-roadmap.md) — fasi e milestone
8. [Sviluppo](docs/08-development.md) — workflow di lavoro
9. [Decisioni](docs/09-decisions.md) — log delle scelte e stato attuale
10. [Uso](docs/10-uso.md) — **come si usa Argus** (guida pratica)
11. [Backlog](docs/11-backlog.md) — feature/implementazioni mancanti, prioritizzate

Sviluppatori (umani e AI) che collaborano su Argus devono leggere **`CLAUDE.md`** in radice per le convenzioni.

## Stack tecnologico

- **Rust** 1.92+ — no GC, no runtime, binary singolo
- **windows-rs** — API Win32 native
- **wgpu** — rendering GPU moderno (backend auto-selezionato: Vulkan/DX12 su Windows)
- **egui / eframe** — UI immediate-mode
- **ETW** — Event Tracing for Windows per cattura kernel-level (Fase 2+)

Dettagli e alternative scartate: [docs/03-tech-stack.md](docs/03-tech-stack.md).

## Licenza

MIT — vedi [LICENSE](LICENSE).
