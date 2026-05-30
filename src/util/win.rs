//! Wrapper Win32 a basso livello. Tutto l'`unsafe` "di sistema" non legato a un
//! processo specifico vive qui, isolato e commentato.

use windows::Win32::Foundation::{CloseHandle, GetLastError, FILETIME, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::SystemInformation::GetSystemInfo;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// RAII guard per un `HANDLE` generico (es. snapshot Toolhelp). Chiude alla Drop.
pub struct HandleGuard(pub HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: l'handle è valido (controllato) e posseduto da questo guard;
            // nessun altro lo chiude.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Converte un `FILETIME` (unità da 100 ns) in `u64`.
#[inline]
pub fn filetime_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// Numero di processori logici visti dal sistema (min 1).
pub fn logical_cpu_count() -> u32 {
    // SAFETY: GetSystemInfo riempie una struct che azzeriamo prima; nessun puntatore
    // sopravvive alla chiamata.
    unsafe {
        let mut info = std::mem::zeroed();
        GetSystemInfo(&mut info);
        info.dwNumberOfProcessors.max(1)
    }
}

/// Tenta di abilitare `SeDebugPrivilege` per il processo corrente.
///
/// Ha effetto solo se Argus gira come amministratore; altrimenti è un no-op
/// innocuo. Ritorna `true` se il privilegio è stato effettivamente concesso.
pub fn enable_debug_privilege() -> bool {
    // SAFETY: sequenza standard OpenProcessToken → LookupPrivilegeValue →
    // AdjustTokenPrivileges. Il token è chiuso dall'HandleGuard. Tutti i puntatori
    // passati sono a variabili locali vive per la durata delle chiamate.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return false;
        }
        let _guard = HandleGuard(token);

        let name: Vec<u16> = "SeDebugPrivilege\0".encode_utf16().collect();
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(name.as_ptr()),
            &mut luid,
        )
        .is_err()
        {
            return false;
        }

        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        if AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None).is_err() {
            return false;
        }

        // AdjustTokenPrivileges "riesce" anche se non ha potuto concedere il
        // privilegio: ERROR_NOT_ALL_ASSIGNED (1300) lo segnala. 0 = concesso.
        GetLastError().0 == 0
    }
}
