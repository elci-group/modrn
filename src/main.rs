use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use console::style;
use walkdir::{DirEntry, WalkDir};

/// File stems that must never be declared as modules: `mod` is the classic
/// declaration file itself, `main` and `lib` are crate roots.
const RESERVED_FILES: [&str; 3] = ["mod", "main", "lib"];

/// Names that cannot be used as module identifiers, even raw (`r#`).
const UNUSABLE_IDENTS: [&str; 4] = ["crate", "self", "super", "Self"];

/// Rust strict and reserved keywords: usable as module names only via `r#`.
const KEYWORDS: [&str; 47] = [
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "unsized",
    "use", "virtual", "where", "while", "yield", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv",
];

/// Where declarations for a directory's children live.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Always `dir/mod.rs`.
    Modrs,
    /// Sibling `dir.rs` for modules, `lib.rs`/`main.rs` at the root.
    Path,
    /// Follow what already exists; fall back to `mod.rs` (or inherit the
    /// parent's style for new directories).
    Auto,
}

struct Config {
    root: PathBuf,
    dry_run: bool,
    check: bool,
    style: Style,
    watch: bool,
    interval: Duration,
}

impl Config {
    fn read_only(&self) -> bool {
        self.dry_run || self.check
    }
}

#[derive(Default)]
struct Report {
    added: Vec<String>,
    removed: Vec<String>,
    kept: Vec<String>,
    stale: Vec<String>,
    changed: bool,
    created: bool,
}

fn main() -> io::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    // When invoked as `cargo modrn`, cargo passes the subcommand name as the
    // first argument — skip it.
    let skip = if argv
        .first()
        .and_then(|a| Path::new(a).file_stem())
        .is_some_and(|s| s == "cargo-modrn")
        && argv.get(1).is_some_and(|a| a == "modrn")
    {
        2
    } else {
        1
    };
    let config = match parse_args(argv.into_iter().skip(skip).collect()) {
        Ok(config) => config,
        Err(code) => std::process::exit(code),
    };

    if !config.root.is_dir() {
        eprintln!(
            "{} {:?} is not a directory",
            style("✖ Error").red(),
            config.root
        );
        std::process::exit(1);
    }

    if config.watch {
        watch_loop(&config);
    }

    let (updated, unchanged) = sync_all(&config)?;

    if config.check {
        if updated > 0 {
            println!(
                "{} {updated} file(s) out of sync — run `modrn` to fix",
                style("✖").red()
            );
            std::process::exit(1);
        }
        println!(
            "{} all module declarations in sync ({unchanged} checked)",
            style("✓").green()
        );
    } else {
        let verb = if config.dry_run {
            "would change"
        } else {
            "updated"
        };
        println!("✨ Done. {updated} file(s) {verb}, {unchanged} up to date");
    }

    Ok(())
}

/// One full sync pass. Returns (changed files, unchanged files).
fn sync_all(config: &Config) -> io::Result<(usize, usize)> {
    let mode = if config.check {
        " (check)"
    } else if config.dry_run {
        " (dry run)"
    } else {
        ""
    };
    println!(
        "{} Scanning project at {:?}{mode}",
        style("🔍").cyan(),
        config.root
    );

    let scan = scan_tree(&config.root);
    let qualified = qualified_dirs(&scan);
    let targets = resolve_targets(&qualified, config);

    let mut updated = 0usize;
    let mut unchanged = 0usize;
    let mut sorted: Vec<&PathBuf> = qualified.iter().collect();
    sorted.sort();

    for dir in sorted {
        let desired = desired_modules(dir, &scan, &qualified);
        for warning in &desired.warnings {
            eprintln!("   {} {warning}", style("⚠").yellow());
        }
        for target in &targets[dir] {
            let report = sync_file(target, &desired.names, config.read_only())?;
            print_report(target, &report, config.read_only());
            if report.changed {
                updated += 1;
            } else {
                unchanged += 1;
            }
        }
    }

    Ok((updated, unchanged))
}

