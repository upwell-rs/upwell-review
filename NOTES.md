# Notes on the Rust rewrite

## Ported

Everything in `legacy/` maps to one binary:

| Legacy | Rust |
| --- | --- |
| `review-pr.sh`, `review-lib.sh` | `review` (orchestrator, workspace, prompts), `provider` (one `Provider` impl per CLI), `lock`, `guard`, `git`, `github`, `cli` |
| `stream_drafts.py` | `stream` (marker scanning, normalization, dedup, stamping, mirror) and the per-provider stream parsers |
| `verify_drafts.py` | `drafts` (read, merge, head check, ids) and `verify` (apply decisions, `review.json`, `verification.jsonl`, `stages.json`) |
| `post_review.py` | `post` (inline/body split, footer, payload, dry run) behind the `GitHub` trait |
| `diffmap.py` | `diffmap` (commentable lines, C-quoted path decoding) |
| `validate_review.py` | `schema` (the JSON-schema subset the schemas use) and the `validate` subcommand |
| `drafts.py` | the `drafts` subcommand |
| `test_review_tools.py` | ported case by case into the module `tests.rs` files (drafts, verify, diffmap, schema, post, review) |
| prompts and schemas | `prompts/`, `schemas/`, embedded with `include_str!`, overridable with `--prompts-dir` |

Known gaps from the brief, now fixed:

1. Drafts are found anywhere in a line, in the plain stream, the final answer and the reasoning
   stream, deduplicated by content; any `status` is accepted and a severity word (`"nit"`) becomes
   the severity and the draft's `kind`.
2. Every JSON shape (provider events, drafts, done marker, verifier answer, review, stages, gh
   payloads) is a typed serde struct.
3. `doctor` checks the provider CLIs, git, gh and its login, mise, the providers file, the prompts
   and schemas, and (when given) that the worktree is clean and detached. Exit 11 on any failure.
4. A TOML providers file of named mixes plus the built-in `cheap` and `risky` mixes (`mixes`
   subcommand, `--mix`).

## Changed from legacy

- Subcommands: `review`, `verify` (the old `UPWELL_REVIEW_VERIFY_ONLY` mode), `doctor`, `drafts`,
  `validate`, `mixes`. The legacy positional `review` arguments and every legacy env var still work.
- The resume hint is now an `upwell-review verify ... --drafts <file>` command instead of an
  `UPWELL_REVIEW_VERIFY_ONLY=... review-pr.sh ...` line.
- `UPWELL_REVIEW_DRY_RUN` accepts any non-false value (`1`, `true`, `yes`); legacy only honoured `1`.
  `0`, `false`, `no`, `off` and the empty string leave it off.
- Option precedence: option > legacy position > `--mix` > `UPWELL_VERIFY_*` > discovery value.
  A mix therefore beats the `UPWELL_VERIFY_*` variables.
- Clap usage errors exit 1 (legacy usage exit), not clap's default 2, which would collide with
  "stale tree". `--help` and `--version` exit 0.
- `validate` exits 3 for an invalid answer and 1 for an unreadable file or unknown schema; legacy
  `validate_review.py` exited 1 for everything.
- Exit 11 is new (`doctor`). Lock I/O errors and unreadable drafts files exit 1.
- `gh` always gets an explicit `--repo` (default `upwell-rs/upwell`, `--repo` to change).
- `--no-mise` runs git and gh directly; by default they run through `mise exec --`, as before.
- Locking uses `std::fs::File::try_lock` (stable since Rust 1.89) instead of `flock(1)`; the same
  `<provider>--<model>.lock` and `.lock.pid` files are used. A `/` in a model id is replaced in the
  lock file name.
- The diff is decoded lossily as UTF-8 for the line map, so a diff with non-UTF-8 content still maps
  by line number; legacy `verify_drafts.py` read it as strict UTF-8 and crashed on such a diff.
- A copilot run also saves `<stage>.response.md` when it fails, to help debugging.
- Hints (resume command, next steps) are printed on stderr after the error message.
- Schema error messages list problems in a deterministic order that may differ from the python
  validator's order.

## Unverified

No provider CLI, model or GitHub call was run while writing this (the tool must not run `gh` or
models during development). These parts are covered only by fixtures and fakes:

- The Copilot event names `assistant.reasoning_delta` and `assistant.reasoning` and the
  `reasoningText` field of `assistant.message` are assumed from the Copilot CLI's JSONL output;
  if the CLI names them differently, reasoning-only drafts would again be missed (the final answer
  and message deltas are still scanned).
- The Antigravity `thinking_delta` field of stream-json step updates is assumed likewise.
- The exact `gh api` payload and the `gh pr view --json` fields were ported from `post_review.py`
  and `review-pr.sh` but not exercised against GitHub.
- A model that echoes the prompt's literal marker example could still create a bogus draft line, the
  same risk as legacy (template lines with `<...>` paths are rejected).

## Dependencies

Beyond the brief's list:

- `toml` (default features off; `parse` and `serde`) for the providers file, which the brief
  allows ("TOML is fine via the `toml` crate").

Not used: `fs4`/`fd-lock` (std `File::try_lock` covers the lock). `cargo tree -i openssl` and
`cargo tree -i rsa` report nothing.

## Open questions

- Should a mix lose to the `UPWELL_VERIFY_*` variables instead? The current order treats an
  explicit `--mix` as more specific than ambient environment.
- Should `doctor` also probe each provider's login (`codex login status`, `copilot` auth)? It only
  checks that the CLIs are installed, since a login probe may cost a model call.
- The license follows the brief (MIT OR Apache-2.0, matching upwell).
