pub mod cli;
pub mod parse;
pub mod scan;
pub mod sync;
pub mod watch;

pub use cli::{parse_args, Config};
pub use scan::{desired_modules, qualified_dirs, scan_tree, Desired, Scan};
pub use sync::{sync_all, Report};
pub use watch::watch_loop;

/// File stems that must never be declared as modules: `mod` is the classic
/// declaration file itself, `main` and `lib` are crate roots.
pub const RESERVED_FILES: [&str; 3] = ["mod", "main", "lib"];

/// Names that cannot be used as module identifiers, even raw (`r#`).
pub const UNUSABLE_IDENTS: [&str; 4] = ["crate", "self", "super", "Self"];

/// Rust strict and reserved keywords: usable as module names only via `r#`.
pub const KEYWORDS: [&str; 47] = [
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "unsized",
    "use", "virtual", "where", "while", "yield", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv",
];

/// Where declarations for a directory's children live.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Always `dir/mod.rs`.
    Modrs,
    /// Sibling `dir.rs` for modules, `lib.rs`/`main.rs` at the root.
    Path,
    /// Follow what already exists; fall back to `mod.rs` (or inherit the
    /// parent's style for new directories).
    Auto,
}
