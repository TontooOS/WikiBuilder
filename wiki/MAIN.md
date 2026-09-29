# WikiBuilder – Wiki

Offline wiki bundle builder for TontooOS. Lists all public repositories of the
`TontooOS` GitHub organisation, downloads each in-code `wiki/` folder that
contains a `wiki/MAIN.md` file, and packs everything into a Stored
(0 compression) ZIP archive with a FishFile manifest.

- Repository: https://github.com/TontooOS/WikiBuilder
- License: TCL v27.0
- Version: 27.0.0

## Feature Index

| Feature | File | Description |
|---|---|---|
| Main index | [MAIN.md](MAIN.md) | This page |
| Rules | [RULE.md](RULE.md) | Wiki design system |
| Builder | [Builder.md](Builder.md) | List, fetch and pack pipeline |
| Manifest | [Manifest.md](Manifest.md) | `manifest.fico` format and fields |
| Usage | [Usage.md](Usage.md) | CLI flags, languages and output layout |
| Automation | [Automation.md](Automation.md) | Scheduled releases every 3 days plus manual runs |

## Quick Start

```bash
wikibuilder --out DeveloperDocumentaion.zip
```

```rust
// The bundle layout produced by WikiBuilder:
let entries = archivekit::zip_unpack(&std::fs::read("DeveloperDocumentaion.zip")?)?;
assert!(entries.iter().any(|e| e.name == "manifest.fico"));
```

See [Builder.md](Builder.md) for details.

## Changelog

- 2026-09-29: Bundle layout `<Repo>/Wiki/...` + `<Repo>/Examples/...`; downloads via git clone (no API quota)
- 2026-09-29: Token auth, rate-limit retries and per-repo error tolerance (fixes CI 403)
- 2026-09-29: Direct `/Library/System/*` dependencies instead of the SDK shim (fixes CI build)
- 2026-09-29: Added release automation (3-day schedule, manual trigger, versioned releases from 0.01)
- 2026-09-29: Initial wiki for WikiBuilder 27.0.0
