//! Enumerazione dei processi e apertura del target.

use crate::util::error::ArgusError;
use crate::util::win::HandleGuard;
use std::mem::size_of;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// Riga della lista processi mostrata nel pannello sinistro.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub threads: u32,
    #[allow(dead_code)] // usato dalla process-tree view in Fase 2
    pub parent_pid: u32,
}

/// Handle posseduto di un processo target. Chiuso alla Drop.
pub struct ProcessHandle(HANDLE);

impl ProcessHandle {
    #[inline]
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: handle valido posseduto in esclusiva da questo wrapper.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: un HANDLE Windows è un intero opaco. Argus lo usa da un solo thread
// alla volta (il sampler), mai condiviso in scrittura concorrente.
unsafe impl Send for ProcessHandle {}
unsafe impl Sync for ProcessHandle {}

/// Apre il target con il diritto minimo che consente le query di Fase 1.
///
/// `PROCESS_QUERY_LIMITED_INFORMATION` (Win10+) basta per GetProcessTimes /
/// MemoryInfo / IoCounters / HandleCount, ed è molto meno soggetto ad
/// "accesso negato" rispetto a includere `PROCESS_VM_READ`.
pub fn open_process(pid: u32) -> Result<ProcessHandle, ArgusError> {
    // SAFETY: chiamata diretta; l'handle restituito è incapsulato subito in RAII.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    match handle {
        Ok(h) if !h.is_invalid() => Ok(ProcessHandle(h)),
        Ok(_) => Err(ArgusError::Internal("OpenProcess ha dato un handle nullo".into())),
        Err(e) if e.code().0 as u32 == ERROR_ACCESS_DENIED.to_hresult().0 as u32 => {
            Err(ArgusError::Permission {
                hint: format!(
                    "Accesso negato al PID {pid}. Per processi di sistema o con \
                     privilegi elevati, rilancia Argus come amministratore. Alcuni \
                     processi protetti (antivirus, PPL) restano inaccessibili."
                ),
            })
        }
        Err(e) => Err(ArgusError::Os(e)),
    }
}

/// Nome del processo dall'immagine, come fallback se non è nella lista.
pub fn image_name(handle: &ProcessHandle) -> Option<String> {
    let mut buf = [0u16; 1024];
    // SAFETY: buffer locale di dimensione nota passato con la sua lunghezza.
    let len = unsafe { GetProcessImageFileNameW(handle.raw(), &mut buf) };
    if len == 0 {
        return None;
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    // Il path è in forma NT (\Device\HarddiskVolumeX\...): prendiamo il basename.
    Some(full.rsplit('\\').next().unwrap_or(&full).to_string())
}

/// Enumera tutti i processi via snapshot Toolhelp, ordinati per nome.
pub fn list_processes() -> Result<Vec<ProcessInfo>, ArgusError> {
    let mut out = Vec::with_capacity(384);

    // SAFETY: lo snapshot è incapsulato in HandleGuard (chiuso alla Drop). Le
    // iterazioni usano una struct con `dwSize` inizializzato come richiede l'API.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let _guard = HandleGuard(snapshot);

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..end]),
                    threads: entry.cntThreads,
                    parent_pid: entry.th32ParentProcessID,
                });
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}
