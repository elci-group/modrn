use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn modrn(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_modrn"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run modrn")
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn creates_mod_rs_for_flat_files() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");
    write(tmp.path(), "src/beta.rs", "pub fn b() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(content.contains("pub mod alpha;"), "got:\n{content}");
    assert!(content.contains("pub mod beta;"), "got:\n{content}");
}

#[test]
fn skips_main_and_lib() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(!content.contains("main"), "got:\n{content}");
    assert!(!content.contains("lib"), "got:\n{content}");
}

#[test]
fn wires_subdirectories_into_parents() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");
    write(tmp.path(), "src/engine/internal/deep.rs", "pub fn d() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let top = read(tmp.path(), "src/mod.rs");
    assert!(top.contains("pub mod engine;"), "got:\n{top}");
    let mid = read(tmp.path(), "src/engine/mod.rs");
    assert!(mid.contains("pub mod core;"), "got:\n{mid}");
    assert!(mid.contains("pub mod internal;"), "got:\n{mid}");
    let deep = read(tmp.path(), "src/engine/internal/mod.rs");
    assert!(deep.contains("pub mod deep;"), "got:\n{deep}");
}

#[test]
fn prunes_stale_and_preserves_user_content() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");
    write(
        tmp.path(),
        "src/mod.rs",
        "// hand-written header\npub mod gone;\npub mod alpha;\npub use alpha::a;\n",
    );

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(!content.contains("gone"), "got:\n{content}");
    assert!(content.contains("pub mod alpha;"), "got:\n{content}");
    assert!(
        content.contains("// hand-written header"),
        "got:\n{content}"
    );
    assert!(content.contains("pub use alpha::a;"), "got:\n{content}");
}

#[test]
fn keeps_stale_declaration_with_attributes_and_warns() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");
    write(
        tmp.path(),
        "src/mod.rs",
        "#[cfg(test)]\npub mod gone;\npub mod alpha;\n",
    );

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(content.contains("pub mod gone;"), "got:\n{content}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("stale"), "stderr:\n{stderr}");
}

#[test]
fn does_not_duplicate_non_pub_declaration() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/foo.rs", "pub fn f() {}\n");
    write(tmp.path(), "src/mod.rs", "mod foo;\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert_eq!(content.matches("mod foo;").count(), 1, "got:\n{content}");
}

#[test]
fn skips_hidden_and_target_dirs() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");
    write(tmp.path(), "src/target/generated.rs", "pub fn g() {}\n");
    write(tmp.path(), "src/.hidden/secret.rs", "pub fn s() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    assert!(!tmp.path().join("src/target/mod.rs").exists());
    assert!(!tmp.path().join("src/.hidden/mod.rs").exists());
    let content = read(tmp.path(), "src/mod.rs");
    assert!(!content.contains("target"), "got:\n{content}");
    assert!(!content.contains("hidden"), "got:\n{content}");
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    let out = modrn(tmp.path(), &["--dry-run", "src"]);
    assert!(out.status.success());
    assert!(!tmp.path().join("src/mod.rs").exists());
}

#[test]
fn escapes_keyword_file_names() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/type.rs", "pub fn t() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(content.contains("pub mod r#type;"), "got:\n{content}");
}

#[test]
fn skips_invalid_file_names_with_warning() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/my-file.rs", "pub fn m() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    assert!(!tmp.path().join("src/mod.rs").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("my-file"), "stderr:\n{stderr}");
}

#[test]
fn errors_on_missing_root() {
    let tmp = TempDir::new().unwrap();
    let out = modrn(tmp.path(), &["does-not-exist"]);
    assert!(!out.status.success());
}

