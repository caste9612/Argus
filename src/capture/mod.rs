//! Layer 2 — Capture.
//!
//! Osserva il processo target **senza modificarlo**: solo API Win32 read-only e
//! snapshot Toolhelp. Niente injection, niente hook. Vedi `docs/02-architecture.md`.

pub mod process;
pub mod sampler;

/// Spike ETW di Fase 2 — opt-in dietro la feature `etw` (vedi `09-decisions.md` D14).
#[cfg(feature = "etw")]
pub mod etw;
