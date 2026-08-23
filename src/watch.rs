use std::path::Path;

use console::style;
use walkdir::WalkDir;

use crate::cli::Config;
use crate::scan::entry_kept;
use crate::sync::sync_all;

#[cfg(feature = "fsnotify")]
use std::sync::mpsc;

/// Re-sync whenever the tree changes, until interrupted. Meant for codegen
/// pipelines: a generator emits files, modrn keeps declarations in sync.
pub fn watch_loop(config: &Config) -> ! {
    #[cfg(feature = "fsnotify")]
    {
        if let Ok(()) = watch_with_notify(config) {
            std::process::exit(0);
        }
    }

    watch_with_polling(config)
}

#[cfg(feature = "fsnotify")]
#[allow(dead_code)]
fn watch_with_notify(config: &Config) -> Result<(), notify::Error> {
    use notify::Watcher;

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;

    watcher.watch(&config.root, notify::RecursiveMode::Recursive)?;

    println!(
        "{} Watching for changes using filesystem events (Ctrl+C to stop)",
        style("👀").cyan()
    );

    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Ok(_)) => match sync_all(config) {
                Ok((updated, unchanged)) => {
                    println!("✨ Done. {updated} file(s) updated, {unchanged} up to date");
                }
                Err(err) => eprintln!("{} {err}", style("⚠").yellow()),
            },
            Ok(Err(err)) => eprintln!("{} {err}", style("⚠").yellow()),
            Err(_) => continue,
        }
    }
}

#[cfg(not(feature = "fsnotify"))]
#[allow(dead_code)]
fn watch_with_notify(_config: &Config) -> Result<(), String> {
    Err("fsnotify feature not enabled".into())
}

fn watch_with_polling(config: &Config) -> ! {
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
