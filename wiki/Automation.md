# Automation

The scheduled GitHub Actions workflow that builds `DeveloperDocumentaion.zip`
every 3 days and publishes it as a versioned release. It also supports
manual runs with an organisation or version override.

## Workflow

| Item | Value |
|---|---|
| File | `.github/workflows/wiki-bundle.yml` |
| Schedule | `0 3 */3 * *` (every 3 days at 03:00 UTC) |
| Manual trigger | `workflow_dispatch` with `org` and `version` inputs |
| Runner | `ubuntu-latest` |
| Permissions | `contents: write` (create releases) |
| Concurrency | Group `wiki-bundle`, no cancellation of running jobs |

## Steps

| Step | Description |
|---|---|
| `Resolve release version` | Runs `.github/workflows/next_version.py` |
| `Provide TontooOS frameworks` | Clones `ArchiveKit`, `FishFile`, `Foundation`, `NetworkKit` into `/Library/System/` |

> **Note:** These four repositories are the full transitive path-dependency
> closure (`ArchiveKit` needs `FishFile`, `FishFile` and `NetworkKit` need
> `Foundation`, `Foundation` needs no other TontooOS repo). When a framework
> gains a new TontooOS path dependency, add it to the clone loop.
| `Build wikibuilder` | `cargo build --release` with `WIKIBUILDER_VERSION` set |
| `Build wiki bundle` | Runs the binary with `GITHUB_TOKEN` (5000 req/h quota), extracts `manifest.fico` for the release |
| `Create release` | `gh release create` with the ZIP, the manifest and notes |

## Versions

```text
0.01 -> 0.02 -> ... -> 0.09 -> 0.10 -> ... -> 0.99 -> 1.00 -> ...
```

- The next version is resolved by `next_version.py` from this repo's own
  releases: latest `vX.Y` tag plus one, with rollover at `.99`.
- The first release starts at `0.01` when no release exists yet.
- Taken tags are skipped; a manual `version` override must match `X.Y` and
  must not exist yet, else the run fails.
- Release builds compile with `WIKIBUILDER_VERSION` set to the release
  version, so `manifest.fico` records it in `wiki.builder_version`.
- Repository files (`Cargo.toml`, `tontoo.proj`) are never rewritten;
  the version lives only in the release tag and the manifest.
- Returns `Err` (non-zero exit) when the override is malformed, the tag is
  taken, or the `gh` calls fail.

### Resolver

```rust
// .github/workflows/next_version.py (Python, stdlib only)
bump(major, minor)  // minor + 1, rollover to major + 1 at 99
fmt(major, minor)   // "0.02", "1.00", "24.43"
```

- `FAKE_LAST_TAG` and `FAKE_EXISTING_TAGS` simulate releases for local tests.
- Writes `version=X.Y` to `$GITHUB_OUTPUT`.

## Usage / Example

```bash
gh workflow run "Wiki Bundle Release" --repo TontooOS/WikiBuilder
gh workflow run "Wiki Bundle Release" --repo TontooOS/WikiBuilder \
  -f org=TontooOS -f version=0.07
```

## Changelog

- 2026-09-29: Direct `/Library/System/*` dependencies instead of the SDK shim (CI clones the four frameworks)

## Cross References

- [Builder.md](Builder.md) – the pipeline the workflow executes
- [Manifest.md](Manifest.md) – where the release version is recorded
- [Usage.md](Usage.md) – CLI flags used by the workflow
