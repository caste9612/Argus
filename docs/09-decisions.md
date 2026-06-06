# 09 – Decisioni e stato

Registro delle decisioni prese durante lo sviluppo (stile ADR, sintetico) e
fotografia del punto in cui siamo. Quando una scelta cambia, si aggiorna qui.

## Stato attuale

**Fase 1 (MVP polling) — completata.** Argus si attacca a un processo Windows e
ne mostra in tempo reale CPU, RAM (working set + private), I/O, thread e handle,
con dashboard GPU e lista processi raggruppata/ordinabile. Build, clippy e test
(2 unit + 3 integration) verdi.

**Tag `v0.1.0`** sul completamento Fase 1. Igiene pre-Fase-2 completata: lint
no-panic cablati nel gate, binario release misurato (10.78 MB), decisioni aperte
chiuse (RAM, ring buffer, nome) e nuove D13/D14 registrate.

**Prossimo**: Fase 2 — ETW + flame graph (il pezzo che mostra *dove* il codice
spende tempo). Vedi [`07-roadmap.md`](07-roadmap.md).

## Decisioni

### D1 — Linguaggio e stack: Rust + windows-rs + wgpu + egui
Niente GC/runtime (latenza prevedibile), binario singolo portabile, memory
safety su codice pieno di handle e puntatori. Dettagli e alternative scartate in
[`03-tech-stack.md`](03-tech-stack.md). **Conseguenza**: per alcune aree (ETW)
i binding sono meno maturi di .NET → scriveremo wrapper su misura in Fase 2.

### D2 — Nome: Argus
Il gigante dai 100 occhi della mitologia: osservatore totale. Binario `argus.exe`.

### D3 — Architettura a layer, ma 2 thread in Fase 1
Il target è 3 thread (sampler → aggregator → UI). In Fase 1 il polling produce
già valori scalari, quindi **sampler e aggregator sono un solo thread**;
l'aggregator dedicato arriverà con gli eventi ETW ad alta frequenza in Fase 2.
**Conseguenza**: meno complessità ora, senza precludere l'evoluzione.

### D4 — Pipeline lock-free: `arc-swap` + `crossbeam-channel`
Il sampler pubblica `Snapshot` immutabili via `ArcSwap`; la UI li legge senza
mai bloccarsi. I comandi UI→sampler (attach/detach/refresh) viaggiano su canale.
**Conseguenza**: la UI non aspetta mai il campionamento; clone dello snapshot
(~17 KB a 10 Hz) trascurabile.

### D5 — Conteggio thread a 1 Hz (carry-forward)
Le metriche O(1) (CPU, RAM, I/O, handle) sono campionate a 10 Hz; il numero di
thread arriva dall'enumerazione processi a 1 Hz e viene "tenuto" tra un refresh
e l'altro. **Conseguenza**: nessuna enumerazione costosa ad ogni tick — è
l'ottimizzazione che Argus stesso vuole insegnare.

### D6 — Backend wgpu in auto-selezione (non DX12 forzato)
Forzare DX12-only ha fatto **fallire la creazione della surface** sulla GPU di
test (RTX 5070 Ti, molto recente); Vulkan funzionava. Lasciamo che wgpu scelga
il backend disponibile (PRIMARY: Vulkan/DX12/…). **Conseguenza**: massima
portabilità; i warning Vulkan innocui sono silenziati nel log. Lezione: seguire
l'evidenza dell'hardware, non l'assunzione.

### D7 — Enumerazione processi via `NtQuerySystemInformation`
Per ordinare per uso di risorse serve CPU+RAM per **ogni** processo. Aprire 300
handle al secondo sarebbe spreco; `NtQuerySystemInformation(SystemProcessInformation)`
restituisce nome, sessione, RAM, tempi CPU, thread e handle di tutti i processi
in **una sola syscall** (come Task Manager / Process Explorer). **Conseguenza**:
overhead minimo; dipendenza da una API semi-documentata ma stabilissima.