#[test]
fn fixture_compiles_after_sync() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        tmp.path(),
        "src/lib.rs",
        "mod engine;\n\npub fn run() {\n    engine::core::go();\n}\n",
    );
    write(tmp.path(), "src/engine/core.rs", "pub fn go() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let check = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to run cargo check");
    assert!(
        check.status.success(),
        "cargo check failed:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

fn all_mod_rs(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().unwrap() == "mod.rs" {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn check_mode_fails_when_out_of_sync_and_passes_when_clean() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    let out = modrn(tmp.path(), &["--check", "src"]);
    assert!(
        !out.status.success(),
        "--check should fail when out of sync"
    );
    assert!(
        !tmp.path().join("src/mod.rs").exists(),
        "--check must not write"
    );

    assert!(modrn(tmp.path(), &["src"]).status.success());
    let out = modrn(tmp.path(), &["--check", "src"]);
    assert!(out.status.success(), "--check should pass when clean");
}

#[test]
fn path_style_updates_lib_rs_and_creates_sibling_files() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub fn x() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");

    let out = modrn(tmp.path(), &["--style", "path", "src"]);
    assert!(out.status.success());

    let lib = read(tmp.path(), "src/lib.rs");
    assert!(lib.contains("pub mod alpha;"), "got:\n{lib}");
    assert!(lib.contains("pub mod engine;"), "got:\n{lib}");
    assert!(lib.contains("pub fn x() {}"), "got:\n{lib}");
    let engine = read(tmp.path(), "src/engine.rs");
    assert!(engine.contains("pub mod core;"), "got:\n{engine}");
    assert!(
        all_mod_rs(tmp.path()).is_empty(),
        "no mod.rs should be created"
    );
}

#[test]
fn auto_style_detects_existing_path_layout() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod engine;\n");
    write(tmp.path(), "src/engine.rs", "// engine module\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let lib = read(tmp.path(), "src/lib.rs");
    assert_eq!(lib, "pub mod engine;\n", "lib.rs should be untouched");
    let engine = read(tmp.path(), "src/engine.rs");
    assert!(engine.contains("// engine module"), "got:\n{engine}");
    assert!(engine.contains("pub mod core;"), "got:\n{engine}");
    assert!(all_mod_rs(tmp.path()).is_empty());
}

#[test]
fn auto_style_inherits_path_for_new_dirs() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod engine;\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");
    write(tmp.path(), "src/engine/internal/deep.rs", "pub fn d() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    let engine = read(tmp.path(), "src/engine.rs");
    assert!(engine.contains("pub mod core;"), "got:\n{engine}");
    assert!(engine.contains("pub mod internal;"), "got:\n{engine}");
    let internal = read(tmp.path(), "src/engine/internal.rs");
    assert!(internal.contains("pub mod deep;"), "got:\n{internal}");
    assert!(all_mod_rs(tmp.path()).is_empty());
}

#[test]
fn forced_modrs_uses_existing_sibling_file_with_warning() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod engine;\n");
    write(tmp.path(), "src/engine.rs", "// engine module\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");

    let out = modrn(tmp.path(), &["--style", "modrs", "src"]);
    assert!(out.status.success());

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("engine.rs"), "stderr:\n{stderr}");
    assert!(!tmp.path().join("src/engine/mod.rs").exists());
    let engine = read(tmp.path(), "src/engine.rs");
    assert!(engine.contains("pub mod core;"), "got:\n{engine}");
}

#[test]
fn path_style_prunes_stale_in_lib_rs() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod gone;\npub fn x() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    let out = modrn(tmp.path(), &["--style", "path", "src"]);
    assert!(out.status.success());

    let lib = read(tmp.path(), "src/lib.rs");
    assert!(!lib.contains("gone"), "got:\n{lib}");
    assert!(lib.contains("pub mod alpha;"), "got:\n{lib}");
    assert!(lib.contains("pub fn x() {}"), "got:\n{lib}");
}

#[test]
fn path_style_manages_both_crate_roots() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/lib.rs", "pub fn x() {}\n");
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/engine/core.rs", "pub fn c() {}\n");

    let out = modrn(tmp.path(), &["--style", "path", "src"]);
    assert!(out.status.success());

    let lib = read(tmp.path(), "src/lib.rs");
    assert!(lib.contains("pub mod engine;"), "got:\n{lib}");
    let main = read(tmp.path(), "src/main.rs");
    assert!(main.contains("pub mod engine;"), "got:\n{main}");
}

#[test]
fn cargo_subcommand_prefix_is_tolerated() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    // `cargo modrn src` reaches the binary as `cargo-modrn modrn src`.
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-modrn"))
        .args(["modrn", "src"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to run cargo-modrn");
    assert!(out.status.success());

    let content = read(tmp.path(), "src/mod.rs");
    assert!(content.contains("pub mod alpha;"), "got:\n{content}");
}

#[test]
fn skips_cargo_bin_dir() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/bin/some-tool.rs", "fn main() {}\n");

    let out = modrn(tmp.path(), &["src"]);
    assert!(out.status.success());

    assert!(!tmp.path().join("src/mod.rs").exists());
    assert!(!tmp.path().join("src/bin/mod.rs").exists());
}

#[test]
fn check_and_watch_cannot_be_combined() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");

    let out = modrn(tmp.path(), &["--check", "--watch", "src"]);
    assert!(!out.status.success());
}

struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(cond: impl Fn() -> bool, what: &str) {
    for _ in 0..100 {
        if cond() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn watch_mode_syncs_new_files() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "src/alpha.rs", "pub fn a() {}\n");

    let child = Command::new(env!("CARGO_BIN_EXE_modrn"))
        .args(["--watch", "--interval", "100", "src"])
        .current_dir(tmp.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn modrn --watch");
    let _guard = KillOnDrop(child);

    let mod_rs = tmp.path().join("src/mod.rs");
    wait_for(|| mod_rs.exists(), "initial sync to create mod.rs");
    assert!(read(tmp.path(), "src/mod.rs").contains("pub mod alpha;"));

    write(tmp.path(), "src/beta.rs", "pub fn b() {}\n");
    wait_for(
        || read(tmp.path(), "src/mod.rs").contains("pub mod beta;"),
        "watch mode to pick up beta.rs",
    );
}