/// Re-sync whenever the tree changes, until interrupted. Meant for codegen
/// pipelines: a generator emits files, modrn keeps declarations in sync.
fn watch_loop(config: &Config) -> ! {
    loop {
        match sync_all(config) {
            Ok((updated, unchanged)) => {
                println!("✨ Done. {updated} file(s) updated, {unchanged} up to date");
            }
            Err(err) => eprintln!("{} {err}", style("⚠").yellow()),
        }
        let signature = tree_signature(&config.root);
        println!(
            "{} Watching for changes every {}ms (Ctrl+C to stop)",
            style("👀").cyan(),
            config.interval.as_millis()
        );
        loop {
            std::thread::sleep(config.interval);
            if tree_signature(&config.root) != signature {
                break;
            }
        }
        println!();
    }
}

/// A fingerprint of every directory and `.rs` file under root: paths, sizes,
/// and mtimes. Computed after syncing, so modrn's own writes don't retrigger.
fn tree_signature(root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut entries: Vec<String> = Vec::new();
    let walker = WalkDir::new(root).into_iter();
    for entry in walker.filter_entry(entry_kept).filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_dir() {
            entries.push(format!("dir:{}", path.display()));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let (len, mtime) = path
                .metadata()
                .map(|m| {
                    let mtime = m.modified().map(|t| format!("{t:?}")).unwrap_or_default();
                    (m.len(), mtime)
                })
                .unwrap_or((0, String::new()));
            entries.push(format!("{}:{len}:{mtime}", path.display()));
        }
    }
    entries.sort_unstable();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

fn parse_args(args: Vec<String>) -> Result<Config, i32> {
    let mut root = None;
    let mut dry_run = false;
    let mut check = false;
    let mut layout = Style::Auto;
    let mut watch = false;
    let mut interval = Duration::from_millis(500);

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "--check" => check = true,
            "-w" | "--watch" => watch = true,
            "--style" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("{} --style requires a value", style("✖ Error").red());
                    return Err(2);
                };
                layout = parse_style(value)?;
            }
            "--interval" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("{} --interval requires a value", style("✖ Error").red());
                    return Err(2);
                };
                interval = parse_interval(value)?;
            }
            "-h" | "--help" => {
                print_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("modrn {}", env!("CARGO_PKG_VERSION"));
                return Err(0);
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--style=") {
                    layout = parse_style(value)?;
                } else if let Some(value) = arg.strip_prefix("--interval=") {
                    interval = parse_interval(value)?;
                } else if arg.starts_with('-') {
                    eprintln!("{} unknown option: {arg}", style("✖ Error").red());
                    print_help();
                    return Err(2);
                } else if root.replace(PathBuf::from(arg)).is_some() {
                    eprintln!(
                        "{} unexpected extra argument: {arg}",
                        style("✖ Error").red()
                    );
                    return Err(2);
                }
            }
        }
        i += 1;
    }

    if check && watch {
        eprintln!(
            "{} --check and --watch cannot be combined",
            style("✖ Error").red()
        );
        return Err(2);
    }

    Ok(Config {
        root: root.unwrap_or_else(|| PathBuf::from("./src")),
        dry_run,
        check,
        style: layout,
        watch,
        interval,
    })
}

fn parse_interval(value: &str) -> Result<Duration, i32> {
    match value.parse::<u64>() {
        Ok(ms) if ms > 0 => Ok(Duration::from_millis(ms)),
        _ => {
            eprintln!(
                "{} invalid --interval `{value}` (expected positive milliseconds)",
                style("✖ Error").red()
            );
            Err(2)
        }
    }
}

fn parse_style(value: &str) -> Result<Style, i32> {
    match value {
        "modrs" => Ok(Style::Modrs),
        "path" => Ok(Style::Path),
        "auto" => Ok(Style::Auto),
        _ => {
            eprintln!(
                "{} unknown style `{value}` (expected `modrs`, `path`, or `auto`)",
                style("✖ Error").red()
            );
            Err(2)
        }
    }
}

fn print_help() {
    println!(
        "modrn — keep Rust module declarations in sync with the files on disk

USAGE:
    modrn [OPTIONS] [ROOT]
    cargo modrn [OPTIONS] [ROOT]

ARGS:
    ROOT    Directory to scan [default: ./src]

OPTIONS:
    -n, --dry-run       Show what would change without writing any files
        --check         Exit 1 (writing nothing) if anything is out of sync
    -w, --watch         Re-sync whenever the tree changes (codegen pipelines)
        --interval <MS> Watch poll interval in milliseconds [default: 500]
        --style <S>     Where declarations live: `modrs` (dir/mod.rs),
                        `path` (dir.rs, lib.rs/main.rs at the root), or
                        `auto` (follow existing files) [default: auto]
    -h, --help          Print this help
    -V, --version       Print the version"
    );
}

