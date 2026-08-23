use std::io;
use std::path::Path;

use console::style;
use modrn::{parse_args, sync_all, watch_loop};

fn main() -> io::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
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
