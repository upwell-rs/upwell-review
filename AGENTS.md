# upwell-review — agent instructions

PR review tool for `upwell-rs/upwell` (Rust 2024, single binary crate). It is used as a git submodule of the main repo.

## Git
- Commit with `mise exec -- git commit …`. mise loads the gitignored `.env` (copy `.env.example`), which provides the commit identity. Never run `git config user.*`.
- Conventional Commits 1.0.0: `<type>[(scope)][!]: <description>`.
- Stage explicit paths (`mise exec -- git add <paths>`), never `-a`/`.`.
- Commit small logical steps.
- Agents never push, add remotes, or run `gh`. The maintainer pushes.

## Before every commit
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings` (zero warnings)
- `cargo nextest run` (never `--retries`)
- No `openssl`/`rsa` in the dependency tree (`cargo tree -i openssl`). Ask before adding dependencies.

## Code style
- Blank line after every block.
- Variables at the top of their scope.
- Return/trailing expression isolated after a blank line (except 1–2 statement bodies).
- No inline `//` comments in bodies.
- `///` doc on every struct/enum/trait.
- Tests in sibling `<module>/tests.rs` (`#[cfg(test)] mod tests;`), never inline.
- Split files around 400 lines.
- Typed `thiserror` errors.
- Static generics over `Box<dyn>`.
- Treat `.clone()` as a smell.

## Tools
- Use the blast-radius MCP (`blast_radius` before changing a pub item, `diff_blast` before each commit) and codegraph when the repo is indexed.
- The `upwell-senior-rust` skill applies to the Rust style rules.
