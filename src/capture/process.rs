//! Enumerazione dei processi e apertura del target.
//!
//! Per la lista usiamo `NtQuerySystemInformation(SystemProcessInformation)`: una
//! sola syscall restituisce per **ogni** processo nome, sessione, memoria, tempi
//! CPU, thread e handle. È l'API che usano Task Manager e Process Explorer —
//! molto più economica che aprire un handle per ciascun processo.

use crate::util::error::ArgusError;
use crate::util::win::HandleGuard;
use core::ffi::c_void;
use std::mem::size_of;
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, HANDLE, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;

/// Riga della lista processi mostrata nel pannello sinistro.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub threads: u32,
    #[allow(dead_code)] // usato dalla process-tree view in Fase 2
    pub parent_pid: u32,
    /// Sessione Terminal Services. 0 = servizi/sistema; >0 = sessione interattiva.
    pub session_id: u32,
    /// True se gira nella stessa sessione di Argus (≈ "avviato dall'utente").
    pub is_user: bool,
    pub working_set_mb: f32,
    /// Tempo CPU cumulativo (kernel+user) in unità da 100 ns. Grezzo: il sampler
    /// ne calcola la percentuale a partire dal delta tra due refresh.
    pub cpu_total_100ns: u64,
    /// % CPU calcolata dal sampler (0 finché non c'è un campione precedente).
    pub cpu_percent: f32,
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
pub fn open_process(pid: u32) -> Result<ProcessHandle, ArgusError> {
    // SAFETY: chiamata diretta; l'handle restituito è incapsulato subito in RAII.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    match handle {
        Ok(h) if !h.is_invalid() => Ok(ProcessHandle(h)),
        Ok(_) => Err(ArgusError::Internal(
            "OpenProcess ha dato un handle nullo".into(),
        )),
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

/// Apre il target con i diritti che servono a DbgHelp per caricare i simboli del
/// processo *vivo* (`PROCESS_QUERY_INFORMATION | PROCESS_VM_READ`).
///
/// È best-effort: usato solo per il flame graph (Fase 2). Se fallisce, il
/// chiamante mostra gli indirizzi grezzi invece dei nomi (graceful degradation).
pub fn open_for_symbols(pid: u32) -> Result<ProcessHandle, ArgusError> {
    // SAFETY: l'handle restituito è incapsulato subito in RAII.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) };
    match handle {
        Ok(h) if !h.is_invalid() => Ok(ProcessHandle(h)),
        Ok(_) => Err(ArgusError::Internal(
            "OpenProcess (symbols) handle nullo".into(),
        )),
        Err(e) => Err(ArgusError::Os(e)),
    }
}

/// Elenca i TID dei thread appartenenti a `pid` via snapshot Toolhelp.
///
/// Serve a filtrare i context-switch (Fase 3) ai thread del target: i `CSwitch`
/// ETW non portano il PID. Best-effort: ritorna vuoto se lo snapshot fallisce.
pub fn thread_ids(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    // SAFETY: snapshot dei thread di sistema; l'handle è chiuso dall'HandleGuard.
    // THREADENTRY32 ha `dwSize` impostato prima di ogni chiamata, come richiesto.
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            Ok(h) if !h.is_invalid() => h,
            _ => return out,
        };
        let _guard = HandleGuard(snap);

        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        if Thread32First(snap, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID == pid {
                    out.push(entry.th32ThreadID);
                }
                entry.dwSize = size_of::<THREADENTRY32>() as u32;
                if Thread32Next(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
    }
    out
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
    Some(full.rsplit('\\').next().unwrap_or(&full).to_string())
}

/// Enumera tutti i processi via `NtQuerySystemInformation`.
pub fn list_processes() -> Result<Vec<ProcessInfo>, ArgusError> {
    let buf = query_process_information()?;
    Ok(parse_process_information(&buf))
}

/// Esegue la query NT in un buffer allineata a 8 byte (i campi della struct sono
/// `i64`/`usize`). Cresce e ritenta finché il buffer è sufficiente.
fn query_process_information() -> Result<Vec<u64>, ArgusError> {
    // Partiamo da 512 KB; la lista cresce/cala tra una chiamata e l'altra.
    let mut buf: Vec<u64> = vec![0; 64 * 1024];
    loop {
        let mut ret_len: u32 = 0;
        // SAFETY: passiamo un buffer valido con la sua lunghezza in byte e un
        // puntatore a ret_len. La classe è quella corretta per la struct attesa.
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessInformation,
                buf.as_mut_ptr() as *mut c_void,
                (buf.len() * 8) as u32,
                &mut ret_len,
            )
        };

        if status == STATUS_INFO_LENGTH_MISMATCH {
            let needed_u64 = (ret_len as usize / 8) + 4096; // slack per nuovi processi
            buf = vec![0; needed_u64.max(buf.len() * 2)];
            continue;
        }
        if status.is_ok() {
            return Ok(buf);
        }
        return Err(ArgusError::Internal(format!(
            "NtQuerySystemInformation ha restituito lo status {:#010x}",
            status.0 as u32
        )));
    }
}

