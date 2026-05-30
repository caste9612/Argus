# 10 – Come si usa Argus

Guida pratica all'uso. Per build/sviluppo vedi [`08-development.md`](08-development.md).

## Avvio

```powershell
cargo run --release            # build ottimizzata
# oppure l'eseguibile:           target\release\argus.exe
```

Argus apre una finestra con, a sinistra, la **lista processi** e, a destra, il
pannello con le **tab**: *Metriche*, *Flame graph*, *Timeline*, *Diff*.

### Utente vs amministratore — importante

| Esecuzione | Cosa funziona |
|---|---|
| **Utente** (normale) | Metriche polling (CPU/RAM/I-O/thread/handle), lista processi, record/diff/export. Flame graph e Timeline mostrano un banner "ETW non disponibile". |
| **Amministratore** | Tutto quanto sopra **+ flame graph e timeline** (richiedono ETW kernel, che ha bisogno dei privilegi). |

Per il profiling completo, avvia Argus **come amministratore** (tasto destro →
"Esegui come amministratore"). Senza, le altre funzioni restano comunque pienamente usabili.

## Collegarsi a un processo

1. Cerca il processo nella lista (casella *filtra per nome*). I tuoi processi
   (sessione utente) sono in cima; i servizi di sistema sotto, attenuati.
2. Ordina per **CPU**, **RAM** o **Nome**.
3. **Doppio click** sul processo (o selezionalo e premi **Collega**).
4. Per staccarti: **Scollega** nella barra in alto.

Da riga di comando puoi collegarti subito: `argus.exe --attach <PID>`
(opzionale `--tab flame|timeline|metriche|diff` per la tab iniziale).

## Le tab

- **📊 Metriche** — KPI card + grafici time-series (CPU, working set/private,
  I/O lettura/scrittura, thread, handle). Ogni elemento ha un tooltip che spiega
  cosa significa.
- **🔥 Flame graph** — *dove* il processo spende tempo CPU (richiede admin).
  - **Click** su un frame per zoomare; **zoom out** torna alla radice.
  - **Cerca (regex)**: evidenzia le funzioni che combaciano, attenua le altre.
  - **Hover**: nome, numero di sample e percentuali (totale e self).
- **📶 Timeline** — Gantt: quando ogni thread del target è in esecuzione
  (verde), dai context switch (richiede admin). I thread più attivi in cima.
- **⇄ Diff** — confronto "prima/dopo" tra due sessioni salvate: grafici
  sovrapposti (A grigio = baseline, B blu = corrente) e le funzioni che cambiano
  di più (movers).

## Registrare, riaprire, confrontare, esportare

Nella barra in alto (attivi quando c'è una sessione):

- **💾 Salva** — salva la sessione corrente (metriche + flame) in un file
  `.argus` in `%LOCALAPPDATA%\Argus\captures`.
- **📂 Apri** — riapre una sessione salvata in *replay* (la rivedi come live).
- **⬇ Esporta** — CSV (metriche), folded-stacks (apribile in
  [speedscope](https://www.speedscope.app/) / flamegraph.pl), SVG (flame graph
  statico, apribile nel browser). Vanno in `%LOCALAPPDATA%\Argus\exports`.
- **⇄ Confronta** — scegli una sessione `.argus` come baseline; il risultato
  appare nella tab **Diff**.

Workflow tipico "ottimizzazione": collega → *Salva* (prima) → ottimizza il codice
→ ricollega → *Confronta* con il file salvato → guarda i *movers* nella tab Diff.

## Dove finiscono i file

- Sessioni: `%LOCALAPPDATA%\Argus\captures\*.argus`
- Export: `%LOCALAPPDATA%\Argus\exports\*.{csv,folded.txt,svg}`
- Log: `%LOCALAPPDATA%\Argus\argus.log` (rotazione giornaliera)

## Note

- Se un processo è protetto (antivirus, PPL) o di sistema, l'attach può fallire
  con "accesso negato": serve l'esecuzione come amministratore (e alcuni restano
  comunque inaccessibili).
- I nomi delle funzioni nel flame graph dipendono dai simboli (`.pdb`): i moduli
  di sistema (ntdll, kernel32) si risolvono col nome; per il codice del target
  servono i suoi `.pdb` (al momento, in mancanza, si vede `modulo!0xINDIRIZZO`).
