# Builder

The list, fetch and pack pipeline. WikiBuilder runs three stages in order:
list repositories, download wiki folders, write the bundle.

## Stages

| Stage | Function | Description |
|---|---|---|
| `list` | `list_repos` | Paginate `GET /orgs/{org}/repos?per_page=100&page=N` until a short page arrives |
| `fetch` | `fetch_repo_wiki` | Download the `wiki/` folder of one repository, if it qualifies |
| `pack` | `pack_bundle` | Write `manifest.fico` plus all wiki files as a Stored ZIP |

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
impl Github {
    fn fetch_repo_wiki(&self, org: &str, repo: &str, branch: &str)
        -> Result<Option<RepoWiki>, String>
}
```

- Lists `GET /repos/{org}/{repo}/contents/wiki?ref={branch}` and recurses
  into subdirectories via `collect_contents`.
- Returns `Ok(None)` when the repo has no `wiki/` folder (HTTP 404).
- Returns `Ok(None)` when no downloaded path equals `wiki/MAIN.md`.
  Only repositories whose code contains `wiki/MAIN.md` are bundled.
- Downloads every remaining file through its `download_url`.
- Resolves the head commit SHA via `GET /repos/{org}/{repo}/branches/{branch}`;
  unreachable SHAs become `"unknown"`.
- Returns `Err` when a listing or file download fails with a non-2xx status.
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

## Packing

```rust
fn pack_bundle(manifest: &FishDocument, repos: &[RepoWiki]) -> Result<Vec<u8>, String>
```

- Uses `ZipWriter` with `CompressionLevel::None`, so every entry is Stored
  (0 compression).
- Writes `manifest.fico` first, then `<RepoName>/<relative wiki path>` files
  sorted by repository and path.
- Returns `Err` when an entry name is unsafe or packing fails.

## Usage / Example

```bash
wikibuilder --org TontooOS --out DeveloperDocumentaion.zip
wikibuilder --org TontooOS --branch main --list-only
```

## Cross References

- [Manifest.md](Manifest.md) – the `manifest.fico` format written before packing
- [Usage.md](Usage.md) – CLI flags and bundle layout
