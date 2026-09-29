# Manifest

The `manifest.fico` file at the bundle root. It records all repositories,
their versions and the build date in FishFile syntax.

## Builder

```rust
fn build_manifest(org: &str, repos: &[RepoWiki], skipped: &[String]) -> FishDocument
```

- Takes the bundled repositories plus the skipped repository names.
- Serializes with `FishDocument::to_string`; the result is stored as the
  first ZIP entry under the name `manifest.fico`.
- Repository table keys use `sanitize_fico_key` (anything outside
  `[A-Za-z0-9_-]` becomes `_`) so dotted repo names stay valid path segments.
- The original repository name is always kept in the `name` field.

## Fields

| Field | Type | Description |
|---|---|---|
| `wiki.builder_version` | `string` | WikiBuilder version, e.g. `"26.1.0"` |
| `wiki.build_date` | `string` | Build date as UTC ISO-8601, e.g. `"2026-09-29T12:00:00.000Z"` |
| `wiki.org` | `string` | Scanned GitHub organisation, e.g. `"TontooOS"` |
| `wiki.repo_count` | `integer` | Number of bundled repositories |
| `wiki.skipped_count` | `integer` | Number of skipped repositories |
| `wiki.failed_count` | `integer` | Number of repos that errored mid-fetch |
| `wiki.bundle` | `string` | Output file name, `"DeveloperDocumentaion.zip"` |
| `wiki.repos` | `array` | Bundled repository names in sort order |
| `repo.<Key>.name` | `string` | Original repository name |
| `repo.<Key>.version` | `string` | 7-char commit prefix, or branch when the SHA is unknown |
| `repo.<Key>.branch` | `string` | Scanned branch (explicit `--branch` or repo default) |
| `repo.<Key>.commit` | `string` | Full head commit SHA, or `"unknown"` |
| `repo.<Key>.wiki_files` | `integer` | Number of bundled wiki files (bundled repos only) |
| `repo.<Key>.status` | `string` | `"ok"`, `"skipped_no_main"` or `"failed"` |
| `repo.<Key>.error` | `string` | Failure message (failed repos only) |

## Usage / Example

```text
wiki {
    builder_version: "26.1.0"
    build_date: "2026-09-29T12:00:00.000Z"
    org: TontooOS
    repo_count: 2
    skipped_count: 1
    bundle: DeveloperDocumentaion.zip
    repos: [ArchiveKit, Foundation]
}

repo {
    ArchiveKit {
        name: ArchiveKit
        version: abcdef1
        branch: main
        commit: abcdef1234567890
        wiki_files: 9
        status: ok
    }
    EmptyRepo {
        name: EmptyRepo
        version: none
        status: skipped_no_main
    }
}
```

## Cross References

- [Builder.md](Builder.md) – how the manifest inputs are collected
- [Usage.md](Usage.md) – where `manifest.fico` sits in the bundle
