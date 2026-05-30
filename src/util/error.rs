//! Tipo di errore unificato di Argus.
//!
//! Vedi `docs/06-reliability.md`. Regola: i layer bassi (capture, aggregation)
//! ritornano `Result<T, ArgusError>`; i layer alti (ui) lo convertono in stato
//! visualizzato. Nessun errore deve propagare fino al main loop come panic.

use std::fmt;

#[derive(Debug)]
pub enum ArgusError {
    /// Errore proveniente da una API Win32 / ETW.
    Os(windows::core::Error),
    /// Operazione che richiede un processo collegato, ma non c'è.
    #[allow(dead_code)] // usato dai comandi che operano sul target (Fase 1+)
    NotAttached,
    /// Accesso negato dal sistema, con suggerimento per l'utente.
    Permission { hint: String },
    /// Bug interno: loggato, non fatale.
    Internal(String),
}

impl fmt::Display for ArgusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgusError::Os(e) => write!(f, "Errore di sistema: {e}"),
            ArgusError::NotAttached => write!(f, "Nessun processo collegato."),
            ArgusError::Permission { hint } => write!(f, "{hint}"),
            ArgusError::Internal(msg) => write!(f, "Errore interno: {msg}"),
        }
    }
}

impl std::error::Error for ArgusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArgusError::Os(e) => Some(e),
            _ => None,
        }
    }
}

impl From<windows::core::Error> for ArgusError {
    fn from(e: windows::core::Error) -> Self {
        ArgusError::Os(e)
    }
}
