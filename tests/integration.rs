//! Test d'integrazione: avviano un binario `fixture` con comportamento noto e
//! verificano che il layer di capture di Argus lo misuri correttamente.
//!
//! Chiude la Fase 1 della roadmap: validazione end-to-end del path Win32 reale
//! (enumerazione, nomi, memoria, tempi CPU, thread, sessione) contro un carico
//! deterministico — senza mock.

use argus::capture::process::{list_processes, open_process, ProcessInfo};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

fn find(list: &[ProcessInfo], pid: u32) -> Option<&ProcessInfo> {
    list.iter().find(|p| p.pid == pid)
}

#[test]
fn enumerates_and_measures_a_known_workload() {
    // 4 thread che bruciano CPU, 100 MB allocati, per 6 secondi.
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture"))
        .args(["4", "100", "6000"])
        .spawn()
        .expect("avvio del fixture");
    let pid = child.id();

    sleep(Duration::from_millis(600)); // warmup: thread avviati, memoria toccata

    let list1 = list_processes().expect("prima enumerazione");
    let p1 = find(&list1, pid).expect("il fixture deve comparire nella lista");
    assert!(p1.is_user, "il fixture gira nella sessione utente");
    assert!(p1.threads >= 4, "almeno 4 thread, visti {}", p1.threads);
    assert!(
        p1.working_set_mb >= 50.0,
        "working set >= 50 MB dopo 100 MB allocati, visto {:.1}",
        p1.working_set_mb
    );
    assert!(!p1.name.is_empty(), "il nome non deve essere vuoto");
    let cpu1 = p1.cpu_total_100ns;

    sleep(Duration::from_millis(800));

    let list2 = list_processes().expect("seconda enumerazione");
    let p2 = find(&list2, pid).expect("il fixture è ancora attivo");
    let cpu2 = p2.cpu_total_100ns;

    // Bruciando CPU su 4 thread, il tempo CPU cumulativo deve crescere nettamente
    // (questo valida l'estrazione di KernelTime/UserTime dai byte Reserved1).
    assert!(cpu2 > cpu1, "il tempo CPU deve avanzare ({cpu1} -> {cpu2})");
    assert!(
        cpu2 - cpu1 > 1_000_000, // > 100 ms di CPU (unità da 100 ns)
        "delta CPU troppo piccolo: {} (atteso molto lavoro su 4 thread)",
        cpu2 - cpu1
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn finds_self_and_handles_bad_pid_gracefully() {
    let me = std::process::id();
    let list = list_processes().expect("enumerazione");
    assert!(
        find(&list, me).is_some(),
        "il processo di test deve essere in lista"
    );

    // Aprire un PID quasi-certamente inesistente deve dare un errore controllato,
    // mai un panic (no-panic policy, docs/06-reliability.md).
    let r = open_process(0xFFFF_FFF0);
    assert!(
        r.is_err(),
        "PID inesistente deve restituire Err, non panicare"
    );
}

#[test]
fn can_open_own_process() {
    // Possiamo sempre aprire noi stessi: verifica il path di attach felice.
    let me = std::process::id();
    assert!(open_process(me).is_ok(), "apertura del processo corrente");
}
