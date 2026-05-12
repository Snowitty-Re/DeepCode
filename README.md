# DeepCode

DeepCode is a read-only Rust CLI for generating code understanding, analysis, planning, ideas, and report outputs with DeepSeek.

It targets the DeepSeek OpenAI-compatible chat completions API and the `deepseek-v4-pro` model by default.

## Commands

```bash
deepcode summarize <path>
deepcode understand <path>
deepcode analyze <path>
deepcode plan <path> --goal "add audit logging"
deepcode refactor <path> --goal "reduce coupling in the API layer"
deepcode docs <path> --kind readme --kind architecture
deepcode diff <old-path> <new-path>
deepcode explore <path>
deepcode chat <path>
deepcode ideas <path>
deepcode report <path>
```

All commands only read the target path. DeepCode does not execute, modify, or commit code in projects it analyzes.

Global overrides are available for one-off runs:

```bash
deepcode --format markdown --output-dir reports --max-files 80 analyze ./src
deepcode --max-concurrency 8 --max-total-bytes 4000000 understand .
deepcode --model deepseek-v4-pro --base-url https://api.deepseek.com report .
deepcode --max-tokens 32768 --retry-attempts 5 report .
deepcode --config ./project.deepcode.toml --no-cache plan . --goal "split API and worker"
```

## Configuration

Copy the example config and fill in the API key manually:

```bash
cp .deepcode.example.toml .deepcode.toml
```

`.deepcode.toml` is ignored by git. The default config uses:

- `base_url = "https://api.deepseek.com"`
- `model = "deepseek-v4-pro"`
- `max_tokens = 16384`
- `thinking_enabled = false`
- `reasoning_effort = "high"`
- `retry_attempts = 3`
- `retry_backoff_ms = 1000`
- `api_timeout_secs = 600`
- `format = "both"`
- `output_dir = "deepcode-reports"`
- `max_file_bytes = 200000`
- `max_files = 200`
- `max_total_bytes = 2000000`
- `max_concurrency = 4`

DeepCode intentionally does not read the API key from environment variables.

## Output

Reports are written to `output_dir` as Markdown, JSON, or both, depending on `format`.

`deepcode docs` also writes each generated document from the model response into a separate Markdown file under a `*-docs/` directory.

`deepcode chat <path>` starts a Chinese terminal chat interface over a file or project. It scans the target, keeps conversation history on screen, and accepts direct questions such as "解释这个项目的启动流程" or "分析这个文件的风险". Built-in chat commands:

- `/rescan` rescans the current target.
- `/path <path>` switches to another file or folder and scans it.
- `/clear` clears the visible conversation.
- `/quit` exits the TUI.

Model responses are cached under `output_dir/.cache` by workflow, goal, model, base URL, and scanned file contents. Use `--no-cache` to force a fresh API request:

```bash
deepcode --no-cache analyze ./src
```

Progress and API diagnostics are written to stderr with a `[deepcode]` prefix. DeepCode reports scan progress, cache hits/misses, request attempts, retries, response parsing, and report writing. Retryable DeepSeek failures include transport errors, response body read timeouts, `429`, `500`, `502`, `503`, `504`, and empty JSON-mode message content.

## Scanning

DeepCode skips common generated and dependency paths including `.git`, `target`, `node_modules`, `dist`, `build`, and lock files. Text files larger than `max_file_bytes` are truncated before being sent to the model.

Scanning is scheduled in two stages:

- first pass: walk the tree, read metadata, apply ignore rules, and enforce `max_files` plus `max_total_bytes`
- second pass: read selected files with bounded parallelism controlled by `max_concurrency`

Only `max_file_bytes + 1` bytes are read from each selected file, so very large text files are not fully loaded into memory just to be truncated.

The scanner also records local evidence before the model call:

- files read and skipped
- bytes sent
- total lines and code lines
- per-language file, byte, and code-line counts
- per-file line, blank, comment, and longest-line metrics

`max_files` and `max_total_bytes` cap the amount of project content sent to the model for cost and latency control. Increase `max_concurrency` for fast local disks; keep it lower on spinning disks, network filesystems, or memory-constrained machines.

## Capability Map

- P0 structured code understanding: `understand` builds entrypoints, modules, symbols, imports, and dependency edges. Reports include a local structure map even when the model omits one.
- P0 automatic documentation: `docs` can generate README, architecture, API, onboarding, and changelog-style Markdown documents.
- P1 refactoring planning: `refactor --goal ...` focuses the plan and improvements sections on sequencing, risk, rollout, and tests.
- P1 diff analysis: `diff <old> <new>` compares two scanned snapshots and includes local added, removed, modified, and unchanged file evidence.
- P2 interactive exploration: `explore <path>` starts a simple REPL for repeated source-grounded questions over the same scanned context.
- P2 terminal chat: `chat <path>` starts a Chinese TUI for repeated natural-language questions over a file or project, with scan/request status shown in the terminal.
