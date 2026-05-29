# 03 – Stack tecnologico

## Scelte definitive

| Categoria | Scelta | Versione minima |
|---|---|---|
| Linguaggio | Rust | 1.92 (edition 2021) |
| Bindings Windows | `windows` crate | 0.58+ |
| UI immediate-mode | `egui` via `eframe` (backend wgpu) | 0.29+ |
| Grafica GPU | `wgpu` | 22+ (incluso in eframe) |
| Sincronizzazione | `parking_lot`, `arc-swap` | latest |
| Canali | `crossbeam-channel` | latest |
| ETW (Fase 2+) | `ferrisetw` + fallback `windows-rs` raw | latest |
| Symbol resolution | `windows::Win32::System::Diagnostics::Debug` (DbgHelp) | — |
| Logging | `tracing` + `tracing-subscriber` | latest |

## Perché Rust

- **No GC, no runtime** → latency prevedibile, niente pause come .NET. Cruciale per un real-time profiler.
- **Single static binary** → nessuna dipendenza da DLL o framework. Argus è un `.exe` portabile.
- **Memory safety** → in un tool che maneggia handle kernel, indirizzi, e stack frame, gli errori di memoria sono catastrofici. Rust li previene a compile time.
- **Performance a livello C++** → nessun overhead per le astrazioni che usiamo.
- **Ecosistema crescente** → `windows-rs` è ufficiale Microsoft, `wgpu` è battle-tested (Firefox), `egui` è production-grade.

### Costo della scelta

- ETW bindings (`ferrisetw`, `krabsetw`) meno maturi di `TraceEvent` (.NET). Per il ~20% di funzionalità avanzate scriveremo wrapper su `windows-rs` raw. Accettabile.
- Symbol resolution con DbgHelp richiede `unsafe` wrapping. Isolato in un modulo (`capture/symbols.rs`).

## Perché wgpu (e non DirectX 12 diretto)

- API moderna, ergonomica, mentale-simile a Vulkan/Metal
- Cross-API: DX12 su Windows ma il codice è portabile se mai volessimo Linux/Mac
- Battle-tested in Firefox e WebGPU
- Eccezionale per data viz: compute shader semplici, vertex/fragment chiari
- Integrato bene con eframe: rendering UI + custom rendering nello stesso frame via `egui::PaintCallback`

### Costo

Piccolo overhead vs DX12 diretto (~5-10% in scenari estremi). Per i nostri throughput (decine di milioni di vertici/secondo) è invisibile.

## Perché eframe/egui

- Immediate-mode = niente state machine UI da mantenere
- Sviluppo *molto* più veloce di retained-mode (WPF, Qt)
- Tema scuro nativo bello, customizabile
- Multitouch, scrolling, layout responsive gratis
- `egui_plot` per chart basici già pronto (Fase 1)
- Custom drawing per pannelli wgpu nativi: supportato via `Painter::add(PaintCallback::new(...))`

### Costo

- Look-and-feel "egui" riconoscibile — meno custom-brand-able di un'UI bespoke
- Per chart molto custom (3D flame, particle flows) dobbiamo scrivere shader wgpu da zero — non è zero-code

Mitigazione: in `docs/05-ui-design.md` definiamo un tema visivo specifico Argus sopra egui.

## Alternative considerate (e scartate)

### C# + WPF + TraceEvent
**Perché no**: GC pauses, runtime .NET pesante, distribuzione complicata, grafica meno fluida.
*Ottimo se*: volessimo shipping rapido di un tool con TraceEvent maturo. Non i nostri criteri.

### C++ + DirectX 12 + ETW nativo
**Perché no**: 3-5× più codice, memoria fragile, build infernale su Windows. Avremmo speso tempo a combattere il linguaggio.
*Ottimo se*: avessimo bisogno del 5% extra di performance al limite assoluto.

### Tauri + frontend web
**Perché no**: aggiunge layer (Webview), aumenta dimensione binario, performance grafica peggiore per real-time.
*Ottimo se*: volessimo riusare expertise web frontend.

### Bevy (game engine come framework)
**Perché no**: overkill, ECS non aiuta per una dashboard.

### Iced / Slint (UI Rust alternative)
**Perché no**: ecosistema più piccolo di egui, meno componenti pronti, meno esempi profiler-like.

## Dipendenze: regole

Ogni nuova dipendenza deve passare 3 test:

1. **Indispensabile** — non lo possiamo scrivere noi in < 1 giorno?
2. **Manutenuto** — ultimo commit < 12 mesi?
3. **Sano** — preferibilmente < 50 transitive deps

Vietate:
- Dipendenze cosmetiche (date formatting, slug, ecc. → scriviamo noi)
- GUI framework alternativi (egui è la scelta, niente Iced/Slint a metà progetto)
- Async runtime grandi (tokio ok solo se davvero serve)

## Strumenti di sviluppo

- `cargo` (default)
- `cargo-watch` per dev loop (opzionale)
- `cargo-bloat` per analizzare dimensione binario
- `cargo-flamegraph` per profilare Argus stesso (meta!)
- `cargo-clippy` come gate
- `cargo-fmt` come gate
- `cargo-deny` per audit licenze + advisory (Fase 3)

## CI/CD (Fase 3+)

GitHub Actions su `windows-latest`:

- `cargo build --release`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`
- Artifact: `argus.exe` zippato
- Release: tag → release con `.zip` allegato + checksums
