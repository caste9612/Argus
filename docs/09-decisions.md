# 09 – Decisioni e stato

Registro delle decisioni prese durante lo sviluppo (stile ADR, sintetico) e
fotografia del punto in cui siamo. Quando una scelta cambia, si aggiorna qui.

## Stato attuale

**Fase 1 (MVP polling) — completata.** Argus si attacca a un processo Windows e
ne mostra in tempo reale CPU, RAM (working set + private), I/O, thread e handle,
con dashboard GPU e lista processi raggruppata/ordinabile. Build, clippy e test
(2 unit + 3 integration) verdi.

**Fase 2 — ETW + flame graph: in corso.** Prima tappa fatta: la struttura dati
del flame graph (`aggregation/flame.rs`, albero pesato puro e testato, vedi D13).
Prossimo: symbol resolution (DbgHelp) e sessione ETW per gli stack sample.
Vedi [`07-roadmap.md`](07-roadmap.md).

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

### D13 — Flame graph: albero puro keyed-by-nome, layout senza ricorsione
La struttura dati del flame graph (`aggregation/flame.rs`) è **pura**: aggrega
stack di *nomi di frame già risolti*, senza toccare Win32. Questo la disaccoppia
dal layer simboli (che mappa indirizzo→nome) e la rende interamente testabile
senza ETW né privilegi. Scelte:
- **Ordine stack root→leaf** (frame più esterno per primo): è l'ordine naturale
  del disegno; il layer ETW invertirà se serve.
- **Interning dei nomi** (`name_id: u32`) + una sola mappa `(genitore, name) →
  figlio` per tutto l'albero, invece di una HashMap per nodo: meno allocazioni.
- **Figli in ordine di prima comparsa**: stabile tra un update e l'altro, così i
  frame non saltano lateralmente mentre i contatori crescono live. Ordinamento
  per valore/alfabetico è un affinamento UI futuro.
- **Layout con work-stack esplicito** (niente ricorsione): robusto anche per
  stack patologicamente profondi (no-panic policy). Il focus si espande a piena
  larghezza con la catena di antenati per il drill-down/zoom.
**Conseguenza**: il rendering (egui o wgpu) consuma solo `layout(focus) →
Vec<Rect>` + accessor di lettura; nessuna logica di profiling nella UI.

## Questioni aperte

- **Budget RAM**: a riposo Argus usa ~304 MB, sopra il target di 300 MB scritto
  nei docs. Quasi tutto è overhead del driver GPU/wgpu (le strutture dati di
  Argus sono <1 MB). Da decidere: rivedere il budget o misurare separatamente la
  "RAM nostra".
- **Dimensione binario release**: da misurare contro il target <15 MB.
- **Edge case di affidabilità** non ancora testati in modo dedicato: GPU device
  lost, sistema low-memory (vedi tabella in [`06-reliability.md`](06-reliability.md)).
