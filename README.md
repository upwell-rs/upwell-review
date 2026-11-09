# upwell-review

Independent two-stage pull request review for [`upwell-rs/upwell`](https://github.com/upwell-rs/upwell)
through local model CLIs (Codex, Gemini/Antigravity or GitHub Copilot). Discovery drafts findings,
a fresh verifier keeps, modifies or drops each draft, and the verified result is posted as one
GitHub review with inline, resolvable comments.

It is a single Rust binary that replaces the earlier bash and python scripts and keeps their
behaviour, exit codes and environment variables.

Treat the result as a second pair of eyes, not an approval: a clean review means no verified
findings, and neither stage runs the tests.

## Install

```sh
cargo install --path .
upwell-review doctor            # check the provider CLIs, git, gh and its login
```

Runtime tools: `git`, `gh` (logged in with an account that may review the repository), the CLI of
each provider you use, and optionally `mise` (git and gh run through `mise exec --` unless
`--no-mise` is given).

## Usage

```sh
upwell-review review <pr> <worktree> [options]
upwell-review review <pr> <worktree> [model] [effort] [provider] [focus] [verify-provider] [verify-model] [verify-effort]
upwell-review verify <pr> <worktree> --drafts <drafts.jsonl> [options]
upwell-review doctor [worktree] [--provider <name>]...
upwell-review drafts <drafts.jsonl>
upwell-review validate <discovery|verify|review|schema-file> <answer.json>
upwell-review mixes
```

- `<worktree>` is a clean, detached checkout of the PR head; a tree that differs from the PR head is
  refused.
- The legacy positional arguments still work; an empty one means "the default". Options win over
  positions, positions over a mix, and a mix over the defaults and the `UPWELL_VERIFY_*` variables.

| Option | Meaning |
| --- | --- |
| `--provider`, `--model`, `--effort` | discovery run (defaults: `codex`, `gpt-6.1-terra`, `medium`) |
| `--verify-provider`, `--verify-model`, `--verify-effort` | verification run (default: `UPWELL_VERIFY_PROVIDER`/`_MODEL`/`_EFFORT`, else the discovery value) |
| `--mix <name>` | a named provider mix (see below) |
| `--focus <file>` | text that narrows what discovery looks at |
| `--dry-run` | run everything except the post; print the would-be review and save `payload.json` |
| `--verify-only <drafts>` | same as the `verify` subcommand |
| `--out <dir>` | output root (default `$TMPDIR/upwell-review`) |
| `--prompts-dir <dir>` | files here override the embedded prompts and schemas of the same name |
| `--providers <file>` | providers file of named mixes |
| `--repo <owner/name>` | repository of the PR (default `upwell-rs/upwell`) |
| `--no-mise` | run git and gh directly |

Environment aliases: `UPWELL_REVIEW_OUT` (`--out`), `UPWELL_REVIEW_VERIFY_ONLY` (`--verify-only`),
`UPWELL_REVIEW_DRY_RUN=1` (`--dry-run`), `UPWELL_REVIEW_PROVIDERS` (`--providers`) and
`UPWELL_VERIFY_PROVIDER`, `UPWELL_VERIFY_MODEL`, `UPWELL_VERIFY_EFFORT`.

When you change the verifier's provider, set its model (and effort) too, since a discovery model id
is rarely valid for another provider.

## Two stages

1. **Discovery** reads `pr.diff`, checks suspected defects against the code and prints each one as an
   `UPWELL_FINDING_DRAFT {json}` line the moment it suspects it. Its final answer is only a done
   marker (`discovery-schema.json`: `DISCOVERY_DONE`, the draft count, a summary and what it could not
   check). Drafts are saved as they stream, so a run that dies half-way still leaves usable drafts.
2. **Verification** is a new provider process that has not seen the discovery conversation. It gets
   only the drafts (as untrusted claims with ids `D1`..`Dn`), the PR metadata, `pr.diff`, the review
   rules and the same read-only access. For each draft it answers `keep`, `modify` or `drop` with a
   reason (`verify-schema.json`); it cannot add findings. Only its output is posted.

Draft lines are found anywhere in a line (after a bullet, in backticks, in prose), in the plain
stream, the final answer and the reasoning stream (Copilot reasoning events, Antigravity thinking
deltas), and are deduplicated by content. Any `status` is accepted: a severity word such as `"nit"`
becomes the severity when none is given and is kept as the draft's `kind`.

Fail-closed checks (exit 3, nothing posted, run directory kept):

- the verifier answer breaks its schema, names an unknown or repeated draft id, or leaves a draft out;
- a kept or modified finding targets a line that is not an added or context line of `pr.diff`;
- with no drafts, discovery did not return a valid done marker reporting zero drafts;
- the done marker counts more drafts than were saved.

With zero drafts verification is skipped and a `NO_FINDINGS` review is posted. The head, base and
local checkout checks run before verification and again before posting.

## Providers

| Provider | CLI | Models | Effort |
| --- | --- | --- | --- |
| `codex` | `codex` | `gpt-6.1-terra`, `gpt-6.1-sol`, ... | `-c model_reasoning_effort=<effort>` |
| `gemini` | `agy` (Antigravity) | `agy models` | part of the model name (the effort is ignored) |
| `copilot` | `copilot` (GitHub Copilot CLI) | Copilot model ids | `--reasoning-effort` |

Every stage runs read-only in the worktree, in a fresh process (never a resumed session):

- `codex exec -m <model> -c model_reasoning_effort=<effort> -s read-only --ephemeral --output-schema <schema> -C <worktree> -o <answer> -`
  with the prompt on stdin.
- `agy --model <model> --mode plan --sandbox --output-format stream-json --settings <allow list> --json-schema <schema> --add-dir <worktree> --add-dir <run dir> -p <prompt>`
  with a session-only allow list for the read-only blast-radius and codegraph tools.
  `--dangerously-skip-permissions` is never used.
- `copilot -p <prompt> --model <model> --reasoning-effort <effort> --output-format json --no-ask-user --disable-builtin-mcps --disable-client-telemetry --no-auto-update -C <worktree> --add-dir <run dir> --available-tools view glob rg <read-only blast-radius and codegraph tools> --deny-tool=shell --deny-tool=write --deny-tool=memory --deny-tool=url --disable-mcp-server MCP_DOCKER --disable-mcp-server playwright --disable-mcp-server railway`.
  `--allow-all-tools`, `--allow-all-paths`, `--share`, `--resume` and `--continue` are never used.
  Copilot has no schema flag, so the last JSON block of the final message is validated instead.

The full final message of gemini and copilot runs is saved as `<stage>.response.md`. Any other
provider name exits 8.

## Mixes

`upwell-review mixes` lists the built-in mixes and those of the providers file
(`--providers`, `UPWELL_REVIEW_PROVIDERS`, else `$XDG_CONFIG_HOME/upwell-review/providers.toml` or
`~/.config/upwell-review/providers.toml` when it exists). A file entry with a built-in name replaces it.

| Mix | Discovery | Verification |
| --- | --- | --- |
| `cheap` | copilot `gpt-6.1-terra` `medium` | codex `gpt-6.1-terra` `medium` |
| `risky` | copilot `claude-opus-5.5` `high` | codex `gpt-6.1-terra` `medium` |

```toml
[mix.deep]
discovery = { provider = "codex", model = "gpt-6.1-terra", effort = "xhigh" }
verify = { provider = "codex", model = "gpt-6.1-sol", effort = "high" }   # effort defaults to medium
```

Every review with drafts costs two provider runs. A different model per stage gives a real second
opinion; the same model still helps because the verifier starts without discovery's reasoning.
Cost rule: never run `gpt-6-astra` or `gemini-4-argon-*` except as a single instance on a small,
focused slice (pass `--focus`), and never run several expensive models in parallel.

## Resume: verification only

When a run fails after discovery (provider exit 10 with drafts saved, a busy verifier lock, an
invalid verifier answer), the error prints a ready-to-paste command:

```sh
upwell-review verify <pr> <worktree> --drafts <drafts.jsonl> --provider <p> --model <m> --effort <e> \
  --verify-provider <vp> --verify-model <vm> --verify-effort <ve>
```

The drafts file may be a run's `drafts.jsonl`, the mirror `pr<N>-<head8>.jsonl` (duplicates are
merged) or a run's `verify-input.jsonl`. Each saved draft is stamped with the PR and full head sha
it was found on; a draft of another PR or head is refused with exit 5. An empty drafts file exits 1.

