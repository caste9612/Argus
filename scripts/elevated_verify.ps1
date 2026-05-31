# Verifica ETW live (Fase 2/3) + overhead, da eseguire ELEVATO (admin).
#
# Esegue i due test #[ignore] che richiedono privilegi di amministratore
# (NT Kernel Logger) in release, e scrive l'output in
# %TEMP%\argus_elevated_verify.txt.
#
# Prerequisito: aver gia' compilato i binari di test, es.
#   cargo test --release --no-run --test etw_live --test overhead
#
# Uso (da terminale elevato):
#   pwsh -NoProfile -ExecutionPolicy Bypass -File scripts\elevated_verify.ps1
$ErrorActionPreference = 'Continue'
$out = Join-Path $env:TEMP 'argus_elevated_verify.txt'
$deps = Join-Path $PSScriptRoot '..\target\release\deps'

# Ferma eventuali sessioni "NT Kernel Logger" orfane (es. lasciate da un run
# precedente interrotto o da una sospensione del sistema), così StartTrace parte
# pulito. Errori ignorati (nessuna sessione attiva = ok).
try { logman stop "NT Kernel Logger" -ets 2>$null | Out-Null } catch {}

# Trova i binari di test piu' recenti per prefisso (l'hash cambia a ogni build).
function Find-TestExe($prefix) {
    Get-ChildItem -Path $deps -Filter "$prefix-*.exe" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
$etw = Find-TestExe 'etw_live'
$ovh = Find-TestExe 'overhead'

"== ARGUS ELEVATED VERIFY ==" | Set-Content $out
if (-not $etw -or -not $ovh) {
    "ERRORE: binari di test non trovati in $deps. Compila prima con:" | Add-Content $out
    "  cargo test --release --no-run --test etw_live --test overhead" | Add-Content $out
    return
}
"--- etw_live (flame + timeline + lock + diskio) ---" | Add-Content $out
& $etw --ignored --nocapture --test-threads=1 *>&1 | Add-Content $out
"" | Add-Content $out
"--- overhead (ETW vs baseline) ---" | Add-Content $out
& $ovh --ignored --nocapture --test-threads=1 *>&1 | Add-Content $out
"== DONE ==" | Add-Content $out
