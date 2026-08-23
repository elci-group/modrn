use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use console::style;
use walkdir::{DirEntry, WalkDir};

use crate::{RESERVED_FILES, UNUSABLE_IDENTS};

pub struct Scan {
    /// Directory -> module names derived from its `.rs` files.
    pub file_modules: BTreeMap<PathBuf, Vec<String>>,
    /// Every directory visited during the scan.
    pub dirs: Vec<PathBuf>,
}

pub struct Desired {
    pub names: Vec<String>,
    pub warnings: Vec<String>,
}

/// Entries modrn ignores: hidden files/dirs, `target`, and `bin` directories
/// (cargo's `src/bin` convention — those are crate targets, not modules).
pub fn entry_kept(entry: &DirEntry) -> bool {
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

pub fn scan_tree(root: &Path) -> Scan {
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
pub fn qualified_dirs(scan: &Scan) -> HashSet<PathBuf> {
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

/// Modules a directory should declare: its own `.rs` files plus every
/// qualified child directory.
pub fn desired_modules(dir: &Path, scan: &Scan, qualified: &HashSet<PathBuf>) -> Desired {
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