struct Scan {
    /// Directory -> module names derived from its `.rs` files.
    file_modules: BTreeMap<PathBuf, Vec<String>>,
    /// Every directory visited during the scan.
    dirs: Vec<PathBuf>,
}

/// Entries modrn ignores: hidden files/dirs, `target`, and `bin` directories
/// (cargo's `src/bin` convention — those are crate targets, not modules).
fn entry_kept(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') || name == "target" {
        return false;
    }
    if entry.file_type().is_dir() && name == "bin" {
        return false;
    }
    true
}

fn scan_tree(root: &Path) -> Scan {
    let mut file_modules: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let mut dirs = Vec::new();

    let walker = WalkDir::new(root).sort_by_file_name().into_iter();
    let entries = walker.filter_entry(entry_kept);

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("{} {err}", style("⚠ Skipping").yellow());
                continue;
            }
        };
        let path = entry.path();
        if entry.file_type().is_dir() {
            dirs.push(path.to_path_buf());
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            if RESERVED_FILES.contains(&stem.as_str()) {
                continue;
            }
            if let Some(parent) = path.parent() {
                file_modules
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(stem);
            }
        }
    }

    for names in file_modules.values_mut() {
        names.sort_unstable();
        names.dedup();
    }

    Scan { file_modules, dirs }
}

/// A directory qualifies for declarations when it holds module files of its
/// own or has a child directory that qualifies (so parents wire up children).
fn qualified_dirs(scan: &Scan) -> HashSet<PathBuf> {
    let mut qualified = HashSet::new();
    let mut deepest_first: Vec<&PathBuf> = scan.dirs.iter().collect();
    deepest_first.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    for dir in deepest_first {
        let has_files = scan.file_modules.get(dir).is_some_and(|v| !v.is_empty());
        let has_qualified_child = scan
            .dirs
            .iter()
            .any(|d| d.parent() == Some(dir.as_path()) && qualified.contains(d));
        if has_files || has_qualified_child {
            qualified.insert(dir.clone());
        }
    }

    qualified
}

/// Decide, for every qualified directory (parents first so children can
/// inherit their style), which file(s) hold its module declarations.
fn resolve_targets(
    qualified: &HashSet<PathBuf>,
    config: &Config,
) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut targets = HashMap::new();
    let mut path_styled: HashSet<PathBuf> = HashSet::new();
    let mut parents_first: Vec<&PathBuf> = qualified.iter().collect();
    parents_first.sort();

    for dir in parents_first {
        let inherits_path = dir.parent().is_some_and(|p| path_styled.contains(p));
        let (files, is_path) = target_files(dir, &config.root, config.style, inherits_path);
        if files.is_empty() {
            eprintln!(
                "   {} no lib.rs or main.rs at {} for --style path; skipping",
                style("⚠").yellow(),
                dir.display()
            );
        }
        if is_path {
            path_styled.insert(dir.clone());
        }
        targets.insert(dir.clone(), files);
    }

    targets
}

/// The declaration file(s) for one directory. Existing files always win over
/// the requested style — picking anything else would produce an ambiguous
/// module — with a warning when that overrides an explicit `--style`.
fn target_files(
    dir: &Path,
    root: &Path,
    layout: Style,
    inherits_path: bool,
) -> (Vec<PathBuf>, bool) {
    let is_root = dir == root;
    let mod_rs = dir.join("mod.rs");
    let sibling = (!is_root).then(|| {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        dir.parent().unwrap().join(format!("{name}.rs"))
    });

    if mod_rs.exists() {
        if layout == Style::Path {
            eprintln!(
                "   {} {} exists; using it (delete it to switch to path style)",
                style("⚠").yellow(),
                mod_rs.display()
            );
        }
        return (vec![mod_rs], false);
    }
    if let Some(sib) = &sibling {
        if sib.exists() {
            if layout == Style::Modrs {
                eprintln!(
                    "   {} {} exists; using it (delete it to switch to mod.rs style)",
                    style("⚠").yellow(),
                    sib.display()
                );
            }
            return (vec![sib.clone()], true);
        }
    }

    let crate_roots = |dir: &Path, require_decls: bool| -> Vec<PathBuf> {
        ["lib.rs", "main.rs"]
            .iter()
            .map(|n| dir.join(n))
            .filter(|p| p.exists())
            .filter(|p| !require_decls || file_has_mod_decl(p))
            .collect()
    };

    match layout {
        Style::Modrs => (vec![mod_rs], false),
        Style::Path => {
            if is_root {
                (crate_roots(dir, false), true)
            } else {
                (vec![sibling.unwrap()], true)
            }
        }
        Style::Auto => {
            if is_root {
                let roots = crate_roots(dir, true);
                if roots.is_empty() {
                    (vec![mod_rs], false)
                } else {
                    (roots, true)
                }
            } else if inherits_path {
                (vec![sibling.unwrap()], true)
            } else {
                (vec![mod_rs], false)
            }
        }
    }
}

