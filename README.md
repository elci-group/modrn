# modrn

`modrn` keeps Rust module declarations in sync with the files that actually
exist on disk. Point it at a Rust source tree and it recursively:

- adds `pub mod name;` for every new `.rs` file,
- wires child directories into their parent module (so nested modules are
  actually reachable),
- removes declarations whose file no longer exists,
- leaves everything else — comments, `pub use` lines, `#[cfg]` items, inline
  modules — untouched.

It works with both module layouts: the classic `dir/mod.rs` style and the
Rust 2018 `dir.rs` + `dir/` style (including declarations in `lib.rs` /
`main.rs` at the crate root).

## Usage

```sh
modrn [OPTIONS] [ROOT]
cargo modrn [OPTIONS] [ROOT]

# ROOT            directory to scan (default: ./src)
# -n, --dry-run   show what would change without writing any files
#     --check     exit 1 (writing nothing) if anything is out of sync
# -w, --watch     re-sync whenever the tree changes (codegen pipelines)
#     --interval <MS>  watch poll interval in milliseconds (default: 500)
#     --style <S> modrs | path | auto (default: auto)
# -h, --help      print help
# -V, --version   print version
```

Example:

```console
$ modrn ./src
🔍 Scanning project at "./src"
📝 Updated src/lib.rs
   ➕ alpha
📂 Created src/engine.rs
   ➕ core
   ➕ internal
✨ Done. 2 file(s) updated, 0 up to date
```

## Layout styles

Declarations for a directory's children can live in different files:

| Style   | Module `engine/`        | Crate root                    |
|---------|-------------------------|-------------------------------|
| `modrs` | `engine/mod.rs`         | `src/mod.rs`                  |
| `path`  | `engine.rs` (sibling)   | `src/lib.rs` and `src/main.rs`|
| `auto`  | follows existing files  | follows existing files        |

With the default `--style auto`, modrn detects the layout per directory:
an existing `mod.rs` or `dirname.rs` is always used, the crate root is
managed through `lib.rs`/`main.rs` when they already declare modules, and
new directories inherit the style of their parent (falling back to
`mod.rs`). Existing files always win over an explicit `--style` — picking
anything else would create an ambiguous module — with a warning when that
happens.

## Watch mode

`modrn --watch` keeps declarations in sync continuously — useful next to code
generators (protobuf, FFI bridges, scaffolding scripts) that emit or delete
`.rs` files:

```sh
modrn --watch --interval 250 src &
protoc --rust_out=src/gen protos/
# src/gen's declarations are synced as files appear
```

It polls the tree fingerprint (paths, sizes, mtimes) instead of adding a
filesystem-watcher dependency; modrn's own writes never retrigger a pass.

## CI: `--check`

`modrn --check` writes nothing and exits 1 when any declaration file is out
of sync, so it can gate a pipeline:

```yaml
# .github/workflows/ci.yml
- run: cargo install --path .
- run: cargo modrn --check src
```

This repository dogfoods exactly that — see
[.github/workflows/ci.yml](.github/workflows/ci.yml).

## Behavior notes

- `main.rs`, `lib.rs`, and `mod.rs` are never declared as modules.
- Hidden directories (`.git`, `.hidden`, ...), `target/`, and `bin/`
  directories (cargo's `src/bin` convention) are skipped.
- Existing declarations are detected whether written as `mod foo;`,
  `pub mod foo;`, `pub(crate) mod foo;`, or `pub mod r#type;`, and are never
  duplicated. Inline modules (`mod foo { ... }`) are left alone.
- Stale declarations are removed automatically unless they carry an attribute
  or doc comment — those are reported as warnings instead.
- File names that are Rust keywords (e.g. `type.rs`) are declared with a raw
  identifier (`pub mod r#type;`). Names that cannot be module names at all
  (e.g. `my-file.rs`, `self.rs`) are skipped with a warning.
- New declarations are appended in sorted order; existing content keeps its
  position and formatting.

## Install

```sh
cargo install --path .
```

This installs two binaries: `modrn` (run directly) and `cargo-modrn`
(enables `cargo modrn`).

## Development

```sh
cargo test    # unit + integration tests
cargo clippy  # lints
deliver --spec deliver.toml --strict  # acceptance gate
```

## License

MIT — see [LICENSE](LICENSE).
