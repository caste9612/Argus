//! Symbol resolution via DbgHelp: indirizzo → `"modulo!funzione+0xNN"`.
//!
//! DbgHelp **non è thread-safe**: tutte le chiamate per un dato handle devono
//! essere serializzate. Il resolver è perciò pensato per essere posseduto da un
//! solo thread (l'aggregatore in Fase 2). Se la risoluzione fallisce (PDB
//! mancante, modulo non caricato, …) si ripiega sull'indirizzo grezzo: mai un
//! panic, mai un nome inventato (docs/06-reliability.md, graceful degradation).
//!
//! Per i simboli del processo *target* la strategia definitiva (Fase 2, sessione
//! ETW) sarà caricare i moduli da disco via gli eventi Image/Load — qui il
//! resolver è già pronto a operare su un handle di processo (usato anche dai
//! test, che risolvono i simboli di sé stessi).

use crate::util::error::ArgusError;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    SymCleanup, SymFromAddrW, SymGetModuleInfoW64, SymInitializeW, SymSetOptions,
    IMAGEHLP_MODULEW64, SYMBOL_INFOW, SYMOPT_DEFERRED_LOADS, SYMOPT_FAIL_CRITICAL_ERRORS,
    SYMOPT_LOAD_LINES, SYMOPT_UNDNAME,
};

/// Massimo numero di caratteri per un nome di simbolo (come `dbghelp.h`).
const MAX_SYM_NAME: usize = 2000;

/// Capacità per generazione della cache (≈ working set di indirizzi caldi).
const CACHE_CAP: usize = 16_384;

/// Buffer (in `u64`, allineato a 8) per `SYMBOL_INFOW` + il nome a lunghezza
/// variabile che lo segue. `SizeOfStruct` resta `size_of::<SYMBOL_INFOW>()`; lo
/// spazio extra oltre la struct ospita il nome (`MaxNameLen` caratteri).
const SYM_BUF_U64: usize = (size_of::<SYMBOL_INFOW>() + MAX_SYM_NAME * 2) / 8 + 2;

/// Cache a due generazioni: memoria limitata a ~`2 × CACHE_CAP`, costo O(1).
/// Gli indirizzi ripescati vengono promossi nella generazione "calda" e
/// sopravvivono alla rotazione — approssima un LRU senza liste intrusive.
struct Cache {
    hot: HashMap<u64, Arc<str>>,
    cold: HashMap<u64, Arc<str>>,
}

impl Cache {
    fn new() -> Self {
        Self {
            hot: HashMap::with_capacity(CACHE_CAP),
            cold: HashMap::new(),
        }
    }

    fn get(&mut self, key: u64) -> Option<Arc<str>> {
        if let Some(v) = self.hot.get(&key) {
            return Some(v.clone());
        }
        // Hit nella generazione fredda: promuovi così sopravvive alla prossima
        // rotazione.
        if let Some(v) = self.cold.remove(&key) {
            self.hot.insert(key, v.clone());
            return Some(v);
        }
        None
    }

    fn put(&mut self, key: u64, value: Arc<str>) {
        if self.hot.len() >= CACHE_CAP {
            self.cold = std::mem::take(&mut self.hot);
            self.hot = HashMap::with_capacity(CACHE_CAP);
        }
        self.hot.insert(key, value);
    }
}

/// Resolver di simboli legato a un handle di processo. `SymCleanup` alla Drop.
pub struct SymbolResolver {
    handle: HANDLE,
    cache: Cache,
}

// SAFETY: l'handle è un intero opaco e il resolver è usato da un solo thread
// alla volta (DbgHelp è serializzato da questo possesso esclusivo).
unsafe impl Send for SymbolResolver {}

impl SymbolResolver {
    /// Inizializza DbgHelp per `handle`. Con `invade = true` enumera subito i
    /// moduli già caricati nel processo (utile per risolvere sé stessi o un
    /// target vivo); con `false` i moduli vanno registrati a mano.
    ///
    /// L'handle deve restare valido per tutta la vita del resolver.
    pub fn for_process(handle: HANDLE, invade: bool) -> Result<Self, ArgusError> {
        // SAFETY: opzioni globali di DbgHelp + init per l'handle fornito. In caso
        // di errore non costruiamo il resolver, quindi nessun SymCleanup pendente.
        unsafe {
            SymSetOptions(
                SYMOPT_UNDNAME
                    | SYMOPT_DEFERRED_LOADS
                    | SYMOPT_LOAD_LINES
                    | SYMOPT_FAIL_CRITICAL_ERRORS,
            );
            SymInitializeW(handle, PCWSTR::null(), BOOL::from(invade))?;
        }
        Ok(Self {
            handle,
            cache: Cache::new(),
        })
    }

    /// Risolve un indirizzo in un nome leggibile, con caching. Non fallisce mai:
    /// se DbgHelp non sa risolvere, ritorna l'indirizzo formattato.
    pub fn resolve(&mut self, addr: u64) -> Arc<str> {
        if let Some(v) = self.cache.get(addr) {
            return v;
        }
        let name: Arc<str> = Arc::from(self.resolve_uncached(addr));
        self.cache.put(addr, name.clone());
        name
    }

