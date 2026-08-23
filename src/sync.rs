use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use console::style;

use crate::cli::Config;
use crate::parse::{logical_name, mod_ident, parse_mod_decl};
use crate::scan::{desired_modules, qualified_dirs, scan_tree};

#[derive(Default)]
pub struct Report {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub kept: Vec<String>,
    pub stale: Vec<String>,
    pub changed: bool,
    pub created: bool,
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

/// One full sync pass. Returns (changed files, unchanged files).
pub fn sync_all(config: &Config) -> io::Result<(usize, usize)> {
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
    layout: crate::Style,
    inherits_path: bool,
) -> (Vec<PathBuf>, bool) {
    let is_root = dir == root;
    let mod_rs = dir.join("mod.rs");
    let sibling = (!is_root).then(|| {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        dir.parent().unwrap().join(format!("{name}.rs"))
    });

    if mod_rs.exists() {
        if layout == crate::Style::Path {
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
            if layout == crate::Style::Modrs {
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
        crate::Style::Modrs => (vec![mod_rs], false),
        crate::Style::Path => {
            if is_root {
                (crate_roots(dir, false), true)
            } else {
                (vec![sibling.unwrap()], true)
            }
        }
        crate::Style::Auto => {
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

/// Bring one declaration file in sync with the desired module set: drop stale
/// plain declarations, keep everything else, append missing ones.
pub fn sync_file(path: &Path, desired: &[String], read_only: bool) -> io::Result<Report> {
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

pub fn print_report(path: &Path, report: &Report, read_only: bool) {
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
