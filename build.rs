//! Build script: incorpora l'icona dell'app (`assets/argus.ico`, generata da
//! `assets/argus.svg`) nell'eseguibile Windows, così appare in Explorer, nella
//! taskbar e come icona della finestra.
//!
//! L'icona è un extra estetico, non un requisito funzionale: se manca il resource
//! compiler (rc.exe della Windows SDK) non blocchiamo la build, emettiamo solo un
//! warning — coerente con la policy "degrada con grazia" di `docs/06-reliability.md`.

fn main() {
    println!("cargo:rerun-if-changed=assets/argus.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/argus.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icona non incorporata (resource compiler assente?): {e}");
        }
    }
}