/// Cammina la lista concatenata di `SYSTEM_PROCESS_INFORMATION` nel buffer.
fn parse_process_information(buf: &[u64]) -> Vec<ProcessInfo> {
    let mut out = Vec::with_capacity(384);
    let base = buf.as_ptr() as *const u8;
    let current_pid = std::process::id();
    let mut user_session: Option<u32> = None;
    let mut offset = 0usize;

    loop {
        // SAFETY: `base` è allineato a 8 byte (Vec<u64>) e gli offset forniti dal
        // kernel sono multipli di 8, quindi ogni entry è correttamente allineata
        // e interamente contenuta nel buffer restituito dalla query.
        let p = unsafe { &*(base.add(offset) as *const SYSTEM_PROCESS_INFORMATION) };

        let pid = p.UniqueProcessId.0 as usize as u32;
        let session = p.SessionId;
        // windows-rs non espone KernelTime/UserTime come campi nominati: vivono
        // dentro Reserved1 (layout stabile da Vista — UserTime @32, KernelTime @40).
        let user = i64::from_le_bytes(p.Reserved1[32..40].try_into().unwrap_or([0u8; 8]));
        let kernel = i64::from_le_bytes(p.Reserved1[40..48].try_into().unwrap_or([0u8; 8]));
        let cpu_total = (kernel as u64).wrapping_add(user as u64);

        let name = read_image_name(p, pid);
        if pid == current_pid {
            user_session = Some(session);
        }

        out.push(ProcessInfo {
            pid,
            name,
            threads: p.NumberOfThreads,
            // InheritedFromUniqueProcessId è esposto come Reserved2 in windows-rs.
            parent_pid: p.Reserved2 as usize as u32,
            session_id: session,
            is_user: false, // riempito sotto, quando conosciamo la nostra sessione
            working_set_mb: p.WorkingSetSize as f32 / (1024.0 * 1024.0),
            cpu_total_100ns: cpu_total,
            cpu_percent: 0.0,
        });

        if p.NextEntryOffset == 0 {
            break;
        }
        offset += p.NextEntryOffset as usize;
    }

    let us = user_session.unwrap_or(1);
    for pi in &mut out {
        pi.is_user = pi.session_id == us && pi.pid != 0;
    }
    out
}

/// Estrae il nome immagine dalla UNICODE_STRING, con fallback per i PID speciali.
fn read_image_name(p: &SYSTEM_PROCESS_INFORMATION, pid: u32) -> String {
    let u = &p.ImageName;
    if !u.Buffer.is_null() && u.Length > 0 {
        // SAFETY: Buffer punta a Length byte (Length/2 unità u16) validi per la
        // durata di questo buffer.
        let slice = unsafe { std::slice::from_raw_parts(u.Buffer.0, (u.Length / 2) as usize) };
        String::from_utf16_lossy(slice)
    } else {
        match pid {
            0 => "System Idle Process".to_string(),
            4 => "System".to_string(),
            _ => format!("PID {pid}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_ids_of_self_then_invalid() {
        let tids = thread_ids(std::process::id());
        assert!(!tids.is_empty(), "il processo corrente ha almeno un thread");
        assert!(
            thread_ids(0xFFFF_FFF0).is_empty(),
            "un PID inesistente non ha thread"
        );
    }
}
