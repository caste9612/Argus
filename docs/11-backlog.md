# 11 – Backlog / feature e implementazioni mancanti

Analisi dei gap a valle del lavoro su Fasi 1–4 + timeline. Confronto tra lo stato
del codice e gli obiettivi/standard dei doc (`01-vision`, `05-ui-design`,
`06-reliability`, `04-metrics`, `07-roadmap`). Prioritizzato per impatto/sforzo.

## Stato (aggiornato)

**✅ Fatto e verificato in questa sessione:**
- P1 — simboli on-disk del target + flame per-funzione; rimozione emoji.
- P2 — **lock contention** (KWAIT_REASON → categorie + breakdown UI + tooltip);
  **timeline ricca** (Ready/Waiting); **Disk I/O detail** (provider DiskIo,
  parser+aggregazione+UI, layout validato live); **memoria** (provider PageFault:
  hard page fault + VirtualAlloc/Free del target, validato live).
- P2 — **latenza disco p50/p99/max in ms** (istogramma + calibrazione QPC).
- P3 — **tema/palette** Argus (`ui/theme.rs`) + numeri monospace.
- P4 — **overhead misurato** (~2.0–2.2% caso peggiore, `tests/overhead.rs`).
- P5 — **CI** GitHub Actions + **cargo-deny** + export **JSON** completo (wait/mem/disk)
  + formato **`.argus` v2** (persiste disco/memoria/lock, retro-compatibile v1).

**⬜ Rinviato / fuori scope (con motivazione):**
- P2 — **heap allocations a livello `HeapAlloc`**: **fuori scope by design** (D25) —
  richiede l'opt-in del target al lancio (IFEO/relaunch), incompatibile con l'attach
  a un processo già avviato senza injection. L'alternativa compatibile (VirtualAlloc/
  Free, granularità di pagina) **è implementata**.
- P2 — Disk I/O: **nome file** per operazione (correlazione FileObject→nome). I
  percentili di latenza in ms sono **fatti**. **Allocation flame graph**: stack-walk
  sugli eventi VirtualAlloc (aggiunta mirata, serve correlazione per timestamp).
- **Replay a video delle metriche profonde**: i dati (disco/memoria/lock) sono già
  nel `.argus` v2 e nel JSON, ma la UI di replay mostra ancora solo metriche+flame.
  Prossimo passo: ripopolare `shared.disk`/`shared.mem` in `open_capture` e far
  leggere alla Dashboard quei dati anche in stato `Replay`.
- P3 — animazioni, flame color per tipo, process tree, hover line-chart: minori.
- P4 — stabilità 8h (durata), GPU device lost / low-memory / target 32-bit.
- P5 — **zstd** (.argus minuscoli, libreria C → supply-chain; D18/D24) ed export
  **PNG** (ridondante con SVG): non fatti per disciplina dipendenze.
- P6 — PMU/GPU/multi-process: esplorativi, richiedono driver/SDK vendor.

## P1 — Qualità/correttezza che mina il valore centrale

- **[grande] Simboli del target → nomi di funzione** (D14). Oggi i frame del
  target nel flame sono `modulo!0xINDIRIZZO` (i moduli di sistema invece hanno il
  nome). Senza, il criterio "trovare la causa CPU in <5 min" (`01-vision`) non è
  davvero soddisfatto. Approccio: caricare i moduli *on-disk* (`SymLoadModuleExW`)
  dai path/base degli eventi ETW Image/Load, invece dell'handle vivo.
- **[medio] Flame: aggregare per funzione, non per IP.** Il resolver include il
  displacement (`func+0xNN`): IP diversi nella stessa funzione creano nodi diversi
  → flame frammentato (235 nodi per un loop banale). Per il flame, keyare per
  *funzione* (senza `+0xNN`); tenere il displacement solo per la foglia/tooltip.
  (Va con il punto sopra.)
- **[piccolo] Niente emoji nell'UI.** `05-ui-design` lo vieta esplicitamente
  ("mai professionale"), ma le tab/bottoni usano 📊🔥📶⇄💾📂⬇. Sostituire con
  testo o glyph non-emoji.

## P2 — Deliverable previsti ma non fatti (roadmap/metriche)

