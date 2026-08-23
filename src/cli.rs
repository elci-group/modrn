use std::path::PathBuf;
use std::time::Duration;

use console::style;

use crate::Style;

pub struct Config {
    pub root: PathBuf,
    pub dry_run: bool,
    pub check: bool,
    pub style: Style,
    pub watch: bool,
    pub interval: Duration,
}

impl Config {
    pub fn read_only(&self) -> bool {
        self.dry_run || self.check
    }
}

pub fn parse_args(args: Vec<String>) -> Result<Config, i32> {
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

pub fn print_help() {
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
