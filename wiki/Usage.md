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
| `--timeout` | `30` | HTTP timeout per request in seconds |

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
├── ArchiveKit/MAIN.md
├── ArchiveKit/RULE.md
├── ArchiveKit/Zip.md
├── Foundation/MAIN.md
└── ...
```

- Every ZIP entry uses the Stored method (0 compression).
- `manifest.fico` is always the first entry.
- Wiki files live under `<RepoName>/` with their `wiki/` prefix stripped.
- Repositories without `wiki/MAIN.md` in their code are absent from the
  archive but listed in the manifest with status `skipped_no_main`.

## Usage / Example

```bash
wikibuilder --out DeveloperDocumentaion.zip
wikibuilder --org TontooOS --branch main --lang de_de
wikibuilder --list-only
```

## Cross References

- [Builder.md](Builder.md) – the list, fetch and pack pipeline
- [Manifest.md](Manifest.md) – the `manifest.fico` format