fn file_has_mod_decl(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .any(|line| parse_mod_decl(line.trim()).is_some())
        })
        .unwrap_or(false)
}

struct Desired {
    names: Vec<String>,
    warnings: Vec<String>,
}

/// Modules a directory should declare: its own `.rs` files plus every
/// qualified child directory.
fn desired_modules(dir: &Path, scan: &Scan, qualified: &HashSet<PathBuf>) -> Desired {
    let mut names: Vec<String> = Vec::new();
    let mut warnings = Vec::new();

    if let Some(files) = scan.file_modules.get(dir) {
        names.extend(files.iter().cloned());
    }
    for child in qualified.iter().filter(|q| q.parent() == Some(dir)) {
        if let Some(name) = child.file_name() {
            names.push(name.to_string_lossy().into_owned());
        }
    }

    names.sort_unstable();
    names.dedup();

    let mut valid = Vec::new();
    for name in names {
        match validate_ident(&name) {
            Ok(()) => valid.push(name),
            Err(reason) => {
                warnings.push(format!("skipping `{name}` in {}: {reason}", dir.display()))
            }
        }
    }

    Desired {
        names: valid,
        warnings,
    }
}

fn validate_ident(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return Err("not a valid Rust module name; rename the file".into()),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("not a valid Rust module name; rename the file".into());
    }
    if UNUSABLE_IDENTS.contains(&name) {
        return Err("`{name}` cannot be used as a module name; rename the file".into());
    }
    Ok(())
}

/// Render a module name for a declaration, escaping keywords as raw idents.
fn mod_ident(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

enum LineKind {
    /// A module declaration: `name` as written (may carry an `r#` prefix),
    /// `inline` when it has a `{ .. }` body instead of `;`.
    Decl {
        name: String,
        inline: bool,
    },
    Attr,
    Doc,
    Other,
}

fn classify_line(line: &str) -> LineKind {
    let trimmed = line.trim();
    if trimmed.starts_with("#[") || trimmed.starts_with("#![") {
        return LineKind::Attr;
    }
    if trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return LineKind::Doc;
    }
    if let Some((name, inline)) = parse_mod_decl(trimmed) {
        return LineKind::Decl { name, inline };
    }
    LineKind::Other
}

/// Parse a single-line module declaration such as `pub(crate) mod foo;` or
/// `mod foo {`. Returns the name as written and whether it has an inline body.
fn parse_mod_decl(line: &str) -> Option<(String, bool)> {
    if line.starts_with("//") {
        return None;
    }
    let mut rest = line;
    if let Some(after_pub) = rest.strip_prefix("pub") {
        let after_pub = after_pub.trim_start();
        rest = if let Some(inner) = after_pub.strip_prefix('(') {
            let close = inner.find(')')?;
            inner[close + 1..].trim_start()
        } else {
            after_pub
        };
    }
    let rest = rest.strip_prefix("mod")?;
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();

    let len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '#'))
        .unwrap_or(rest.len());
    let name = &rest[..len];
    if name.is_empty() {
        return None;
    }
    match rest[len..].trim_start().chars().next() {
        Some(';') => Some((name.to_string(), false)),
        Some('{') => Some((name.to_string(), true)),
        _ => None,
    }
}

/// The logical module name behind a written identifier (`r#type` -> `type`).
fn logical_name(written: &str) -> &str {
    written.strip_prefix("r#").unwrap_or(written)
}