## Safety model

- **Read-only providers.** Every provider runs sandboxed or with an explicit read-only tool allow
  list (see above); none gets shell, write or network tools, and none can post. Only this binary
  posts, through `gh` with your main account.
- **Untrusted model output.** Drafts and answers are parsed into typed structs and validated
  against the embedded schemas; verifier claims are checked against the diff line map. Drafts are
  framed to the verifier as untrusted claims.
- **Fresh verifier.** Verification is a new process without the discovery conversation and may only
  keep, modify or drop drafts.
- **Pinned PR.** The worktree must be clean (exit 4), on the PR head (exit 2) and stay unchanged
  (exit 7); the PR head (exit 5) and base (exit 6) must not move. The diff uses fixed commit ids.
- **One run per model.** An exclusive OS file lock on `<out>/locks/<provider>--<model>.lock` (holder
  pid in `<lock>.pid`) makes a concurrent run with the same provider and model exit 9. The kernel
  releases it however the run ends. A run holds at most one lock and never waits for one; when the
  verifier differs from discovery, the discovery lock is released and the verifier's taken without
  blocking.
- **Drafts never lost.** Drafts are flushed per line to the run directory and to the mirror under
  `<out>/drafts/`, and every failure after discovery prints the resume command.

## Exit codes

| Exit | Meaning |
| --- | --- |
| 0 | posted, dry run completed, or a subcommand succeeded |
| 1 | bad arguments or an I/O error (missing focus or drafts file, empty drafts file, bad providers file) |
| 2 | the worktree head differs from the PR head |
| 3 | an answer is not valid (schema, unknown, repeated or missing draft id, line outside the diff, missing or miscounting done marker) |
| 4 | the worktree has uncommitted tracked changes |
| 5 | the PR head moved, or the drafts belong to another PR or head |
| 6 | the PR base moved |
| 7 | the local checkout changed during the review |
| 8 | unknown provider |
| 9 | the same provider and model is already running |
| 10 | a provider run failed (drafts kept) |
| 11 | `doctor` found a failing check |

## Run directory

Each run gets `<out>/pr<N>.<suffix>/` with `pr.diff`; `discovery-prompt.md`, `discovery.log`,
`discovery.json` (done marker), `discovery.response.md`; `drafts.jsonl` and `stamp.json`;
`verify-input.jsonl` (deduplicated drafts with ids), `verify-prompt.md`, `verify.log`, `verify.json`
(raw verifier answer), `verify.response.md`; `verification.jsonl` (one record per draft with the
decision, reason, original and final finding); `review.json` and `stages.json` (footer data); and
`payload.json` in a dry run. Files of a stage that did not run are absent. Verification records are
also appended to `<out>/drafts/pr<N>-<head8>.verification.jsonl`.

## Prompts and schemas

`prompts/` holds the review rules, the discovery protocol, the verification rules and the provider
checklists; `schemas/` the discovery, verify and review answer schemas. They are embedded in the
binary; `--prompts-dir` overrides any of them by file name, and `doctor` checks that they load.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
```

Git, gh and the provider stages sit behind traits, so the tests run without network access, model
CLIs or a GitHub login.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.
