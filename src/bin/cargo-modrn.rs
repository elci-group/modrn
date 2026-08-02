//! Shim so the tool also installs as `cargo-modrn` (enabling `cargo modrn`)
//! via cargo's `src/bin` autodiscovery. All logic lives in `main.rs`; the
//! binary detects at runtime how it was invoked.
include!("../main.rs");
