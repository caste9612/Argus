# Argus

**Profiler portatile GPU-accelerato per Windows.**
Vedi cosa fa davvero il tuo software, senza rallentarlo.

> 🚧 **Stato:** in fase di progettazione. Nessun codice ancora — la prima implementazione partirà dalla Fase 1 della [roadmap](docs/07-roadmap.md), guidata dai documenti in `docs/`.

## Cos'è

Argus è uno strumento per **analizzare in dettaglio il comportamento di un processo Windows in esecuzione**: CPU, memoria, I/O, thread, hot path delle funzioni, lock contention, allocazioni — tutto in tempo reale con una dashboard moderna GPU-renderizzata che regge milioni di sample senza scatti.

L'obiettivo è triplo:

- **Portatile** — un singolo `.exe`, niente runtime, niente installer
- **Affidabile** — nessun crash, nessun panic, degradazione graduale se mancano permessi o dati
- **Bello + leggibile** — grafica curata, dati comprensibili a colpo d'occhio

## Per chi è

Sviluppatori (Rust / C++ / .NET / Go / …) che vogliono capire cosa sta facendo un loro programma in produzione o in test, **senza dover invocare `perfview` con 17 flag** o leggere stack trace di 200 righe.

## Quick start

Non ancora implementato. La struttura attuale è solo documentazione: vedi la [roadmap](docs/07-roadmap.md) per il piano di sviluppo per fasi.

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

Sviluppatori (umani e AI) che collaborano su Argus devono leggere **`CLAUDE.md`** in radice per le convenzioni.

## Stack tecnologico

- **Rust** 1.92+ — no GC, no runtime, binary singolo
- **windows-rs** — API Win32 native
- **wgpu** — rendering GPU moderno (DirectX 12 backend su Windows)
- **egui / eframe** — UI immediate-mode
- **ETW** — Event Tracing for Windows per cattura kernel-level (Fase 2+)

Dettagli e alternative scartate: [docs/03-tech-stack.md](docs/03-tech-stack.md).

## Licenza

MIT — vedi [LICENSE](LICENSE).