/// Bring one declaration file in sync with the desired module set: drop stale
/// plain declarations, keep everything else, append missing ones.
fn sync_file(path: &Path, desired: &[String], read_only: bool) -> io::Result<Report> {
    let exists = path.exists();
    let existing = if exists {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut report = Report {
        created: !exists,
        ..Report::default()
    };

    if !exists && desired.is_empty() {
        return Ok(report);
    }

    let mut declared: HashSet<String> = HashSet::new();
    let mut lines: Vec<String> = Vec::new();

    for line in existing.lines() {
        match classify_line(line) {
            LineKind::Decl { name, inline } => {
                let logical = logical_name(&name).to_string();
                declared.insert(logical.clone());
                if inline || desired.contains(&logical) {
                    report.kept.push(logical);
                    lines.push(line.to_string());
                } else {
                    // Stale declaration. Only auto-remove it when nothing is
                    // attached (attribute or doc comment on the line above).
                    let attached = lines.last().is_some_and(|l| {
                        matches!(classify_line(l), LineKind::Attr | LineKind::Doc)
                    });
                    if attached {
                        report.stale.push(logical);
                        lines.push(line.to_string());
                    } else {
                        report.removed.push(logical);
                    }
                }
            }
            _ => lines.push(line.to_string()),
        }
    }

    for name in desired {
        if !declared.contains(name) {
            lines.push(format!("pub mod {};", mod_ident(name)));
            report.added.push(name.clone());
        }
    }

    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }

    report.changed = content != existing;
    if report.changed && !read_only {
        fs::write(path, content)?;
    }

    Ok(report)
}

fn print_report(path: &Path, report: &Report, read_only: bool) {
    if !report.changed && report.stale.is_empty() {
        return;
    }

    let verb = match (report.created, read_only) {
        (true, true) => style("📂 Would create").blue(),
        (true, false) => style("📂 Created").blue(),
        (false, true) => style("📝 Would update").green(),
        (false, false) => style("📝 Updated").green(),
    };
    println!("{verb} {}", path.display());

    for name in &report.added {
        println!("   {} {name}", style("➕").green());
    }
    for name in &report.removed {
        println!("   {} {name}", style("➖").red());
    }
    for name in &report.kept {
        println!("   {} {name}", style("✅").cyan());
    }
    for name in &report.stale {
        eprintln!(
            "   {} `{name}` looks stale but has attributes or docs; not removed automatically",
            style("⚠").yellow()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_declarations() {
        assert_eq!(parse_mod_decl("pub mod foo;"), Some(("foo".into(), false)));
        assert_eq!(parse_mod_decl("mod foo;"), Some(("foo".into(), false)));
        assert_eq!(
            parse_mod_decl("pub(crate) mod foo;"),
            Some(("foo".into(), false))
        );
        assert_eq!(
            parse_mod_decl("pub mod r#type;"),
            Some(("r#type".into(), false))
        );
        assert_eq!(
            parse_mod_decl("pub mod foo; // trailing"),
            Some(("foo".into(), false))
        );
    }

    #[test]
    fn parses_inline_modules() {
        assert_eq!(parse_mod_decl("pub mod foo {"), Some(("foo".into(), true)));
        assert_eq!(parse_mod_decl("mod foo { }"), Some(("foo".into(), true)));
    }

    #[test]
    fn rejects_non_declarations() {
        assert_eq!(parse_mod_decl("// pub mod foo;"), None);
        assert_eq!(parse_mod_decl("pub use foo::bar;"), None);
        assert_eq!(parse_mod_decl("pub modfoo;"), None);
        assert_eq!(parse_mod_decl("let mod = 3;"), None);
        assert_eq!(parse_mod_decl(""), None);
    }

    #[test]
    fn validates_idents() {
        assert!(validate_ident("foo").is_ok());
        assert!(validate_ident("_foo_bar2").is_ok());
        assert!(validate_ident("my-file").is_err());
        assert!(validate_ident("9lives").is_err());
        assert!(validate_ident("self").is_err());
        assert!(validate_ident("Super").is_ok());
        assert!(validate_ident("").is_err());
    }

    #[test]
    fn escapes_keywords() {
        assert_eq!(mod_ident("type"), "r#type");
        assert_eq!(mod_ident("foo"), "foo");
    }

    #[test]
    fn maps_raw_idents_to_logical_names() {
        assert_eq!(logical_name("r#type"), "type");
        assert_eq!(logical_name("plain"), "plain");
    }
}