- **[medio] Lock contention (Fase 3).** I `CSwitch` già parsati portano
  `old_state`/`old_wait_reason` (in `cswitch.rs`) ma sono inutilizzati: usarli per
  evidenziare i thread bloccati su wait/lock; pannello "Locks" con waiter analysis.
- **[grande] Heap allocations (Fase 3).** Provider `HeapTrace`/
  `Microsoft-Windows-Kernel-Memory` → allocation flame graph + leak detection.
- **[medio] Disk I/O dettagliato (Fase 2 in `04-metrics`/`05-ui-design`).**
  Provider `DiskIo` → top file per I/O, latenza p50/p99; tab "I/O detail".
- **[medio] Page faults (Fase 3).** Provider `PageFault` → rate + dove (stack).
- **[medio] Timeline più ricca.** Ora mostra solo Running (verde); distinguere
  Ready/Waiting (dati in `old_state`), asse temporale, zoom/scrub orizzontale,
  tooltip con durata/causa-wait (`05-ui-design` lo specifica).

## P3 — UI/UX: standard di `05-ui-design` non ancora applicati

- **[medio] Tema/palette Argus.** I doc fissano hex precisi (`#0E0E12`,
  `#7B61FF`, …) e font (Inter, JetBrains Mono per i numeri). Oggi: visuals dark di
  default egui + colori ad-hoc. Applicare il tema + i font come asset.
- **[medio] Accessibilità.** Navigazione da tastiera (Tab/frecce/Enter/Esc),
  contrasto AA, scala font a DPI alto.
- **[piccolo] Animazioni.** Transizioni tab 200 ms, interpolazione visiva tra
  sample nei grafici (no salti).
- **[piccolo] Flame: colore per tipo** (user/kernel/idle) come da `05-ui-design`.
- **[piccolo] Process tree.** `parent_pid` è raccolto ma inutilizzato
  (`#[allow(dead_code)]`): vista ad albero dei processi.
- **[piccolo] Hover line-chart** con valore + timestamp esatto; "Loading symbols"
  con progress.

## P4 — Affidabilità (edge case di `06-reliability` non coperti)

- **[medio] Overhead < 1% sul target: misurarlo.** È il claim di testa
  (`01-vision`) ma non è mai stato misurato (benchmark con/senza Argus).
- **[medio] Stabilità long-running (8h).** `06-reliability` lo richiede: nessuna
  crescita RAM/handle, nessun degrado UI. Mai testato.
- **[medio] ETW session lost** (driver crash): re-init, dopo 3 fail banner.
- **[piccolo] GPU device lost** (RDP/driver): re-init wgpu + log.
- **[piccolo] Low memory**: banner, stop nuove allocazioni.
- **[piccolo] Target 32-bit**: warning sullo stack-walk (ora `pointer_size`=8 fisso).

## P5 — Infrastruttura e formati

- **[piccolo] CI** (GitHub Actions windows-latest: build/test/clippy/fmt) —
  previsto in `03-tech-stack` (Fase 3+). Nota: `etw_live` è `#[ignore]` (serve admin).
- **[piccolo] `.argus`: compressione zstd** (header già pronto, D18) e **export
  PNG/JSON** (parti di Fase 4 non fatte). La timeline non è ancora nel formato.
- **[piccolo] `cargo-deny`** (audit licenze/advisory) — `03-tech-stack`.

## P6 — Esplorativo (Fase 5, incerto)

- Contatori hardware (PMU: cache miss/branch miss) — richiede driver o SDK vendor.
- GPU profiling (NVML / DXGI) e power (`CallNtPowerInformation`).
- Multi-process attach (oggi singolo processo; rinviato).

## Ordine consigliato

1. **Simboli on-disk + flame per-funzione** (P1) — sblocca il valore reale del flame.
2. **Togliere le emoji** (P1) — rispetto dello standard, costo minimo.
3. **Lock contention + timeline ricca** (P2) — chiude la Fase 3 col valore "didattico".
4. **Overhead + stabilità 8h** (P4) — verifica i claim non negoziabili.
5. **Tema/font + accessibilità** (P3) — la "leggibilità" promessa.
6. **Heap allocations, Disk I/O detail** (P2) — completano le metriche profonde.
7. **CI, zstd, export PNG/JSON** (P5) — robustezza di progetto.
