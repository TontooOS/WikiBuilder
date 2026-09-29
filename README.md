# WikiBuilder

Offline wiki bundle builder for TontooOS.

WikiBuilder lists every public repository of the `TontooOS` GitHub
organisation, downloads the in-code `wiki/` folder of each repository that
contains a `wiki/MAIN.md` file, and packs everything into a single Stored
(0 compression) ZIP archive:

```text
DeveloperDocumentaion.zip
├── manifest.fico
├── <RepoName>/... (wiki files of <RepoName>)
└── ...
```

`manifest.fico` (FishFile syntax) records all repositories, their versions
and the build date.

## Made for TontooOS

Explore more at https://github.com/TontooOS/TontooOS

## Wiki

See [wiki/MAIN.md](wiki/MAIN.md) for the full documentation.

## License

TCL v26.1
