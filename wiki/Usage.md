# Usage

How to run WikiBuilder, switch languages and read the output bundle.

## CLI Flags

| Flag | Default | Description |
|---|---|---|
| `--org` | `TontooOS` | GitHub organisation to scan |
| `--branch` | repo default | Branch to read the `wiki/` folder from |
| `--out` | `DeveloperDocumentaion.zip` | Output ZIP file (Stored, 0 compression) |
| `--lang` | `en_us` | Status language (`en_us` or `de_de`) |
| `--lang-dir` | auto | Directory with `<lang>.json` files |
| `--list-only` | off | Only print repositories, build no bundle |
| `--timeout` | `30` | HTTP timeout per request in seconds (repo listing only) |
| `--token` | `GITHUB_TOKEN` env | GitHub token for the repo listing (higher rate limits) |
| `--workdir` | temp dir | Directory for the shallow repo clones (temp dir is removed after the run) |

## Languages

```bash
wikibuilder --lang de_de
wikibuilder --lang en_us --lang-dir ./lang
```

- Status messages come from `lang/en_us.json` and `lang/de_de.json`
  (`{"lang": ..., "translations": {...}}` format).
- The `lang/` folder is resolved from `--lang-dir`, else from `lang/` next
  to the executable, else from `lang/` in the current folder.
- Missing keys render as the key itself; a missing language falls back to
  `en_us`.
- Returns `Err` never for language handling; unknown languages silently use
  the fallback chain.

## Output Layout

```text
DeveloperDocumentaion.zip
├── manifest.fico
├── ArchiveKit/Wiki/MAIN.md
├── ArchiveKit/Wiki/RULE.md
├── ArchiveKit/Wiki/Zip.md
├── ArchiveKit/Examples/demo.rs
├── Foundation/Wiki/MAIN.md
└── ...
```

- Every ZIP entry uses the Stored method (0 compression).
- `manifest.fico` is always the first entry.
- Wiki files live under `<RepoName>/Wiki/` with their `wiki/` prefix stripped.
- Example files live under `<RepoName>/Examples/` with their `examples/`
  prefix stripped (repos without an `examples/` folder bundle `Wiki/` only).
- Downloads use `git clone` (shallow, `wiki` + `examples` blobs only), so
  only the repo listing touches the GitHub API; `git` must be installed.
- Repositories without `wiki/MAIN.md` in their code are absent from the
  archive but listed in the manifest with status `skipped_no_main`.

## Usage / Example

```bash
wikibuilder --out DeveloperDocumentaion.zip
wikibuilder --org TontooOS --branch main --lang de_de
wikibuilder --list-only
GITHUB_TOKEN=ghp_... wikibuilder --out DeveloperDocumentaion.zip
```

## Error Behavior

- Repos that fail mid-fetch do not abort the run; they land in the manifest
  with status `failed` plus an `error` message.
- Returns `Err` (non-zero exit) when the repo list cannot be fetched or when
  zero repos were bundled.

## Cross References

- [Builder.md](Builder.md) – the list, fetch and pack pipeline
- [Manifest.md](Manifest.md) – the `manifest.fico` format
