# Builder

The list, fetch and pack pipeline. WikiBuilder runs three stages in order:
list repositories, clone wiki and example folders, write the bundle.

## Stages

| Stage | Function | Description |
|---|---|---|
| `list` | `list_repos` | Paginate `GET /orgs/{org}/repos?per_page=100&page=N` until a short page arrives |
| `fetch` | `clone_repo` + `load_repo_docs` | Shallow-clone one repo (`wiki/` + `examples/` blobs only), read both folders |
| `pack` | `pack_bundle` | Write `manifest.fico` plus all files as a Stored ZIP |

## Listing

```rust
impl Github {
    fn list_repos(&self, org: &str) -> Result<Vec<(String, String)>, String>
}
```

- Returns `(name, default_branch)` pairs for every public repository.
- Follows `?page=` pagination; stops when a page holds fewer than 100 entries.
- Returns `Err` when the organisation is missing (HTTP 404) or the API
  answers with a non-2xx status.
- Requests carry `User-Agent: TontooOS-WikiBuilder/26.1` and
  `Accept: application/vnd.github+json`.
- When a token is set (`--token` or `GITHUB_TOKEN`), requests carry
  `Authorization: Bearer <token>` for the 5000-requests-per-hour quota
  instead of the anonymous 60-requests-per-hour quota.

## Fetching

```rust
fn clone_repo(org: &str, repo: &str, branch: Option<&str>, dest: &Path)
    -> Result<String, String>

fn load_repo_docs(repo: &str, branch: &str, commit: &str, clone_dir: &Path)
    -> Option<RepoWiki>
```

- Clones with `git clone --depth 1 --filter=blob:none --sparse` (plus
  `--branch` when overridden), then checks out only `wiki` and `examples`
  via `git sparse-checkout set wiki examples`. Only the repo listing uses
  the GitHub API (1-2 requests); all downloads go through git, so no API
  quota is consumed.
- Returns the cloned commit SHA (`git rev-parse HEAD`) for the manifest.
- `load_repo_docs` collects `<Repo>/Wiki/...` from the `wiki/` folder and
  `<Repo>/Examples/...` from the `examples/` folder (both matched
  case-insensitively, missing `examples/` is fine).
- Returns `Ok(None)` when the repo has no `wiki/MAIN.md`.
  Only repositories whose code contains `wiki/MAIN.md` are bundled.
- Returns `Err` when cloning fails or the commit SHA is empty.
- A per-repo `Err` never aborts the run: the repo is recorded in the
  manifest with status `failed` plus its error message, and the run
  continues. The run only fails when zero repos were bundled.

### Retries

- `Github::get` retries rate-limit answers (HTTP 403/429 with a rate-limit
  body, up to 4 waits honoring `Retry-After` or `X-RateLimit-Reset`) and
  server errors (HTTP 5xx, up to 3 tries with backoff).
- Returns `Err` when the retries are exhausted or the failure is permanent.

### Selection rule

- Bundle the repo when `wiki/MAIN.md` exists in its code.
- Skip the repo otherwise; skipped names are recorded in the manifest with
  status `skipped_no_main`.
- Repos with `wiki/MAIN.md` but no `examples/` folder bundle `Wiki/` only.

## Packing

```rust
fn pack_bundle(manifest: &FishDocument, repos: &[RepoWiki]) -> Result<Vec<u8>, String>
```

- Uses `ZipWriter` with `CompressionLevel::None`, so every entry is Stored
  (0 compression).
- Writes `manifest.fico` first, then `<RepoName>/Wiki/...` and
  `<RepoName>/Examples/...` files sorted by repository and path.
- Returns `Err` when an entry name is unsafe or packing fails.

## Usage / Example

```bash
wikibuilder --org TontooOS --out DeveloperDocumentaion.zip
wikibuilder --org TontooOS --branch main --list-only
```

## Cross References

- [Manifest.md](Manifest.md) – the `manifest.fico` format written before packing
- [Usage.md](Usage.md) – CLI flags and bundle layout
