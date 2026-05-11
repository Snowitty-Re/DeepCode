# DeepCode

DeepCode is a read-only Rust CLI for generating code understanding, analysis, planning, ideas, and report outputs with DeepSeek.

The first version targets the DeepSeek OpenAI-compatible chat completions API and the `deepseek-v4-pro` model.

## Commands

```bash
deepcode summarize <path>
deepcode analyze <path>
deepcode plan <path> --goal "add audit logging"
deepcode ideas <path>
deepcode report <path>
```

All commands only read the target path. DeepCode does not execute, modify, or commit code in projects it analyzes.

## Configuration

Copy the example config and fill in the API key manually:

```bash
cp .deepcode.example.toml .deepcode.toml
```

`.deepcode.toml` is ignored by git. The default config uses:

- `base_url = "https://api.deepseek.com"`
- `model = "deepseek-v4-pro"`
- `format = "both"`
- `output_dir = "deepcode-reports"`

DeepCode intentionally does not read the API key from environment variables in this MVP.

## Output

Reports are written to `output_dir` as Markdown, JSON, or both, depending on `format`.

Model responses are cached under `output_dir/.cache` by workflow, goal, model, base URL, and scanned file contents. Use `--no-cache` to force a fresh API request:

```bash
deepcode --no-cache analyze ./src
```

## Scanning

DeepCode skips common generated and dependency paths including `.git`, `target`, `node_modules`, `dist`, `build`, and lock files. Text files larger than `max_file_bytes` are truncated before being sent to the model.
