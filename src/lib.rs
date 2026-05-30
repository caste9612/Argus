//! Libreria Argus.
//!
//! La logica vive qui (non in `main.rs`) così i test d'integrazione in `tests/`
//! possono usarla come `use argus::...`. Il binario (`main.rs`) è un wrapper
//! sottile che inizializza logging e finestra.

pub mod aggregation;
pub mod app;
pub mod capture;
pub mod persist;
pub mod ui;
pub mod util;
