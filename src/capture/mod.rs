//! Layer 2 — Capture.
//!
//! Osserva il processo target **senza modificarlo**: solo API Win32 read-only e
//! snapshot Toolhelp. Niente injection, niente hook. Vedi `docs/02-architecture.md`.

pub mod etw;
pub mod process;
pub mod sampler;
pub mod symbols;