    fn resolve_uncached(&self, addr: u64) -> String {
        let func = self.symbol_at(addr);
        let module = self.module_at(addr);
        match (module, func) {
            (Some(m), Some((f, 0))) => format!("{m}!{f}"),
            (Some(m), Some((f, d))) => format!("{m}!{f}+0x{d:x}"),
            (None, Some((f, 0))) => f,
            (None, Some((f, d))) => format!("{f}+0x{d:x}"),
            (Some(m), None) => format!("{m}!0x{addr:x}"),
            (None, None) => format!("0x{addr:016x}"),
        }
    }

    /// Nome di funzione + displacement dall'inizio del simbolo, se risolvibile.
    fn symbol_at(&self, addr: u64) -> Option<(String, u64)> {
        let mut buf = [0u64; SYM_BUF_U64];
        // SAFETY: `buf` è allineato a 8 (Vec<u64>) e abbastanza grande per
        // SYMBOL_INFOW + MaxNameLen caratteri. Impostiamo i campi dimensionali
        // come richiede DbgHelp prima della chiamata; leggiamo il nome solo per
        // NameLen ≤ MaxNameLen caratteri, tutti dentro il buffer.
        unsafe {
            let info = buf.as_mut_ptr() as *mut SYMBOL_INFOW;
            (*info).SizeOfStruct = size_of::<SYMBOL_INFOW>() as u32;
            (*info).MaxNameLen = MAX_SYM_NAME as u32;
            let mut disp: u64 = 0;
            SymFromAddrW(self.handle, addr, Some(&mut disp), info).ok()?;
            let len = ((*info).NameLen as usize).min(MAX_SYM_NAME);
            let name_ptr = std::ptr::addr_of!((*info).Name) as *const u16;
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(name_ptr, len));
            if name.is_empty() {
                None
            } else {
                Some((name, disp))
            }
        }
    }

    /// Nome del modulo che contiene l'indirizzo (es. `ntdll.dll`). Disponibile
    /// anche senza PDB: viene dall'elenco moduli, non dai simboli.
    fn module_at(&self, addr: u64) -> Option<String> {
        let mut m = IMAGEHLP_MODULEW64 {
            SizeOfStruct: size_of::<IMAGEHLP_MODULEW64>() as u32,
            ..Default::default()
        };
        // SAFETY: `m` è una struct locale con SizeOfStruct impostato; DbgHelp vi
        // scrive i campi del modulo. ModuleName è un array fisso null-terminato.
        unsafe {
            SymGetModuleInfoW64(self.handle, addr, &mut m).ok()?;
        }
        let len = m
            .ModuleName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(m.ModuleName.len());
        if len == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&m.ModuleName[..len]))
        }
    }
}

impl Drop for SymbolResolver {
    fn drop(&mut self) {
        // SAFETY: rilascia lo stato DbgHelp associato all'handle.
        unsafe {
            let _ = SymCleanup(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcess;

    // Funzione bersaglio con nome noto: ne risolviamo l'indirizzo.
    #[inline(never)]
    fn a_named_target_function() -> u64 {
        // black_box evita che l'ottimizzatore la elimini o la fonda.
        std::hint::black_box(0xA11A_u64)
    }

    /// Un solo test in questo file: DbgHelp ha stato globale per-handle e
    /// `GetCurrentProcess()` è un pseudo-handle condiviso — più test paralleli
    /// confliggerebbero. Copre init, modulo, cache e fallback.
    #[test]
    fn resolves_self_and_falls_back_gracefully() {
        // SAFETY: pseudo-handle del processo corrente, sempre valido.
        let h = unsafe { GetCurrentProcess() };
        let mut r = SymbolResolver::for_process(h, true).expect("SymInitialize del self");

        // L'indirizzo di una nostra funzione deve almeno risolvere il modulo
        // (l'elenco moduli non richiede PDB): il binario di test si chiama
        // "argus-<hash>.exe", quindi il nome contiene "argus".
        // Passiamo per un fn pointer prima del cast a intero (un cast diretto
        // del function item sarebbe rifiutato da clippy::fn_to_numeric_cast).
        let fp: fn() -> u64 = a_named_target_function;
        let addr = fp as usize as u64;
        let name = r.resolve(addr);
        assert!(!name.is_empty(), "nome risolto non vuoto");
        assert!(
            name.to_lowercase().contains("argus"),
            "atteso il modulo 'argus' nel nome risolto, visto: {name}"
        );

        // Cache: una seconda risoluzione dà lo stesso contenuto.
        let name2 = r.resolve(addr);
        assert_eq!(&*name, &*name2, "la cache deve restituire lo stesso nome");

        // Indirizzo palesemente invalido: nessun panic, fallback formattato.
        let bad = r.resolve(0x1);
        assert!(!bad.is_empty());
        assert!(
            bad.contains("0x") || bad.to_lowercase().contains("argus"),
            "fallback atteso per indirizzo invalido, visto: {bad}"
        );
    }
}
