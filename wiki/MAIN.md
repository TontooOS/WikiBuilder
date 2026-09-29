# WikiBuilder – Wiki

Offline wiki bundle builder for TontooOS. Lists all public repositories of the
`TontooOS` GitHub organisation, downloads each in-code `wiki/` folder that
contains a `wiki/MAIN.md` file, and packs everything into a Stored
(0 compression) ZIP archive with a FishFile manifest.

- Repository: https://github.com/TontooOS/WikiBuilder
- License: TCL v26.1
- Version: 26.1.0

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

- 2026-09-29: Added release automation (3-day schedule, manual trigger, versioned releases from 0.01)
- 2026-09-29: Initial wiki for WikiBuilder 26.1.0
