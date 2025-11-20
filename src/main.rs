use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use walkdir::WalkDir;
use console::style;

fn main() -> io::Result<()> {
    let root = std::env::args().nth(1).unwrap_or("./src".to_string());
    let root_path = Path::new(&root);
    println!("{} Scanning project at {:?}", style("🔍").cyan(), root_path);

    process_dir(root_path)?;

    Ok(())
}

fn process_dir(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    let mut rust_files = vec![];

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "rs" && path.file_name().unwrap() != "mod.rs" {
                    rust_files.push(path.file_stem().unwrap().to_string_lossy().to_string());
                }
            }
        }
    }

    if !rust_files.is_empty() {
        let mod_rs_path = dir.join("mod.rs");
        let mut content = String::new();

        if mod_rs_path.exists() {
            content = fs::read_to_string(&mod_rs_path)?;
        } else {
            println!("{} Creating new mod.rs in {:?}", style("📂").blue(), dir);
        }

        let mut added = vec![];
        let mut present = vec![];

        for file in &rust_files {
            let line = format!("pub mod {};", file);
            if !content.contains(&line) {
                content.push_str(&line);
                content.push('\n');
                added.push(file.clone());
            } else {
                present.push(file.clone());
            }
        }

        if !added.is_empty() {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&mod_rs_path)?;
            file.write_all(content.as_bytes())?;
        }

        if !added.is_empty() || !present.is_empty() {
            println!(
                "{} {}",
                style("📝 Checked").green(),
                mod_rs_path.display()
            );
            for f in &added {
                println!("   {} {}", style("➕").green(), f);
            }
            for f in &present {
                println!("   {} {}", style("✅").cyan(), f);
            }
        }
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            process_dir(&path)?;
        }
    }

    Ok(())
}