### D8 — "Processi utente prima" via session ID
I processi della **sessione interattiva** di Argus (≈ avviati dall'utente) sono
raggruppati in cima; i servizi (sessione 0) sotto, attenuati. Ordine secondario:
CPU% / RAM / Nome, scelto in UI. **Conseguenza**: i tuoi programmi sono subito
in evidenza. Una distinzione più precisa per-utente (SID del token) è un
possibile affinamento futuro.

### D9 — Tempi CPU estratti dai byte `Reserved1`
windows-rs **non espone** `KernelTime`/`UserTime` come campi nominati di
`SYSTEM_PROCESS_INFORMATION`: vivono dentro `Reserved1` (layout stabile da
Vista — UserTime @offset 32, KernelTime @40). Li leggiamo con `from_le_bytes`.
**Conseguenza**: piccolo "hack" necessario, documentato e coperto da un test
d'integrazione che lo valida contro un carico reale.

### D10 — Attach con solo `PROCESS_QUERY_LIMITED_INFORMATION`
Niente `PROCESS_VM_READ`: il diritto limitato basta per tutte le query di Fase 1
ed è molto meno soggetto ad "accesso negato". **Conseguenza**: si attaccano molti
più processi utente senza privilegi elevati. `SeDebugPrivilege` viene tentato
all'avvio (utile solo se Argus gira come amministratore).

### D11 — Logging `tracing` filtrato per target
Default `warn,argus=info,wgpu_hal=error`: i log verbosi di wgpu non inondano il
file (`%LOCALAPPDATA%\Argus\argus.log`, rotazione giornaliera). Sovrascrivibile
via `RUST_LOG`. **Conseguenza**: log leggibile e utile per il debug.

### D12 — Struttura lib + bin per la testabilità
La logica vive in `lib.rs` (moduli pubblici); `main.rs` è un wrapper sottile.
Così i test d'integrazione in `tests/` usano `argus::...` e un binario
`src/bin/fixture.rs` con carico deterministico (brucia CPU, alloca, spawna
thread) fa da bersaglio reale. **Conseguenza**: validazione end-to-end del path
Win32 senza mock.

### D13 — Ring buffer sampler→aggregator: `crossbeam-channel::bounded`
La pipeline ETW ad alta frequenza di Fase 2 userà un canale **bounded** di
`crossbeam-channel` (già dipendenza) come SPSC sampler→aggregator. Niente `rtrb`
o altre crate finché un profiling non mostri che l'overhead del canale pesa nella
hot path. **Conseguenza**: nessuna nuova dipendenza; un buffer pieno è di per sé
il segnale che l'aggregator è in ritardo (back-pressure naturale).

### D14 — Lo spike ETW parte da `windows-rs` raw, non da `ferrisetw`
Lo stack (`03`) prevede `ferrisetw` + fallback raw. Per lo **spike** di apertura
Fase 2 partiamo invece da `windows-rs` raw (già dipendenza): valida il path più
difficile (provider kernel `PerfInfo` + stack walk) con pieno controllo e **zero
nuove dipendenze**, e mappa esattamente dove i binding mancano. La scelta di
produzione `ferrisetw`-vs-raw si prende **dopo** lo spike, informata da ciò che
impariamo. **Conseguenza**: non aggiungiamo `ferrisetw` finché non è provato
necessario (regola "no dipendenze comode").

## Questioni aperte

- **Budget RAM** — *risolto*. Si distinguono due grandezze: (a) le **allocazioni
  proprie** di Argus (snapshot, storie, lista processi) → <1 MB oggi, budget < 50
  MB in Fase 1 e < 150 MB in Fase 2-3 (stack samples ~19 MB + cache simboli); (b)
  l'**RSS totale** del processo (~304 MB) → quasi interamente working set del
  driver GPU/wgpu, non controllabile e in linea con qualunque app wgpu. Niente cap
  rigido sull'RSS; il test di stabilità 8h verifica solo che **non cresca** (no
  leak). CLAUDE.md, `02` e `07` sono allineati a questa distinzione.
- **Dimensione binario release** — *risolto*: `argus.exe` = **10.78 MB** (release,
  LTO thin + strip), sotto il target <15 MB.
- **Gate `cargo fmt --check` mai applicato** — *nuovo*. Il codice committato non
  passa `cargo fmt --check` (rustfmt 1.8, default `max_width` 100; nessun
  `rustfmt.toml` nel repo): il gate è documentato in CLAUDE.md ma di fatto non è
  mai stato rispettato. Decisione in sospeso con l'utente — (1) `cargo fmt` una
  tantum su tutto il repo, (2) `rustfmt.toml` su misura, o (3) rilassare il gate.
  Non toccato in autonomia perché riformatterebbe parecchio codice scritto a mano.
- **Edge case di affidabilità** non ancora testati in modo dedicato: GPU device
  lost, sistema low-memory (vedi tabella in [`06-reliability.md`](06-reliability.md)).
