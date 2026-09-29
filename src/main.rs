//! WikiBuilder: offline wiki bundle builder for TontooOS.
//!
//! Lists every public repository of the `TontooOS` GitHub organisation,
//! downloads the in-code `wiki/` folder of each repository that contains a
//! `wiki/MAIN.md` file, and packs everything into a single Stored
//! (0 compression) ZIP archive:
//!
//! ```text
//! DeveloperDocumentaion.zip
//! ├── manifest.fico
//! ├── <RepoName>/... (wiki files of <RepoName>)
//! └── ...
//! ```
//!
//! Networking uses NetworkKit (blocking HTTP over `ureq` + `rustls`), JSON
//! parsing uses Foundation (`JsonValue`, no serde), the manifest is written
//! with FishFile (`.fico`), and the archive is built with ArchiveKit
//! (`ZipWriter` with `CompressionLevel::None`, i.e. Stored).

use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use archivekit::{CompressionLevel, ZipWriter, ZipWriterOptions};
use fishfile::{FishDocument, FishValue};
use foundation::date::{Date, ISO8601DateFormatter};
use foundation::serialization::{JSONSerialization, JsonValue};
use networkkit::http::{HttpRequest, HttpResponse};

/// Current builder version, also recorded in `manifest.fico`.
/// Release builds override it with the `WIKIBUILDER_VERSION` env var
/// (set by `.github/workflows/wiki-bundle.yml`); local builds use the default.
pub const BUILDER_VERSION: &str = match option_env!("WIKIBUILDER_VERSION") {
    Some(v) => v,
    None => "27.0.0",
};
/// Default output file name in the current folder.
pub const DEFAULT_OUT: &str = "DeveloperDocumentaion.zip";
/// GitHub API base URL.
pub const GITHUB_API: &str = "https://api.github.com";
/// User agent sent with every GitHub request.
pub const USER_AGENT: &str = "TontooOS-WikiBuilder/26.1";

#[derive(Parser, Debug)]
#[command(name = "wikibuilder", about = "Build the offline TontooOS wiki bundle")]
struct Args {
    /// GitHub organisation to scan.
    #[arg(long, default_value = "TontooOS")]
    org: String,
    /// Branch to read the wiki folder from. Defaults to each repo's default branch.
    #[arg(long)]
    branch: Option<String>,
    /// Output ZIP file (Stored, 0 compression).
    #[arg(long, default_value = DEFAULT_OUT)]
    out: PathBuf,
    /// Language code for status messages (`en_us` or `de_de`).
    #[arg(long, default_value = "en_us")]
    lang: String,
    /// Directory containing `<lang>.json` language files.
    #[arg(long)]
    lang_dir: Option<PathBuf>,
    /// Only list repositories, do not download or pack anything.
    #[arg(long)]
    list_only: bool,
    /// HTTP timeout per request in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    /// Directory for the shallow repo clones (default: temp dir, removed after the run).
    #[arg(long)]
    workdir: Option<PathBuf>,
    /// GitHub token for the repo listing (higher rate limits).
    /// Falls back to the GITHUB_TOKEN environment variable.
    /// Downloads use `git clone` and need no token.
    #[arg(long)]
    token: Option<String>,
}

// ---------------------------------------------------------------------------
// Localization (lang/en_us.json + lang/de_de.json, used via system locale)
// ---------------------------------------------------------------------------

/// Minimal string store backed by the JSON language files in `lang/`.
#[derive(Debug, Clone)]
struct LangStore {
    map: HashMap<String, String>,
}

impl LangStore {
    fn load(lang_dir: &Path, lang: &str) -> Self {
        let primary = lang_dir.join(format!("{lang}.json"));
        let fallback = lang_dir.join("en_us.json");
        for path in [primary, fallback] {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok((_, map)) = JSONSerialization::parse_lang_file(&text) {
                    return Self { map };
                }
            }
        }
        Self { map: HashMap::new() }
    }

    /// Look up `key`; missing keys render as the key itself.
    fn t(&self, key: &str) -> String {
        self.map.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    /// Look up `key` and replace `{name}` style placeholders.
    fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let mut out = self.t(key);
        for (name, value) in vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}

/// Resolve the `lang/` directory: explicit `--lang-dir`, else `lang/`
/// next to the executable, else `lang/` in the current folder.
fn resolve_lang_dir(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("lang");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("lang")
}

// ---------------------------------------------------------------------------
// GitHub API (via NetworkKit, JSON via Foundation)
// ---------------------------------------------------------------------------

/// A downloaded wiki file with its path inside the bundle.
#[derive(Debug, Clone)]
struct WikiFile {
    /// Bundle path: `<RepoName>/<relative wiki path>`.
    arc_path: String,
    data: Vec<u8>,
}

/// Collected wiki of one repository.
#[derive(Debug, Clone)]
struct RepoWiki {
    name: String,
    branch: String,
    commit: String,
    files: Vec<WikiFile>,
}

/// GitHub API client (via NetworkKit, JSON via Foundation).
///
/// Holds the timeout and the optional token. All listing, content and
/// branch requests go through [`Github::get`], which authenticates when a
/// token is set and retries transient failures (rate limits, 5xx).
#[derive(Debug, Clone, Default)]
struct Github {
    timeout: Duration,
    token: Option<String>,
}

impl Github {
    /// Blocking GET returning `(status, body)`.
    fn get(&self, url: &str) -> Result<(u16, Vec<u8>), String> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let mut req = HttpRequest::get(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .timeout(self.timeout);
            if let Some(token) = self.token.as_deref() {
                req = req.header("Authorization", &format!("Bearer {token}"));
            }
            let resp = req
                .send()
                .map_err(|e| format!("request failed for {url}: {e:?}"))?;
            if (resp.status == 403 || resp.status == 429)
                && String::from_utf8_lossy(resp.bytes()).contains("rate limit")
                && attempt <= 4
            {
                let wait = retry_after_secs(&resp).unwrap_or(60).min(600);
                eprintln!("rate limited, waiting {wait}s (attempt {attempt})...");
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            if resp.status >= 500 && attempt <= 3 {
                std::thread::sleep(Duration::from_secs(2 * u64::from(attempt)));
                continue;
            }
            return Ok((resp.status, resp.bytes().to_vec()));
        }
    }

    /// List all public repositories of `org` (follows `?page=` pagination).
    fn list_repos(&self, org: &str) -> Result<Vec<(String, String)>, String> {
        let mut repos = Vec::new();
        let mut page = 1u32;
        loop {
            let url = format!("{GITHUB_API}/orgs/{org}/repos?per_page=100&page={page}");
            let (status, body) = self.get(&url)?;
            if status == 404 {
                return Err(format!("organisation '{org}' not found (HTTP 404)"));
            }
            if !(200..300).contains(&status) {
                return Err(format!("list repos failed (HTTP {status})"));
            }
            let root = json_text(&body)?;
            let items = root.as_array().ok_or("expected JSON array for repos")?;
            if items.is_empty() {
                break;
            }
            for item in items {
                if let Some(name) = str_field(item, "name") {
                    let branch =
                        str_field(item, "default_branch").unwrap_or_else(|| "main".to_string());
                    repos.push((name, branch));
                }
            }
            if items.len() < 100 {
                break;
            }
            page += 1;
        }
        Ok(repos)
    }
}

/// Seconds until the GitHub rate limit resets (`Retry-After` or
/// `X-RateLimit-Reset`), if the response carries either header.
fn retry_after_secs(resp: &HttpResponse) -> Option<u64> {
    if let Some(value) = resp.header("retry-after") {
        if let Ok(secs) = value.trim().parse::<u64>() {
            return Some(secs.saturating_add(5));
        }
    }
    if let Some(value) = resp.header("x-ratelimit-reset") {
        if let Ok(reset) = value.trim().parse::<i64>() {
            return Some((reset - Date::now().timestamp() + 5).max(5) as u64);
        }
    }
    None
}

fn json_text(bytes: &[u8]) -> Result<JsonValue, String> {
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("response is not UTF-8: {e}"))?;
    JsonValue::parse(&text).map_err(|e| format!("invalid JSON: {e:?}"))
}

fn str_field(value: &JsonValue, field: &str) -> Option<String> {
    value.get(field).and_then(|v| v.as_str()).map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Wiki fetch via git clone (no API quota: only the repo listing uses HTTP)
// ---------------------------------------------------------------------------

/// Run `git` with `args`, returning stdout trimmed. `Err` carries git's
/// stderr message, prefixed with the repo name when given.
fn run_git(args: &[&str], repo: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("{repo}: cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{repo}: git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Shallow-clone one repo (`HEAD` commit, `wiki/` blobs only) into `dest`.
/// `branch` overrides the default branch when set. Returns the commit SHA.
/// Uses the git protocol only, so no GitHub API quota is consumed.
fn clone_repo(org: &str, repo: &str, branch: Option<&str>, dest: &Path) -> Result<String, String> {
    let url = format!("https://github.com/{org}/{repo}.git");
    let dest_str = dest.to_string_lossy().to_string();
    if branch.is_some() {
        run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--filter=blob:none",
                "--sparse",
                "--branch",
                branch.unwrap_or_default(),
                &url,
                &dest_str,
            ],
            repo,
        )?;
    } else {
        run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--filter=blob:none",
                "--sparse",
                &url,
                &dest_str,
            ],
            repo,
        )?;
    }
    // Fetch only the wiki and examples folder contents
    // (empty checkout when both are missing).
    run_git(
        &["-C", &dest_str, "sparse-checkout", "set", "wiki", "examples"],
        repo,
    )?;
    let sha = run_git(&["-C", &dest_str, "rev-parse", "HEAD"], repo)?;
    if sha.is_empty() {
        return Err(format!("{repo}: empty commit SHA after clone"));
    }
    Ok(sha)
}

/// Find a child directory of `parent`, preferring the exact `name` and
/// falling back to a case-insensitive match (`wiki` vs `Wiki`).
fn find_child_dir(parent: &Path, name: &str) -> Option<PathBuf> {
    let exact = parent.join(name);
    if exact.is_dir() {
        return Some(exact);
    }
    fs::read_dir(parent)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
}

/// Collect every file below `dir` into `out` with `prefix` prepended.
fn collect_dir_files(dir: &Path, prefix: &str, out: &mut Vec<WikiFile>) -> Option<()> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path.strip_prefix(dir).ok()?;
                out.push(WikiFile {
                    arc_path: format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/")),
                    data: fs::read(&path).ok()?,
                });
            }
        }
    }
    Some(())
}

/// Read the checked-out `wiki/` and `examples/` folders into bundle files
/// (`<Repo>/Wiki/...` and `<Repo>/Examples/...`).
/// Returns `None` when `wiki/MAIN.md` is missing: only repositories whose
/// code contains `wiki/MAIN.md` are bundled.
fn load_repo_docs(
    repo: &str,
    branch: &str,
    commit: &str,
    clone_dir: &Path,
) -> Option<RepoWiki> {
    let wiki_dir = find_child_dir(clone_dir, "wiki")?;
    if !wiki_dir.join("MAIN.md").is_file() {
        return None;
    }
    let mut files = Vec::new();
    collect_dir_files(&wiki_dir, &format!("{repo}/Wiki"), &mut files)?;
    if let Some(examples_dir) = find_child_dir(clone_dir, "examples") {
        collect_dir_files(&examples_dir, &format!("{repo}/Examples"), &mut files)?;
    }
    files.sort_by(|a, b| a.arc_path.cmp(&b.arc_path));
    Some(RepoWiki {
        name: repo.to_string(),
        branch: branch.to_string(),
        commit: commit.to_string(),
        files,
    })
}

// ---------------------------------------------------------------------------
// manifest.fico (via FishFile)
// ---------------------------------------------------------------------------

/// Make a repo name safe for a FishFile dotted path segment.
fn sanitize_fico_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Split a repo's file count into `(wiki_files, example_files)`
/// by bundle path prefix.
fn split_counts(repo: &RepoWiki) -> (usize, usize) {
    let examples = repo
        .files
        .iter()
        .filter(|f| f.arc_path.starts_with(&format!("{}/Examples/", repo.name)))
        .count();
    (repo.files.len() - examples, examples)
}

/// Short version string: 7-char commit prefix, else the branch name.
fn repo_version(repo: &RepoWiki) -> String {
    if repo.commit.len() >= 7 {
        repo.commit[..7].to_string()
    } else {
        repo.branch.clone()
    }
}

/// Build the `manifest.fico` document: all repos, versions and build date.
/// `failed` holds `(name, error)` pairs for repos that errored mid-fetch;
/// they are recorded with status `failed` so one bad repo never aborts
/// the whole bundle.
fn build_manifest(
    org: &str,
    repos: &[RepoWiki],
    skipped: &[String],
    failed: &[(String, String)],
) -> FishDocument {
    let mut doc = FishDocument::new();
    doc.set("wiki.builder_version", BUILDER_VERSION);
    doc.set("wiki.build_date", ISO8601DateFormatter::string_from(&Date::now()));
    doc.set("wiki.org", org);
    doc.set("wiki.repo_count", repos.len() as i64);
    doc.set("wiki.skipped_count", skipped.len() as i64);
    doc.set("wiki.failed_count", failed.len() as i64);
    doc.set("wiki.bundle", DEFAULT_OUT);
    let names: Vec<FishValue> = repos.iter().map(|r| FishValue::from(r.name.clone())).collect();
    doc.set("wiki.repos", FishValue::from(names));
    for repo in repos {
        let base = format!("repo.{}", sanitize_fico_key(&repo.name));
        let (wiki_files, example_files) = split_counts(repo);
        doc.set(&format!("{base}.name"), repo.name.clone());
        doc.set(&format!("{base}.version"), repo_version(repo));
        doc.set(&format!("{base}.branch"), repo.branch.clone());
        doc.set(&format!("{base}.commit"), repo.commit.clone());
        doc.set(&format!("{base}.wiki_files"), wiki_files as i64);
        doc.set(&format!("{base}.example_files"), example_files as i64);
        doc.set(&format!("{base}.status"), "ok");
    }
    for name in skipped {
        let base = format!("repo.{}", sanitize_fico_key(name));
        doc.set(&format!("{base}.name"), name.clone());
        doc.set(&format!("{base}.version"), "none");
        doc.set(&format!("{base}.status"), "skipped_no_main");
    }
    for (name, err) in failed {
        let base = format!("repo.{}", sanitize_fico_key(name));
        doc.set(&format!("{base}.name"), name.clone());
        doc.set(&format!("{base}.version"), "none");
        doc.set(&format!("{base}.status"), "failed");
        doc.set(&format!("{base}.error"), err.clone());
    }
    doc
}

// ---------------------------------------------------------------------------
// Bundle (via ArchiveKit, Stored = 0 compression)
// ---------------------------------------------------------------------------

/// Pack the manifest plus all wiki files with 0 compression (Stored).
fn pack_bundle(manifest: &FishDocument, repos: &[RepoWiki]) -> Result<Vec<u8>, String> {
    let mut writer = ZipWriter::with_options(ZipWriterOptions {
        level: CompressionLevel::None,
        comment: format!("TontooOS offline wiki bundle (WikiBuilder {BUILDER_VERSION})"),
    });
    writer
        .append_file("manifest.fico", manifest.to_string().as_bytes())
        .map_err(|e| format!("pack manifest.fico: {e:?}"))?;
    for repo in repos {
        for file in &repo.files {
            writer
                .append_file(&file.arc_path, &file.data)
                .map_err(|e| format!("pack {}: {e:?}", file.arc_path))?;
        }
    }
    Ok(writer.finish())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn run(args: &Args, lang: &LangStore) -> Result<(), String> {
    let token = args.token.clone().or_else(|| {
        std::env::var("GITHUB_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
    });
    let github = Github {
        timeout: Duration::from_secs(args.timeout),
        token,
    };
    println!("{}", lang.tf("status.fetch_repos", &[("org", &args.org)]));
    let listed = github.list_repos(&args.org)?;
    println!(
        "{}",
        lang.tf("status.found_repos", &[("count", &listed.len().to_string())])
    );
    if args.list_only {
        for (name, branch) in &listed {
            println!("  {name} ({branch})");
        }
        return Ok(());
    }

    let mut repos: Vec<RepoWiki> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    // Shallow clones live here; the default temp dir is removed after the run.
    let (workdir, keep_workdir) = match &args.workdir {
        Some(dir) => (dir.clone(), true),
        None => (
            std::env::temp_dir().join(format!("wikibuilder-{}", std::process::id())),
            false,
        ),
    };
    fs::create_dir_all(&workdir)
        .map_err(|e| format!("cannot create workdir '{}': {e}", workdir.display()))?;
    for (name, default_branch) in &listed {
        let branch = args.branch.as_ref().unwrap_or(default_branch);
        print!(
            "{}",
            lang.tf("status.fetch_wiki", &[("repo", name), ("branch", branch)])
        );
        // Repo names are `[A-Za-z0-9._-]`: safe as directory names.
        let dest = workdir.join(name);
        let _ = fs::remove_dir_all(&dest);
        let outcome = clone_repo(&args.org, name, args.branch.as_deref(), &dest).map(
            |commit| load_repo_docs(name, branch, &commit, &dest),
        );
        match outcome {
            Ok(Some(wiki)) => {
                let (wiki_files, example_files) = split_counts(&wiki);
                println!(
                    " {}",
                    lang.tf(
                        "status.repo_ok",
                        &[
                            ("wiki", &wiki_files.to_string()),
                            ("examples", &example_files.to_string()),
                        ]
                    )
                );
                repos.push(wiki);
            }
            Ok(None) => {
                println!(" {}", lang.t("status.repo_skip"));
                skipped.push(name.clone());
            }
            Err(err) => {
                println!(" {}", lang.tf("status.repo_failed", &[("error", &err)]));
                failed.push((name.clone(), err));
            }
        }
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    skipped.sort();
    failed.sort_by(|a, b| a.0.cmp(&b.0));
    if !keep_workdir {
        let _ = fs::remove_dir_all(&workdir);
    }

    if repos.is_empty() {
        return Err(lang.t("error.empty"));
    }

    println!("{}", lang.t("status.write_manifest"));
    let manifest = build_manifest(&args.org, &repos, &skipped, &failed);
    println!("{}", lang.t("status.write_zip"));
    let bytes = pack_bundle(&manifest, &repos)?;
    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create output dir: {e}"))?;
        }
    }
    fs::write(&args.out, &bytes).map_err(|e| format!("cannot write output: {e}"))?;
    let total_files: usize = repos.iter().map(|r| r.files.len()).sum();
    println!(
        "{}",
        lang.tf(
            "status.done",
            &[
                ("repos", &repos.len().to_string()),
                ("files", &total_files.to_string()),
                ("out", &args.out.display().to_string()),
                ("bytes", &bytes.len().to_string()),
            ]
        )
    );
    Ok(())
}

fn main() {
    let args = Args::parse();
    let lang = LangStore::load(&resolve_lang_dir(args.lang_dir.clone()), &args.lang);
    if let Err(err) = run(&args, &lang) {
        eprintln!("{}: {err}", lang.t("error.prefix"));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_fico_key_replaces_dots() {
        assert_eq!(sanitize_fico_key("TontooOS.github.io"), "TontooOS_github_io");
        assert_eq!(sanitize_fico_key("ArchiveKit"), "ArchiveKit");
        assert_eq!(sanitize_fico_key("my-repo_2"), "my-repo_2");
    }

    #[test]
    fn repo_version_prefers_commit_prefix() {
        let repo = RepoWiki {
            name: "ArchiveKit".to_string(),
            branch: "main".to_string(),
            commit: "abcdef1234567890".to_string(),
            files: Vec::new(),
        };
        assert_eq!(repo_version(&repo), "abcdef1");
        let unknown = RepoWiki { commit: "unknown".to_string(), ..repo };
        assert_eq!(repo_version(&unknown), "unknown");
    }

    #[test]
    fn manifest_lists_repos_version_and_date() {
        let repos = vec![RepoWiki {
            name: "ArchiveKit".to_string(),
            branch: "main".to_string(),
            commit: "abcdef1234567890".to_string(),
            files: vec![
                WikiFile {
                    arc_path: "ArchiveKit/Wiki/MAIN.md".to_string(),
                    data: b"# wiki".to_vec(),
                },
                WikiFile {
                    arc_path: "ArchiveKit/Examples/demo.rs".to_string(),
                    data: b"fn main() {}".to_vec(),
                },
            ],
        }];
        let doc = build_manifest("TontooOS", &repos, &["EmptyRepo".to_string()], &[]);
        assert_eq!(doc.get("wiki.org").and_then(|v| v.as_str()), Some("TontooOS"));
        assert_eq!(doc.get("wiki.repo_count").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            doc.get("repo.ArchiveKit.version").and_then(|v| v.as_str()),
            Some("abcdef1")
        );
        assert_eq!(
            doc.get("repo.ArchiveKit.wiki_files").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            doc.get("repo.ArchiveKit.example_files").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            doc.get("repo.EmptyRepo.status").and_then(|v| v.as_str()),
            Some("skipped_no_main")
        );
        // Round-trip through the FishFile parser.
        let reparsed = FishDocument::parse(&doc.to_string()).unwrap();
        assert_eq!(reparsed, doc);
    }

    #[test]
    fn bundle_is_stored_and_structured() {
        let repos = vec![RepoWiki {
            name: "ArchiveKit".to_string(),
            branch: "main".to_string(),
            commit: "abcdef1234567890".to_string(),
            files: vec![WikiFile {
                arc_path: "ArchiveKit/Wiki/MAIN.md".to_string(),
                data: b"# wiki".to_vec(),
            }],
        }];
        let manifest = build_manifest("TontooOS", &repos, &[], &[]);
        let bytes = pack_bundle(&manifest, &repos).unwrap();
        let entries = archivekit::zip_unpack(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"manifest.fico"));
        assert!(names.contains(&"ArchiveKit/Wiki/MAIN.md"));
        // 0 compression: every file entry is Stored.
        for entry in &entries {
            if !entry.is_dir() {
                assert_eq!(entry.method, archivekit::ZipMethod::Stored);
            }
        }
        let manifest_entry = entries.iter().find(|e| e.name == "manifest.fico").unwrap();
        let reparsed = FishDocument::parse(
            String::from_utf8(manifest_entry.data.clone()).unwrap().as_str(),
        )
        .unwrap();
        assert_eq!(
            reparsed.get("wiki.org").and_then(|v| v.as_str()),
            Some("TontooOS")
        );
    }

    #[test]
    fn manifest_records_failed_repos() {
        let failed = vec![("Dock".to_string(), "Dock: contents request failed (HTTP 403)".to_string())];
        let doc = build_manifest("TontooOS", &[], &[], &failed);
        assert_eq!(doc.get("wiki.failed_count").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            doc.get("repo.Dock.status").and_then(|v| v.as_str()),
            Some("failed")
        );
        assert_eq!(
            doc.get("repo.Dock.error").and_then(|v| v.as_str()),
            Some("Dock: contents request failed (HTTP 403)")
        );
        let reparsed = FishDocument::parse(&doc.to_string()).unwrap();
        assert_eq!(reparsed, doc);
    }

    #[test]
    fn retry_after_reads_rate_limit_headers() {
        let retry = HttpResponse {
            status: 429,
            headers: vec![("Retry-After".to_string(), "30".to_string())],
            body: b"rate limit".to_vec(),
            url: String::new(),
        };
        assert_eq!(retry_after_secs(&retry), Some(35));
        let reset = Date::now().timestamp() + 120;
        let limited = HttpResponse {
            status: 403,
            headers: vec![("X-RateLimit-Reset".to_string(), reset.to_string())],
            body: b"API rate limit exceeded".to_vec(),
            url: String::new(),
        };
        let wait = retry_after_secs(&limited).unwrap();
        assert!((120..=135).contains(&wait), "wait was {wait}");
        let plain = HttpResponse {
            status: 403,
            headers: Vec::new(),
            body: b"forbidden".to_vec(),
            url: String::new(),
        };
        assert_eq!(retry_after_secs(&plain), None);
    }

    #[test]
    fn load_repo_docs_reads_wiki_and_examples() {
        let root = std::env::temp_dir().join(format!("wikibuilder-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let wiki = root.join("wiki");
        fs::create_dir_all(wiki.join("sub")).unwrap();
        fs::write(wiki.join("MAIN.md"), "# wiki").unwrap();
        fs::write(wiki.join("sub").join("Page.md"), "page").unwrap();
        let examples = root.join("examples");
        fs::create_dir_all(&examples).unwrap();
        fs::write(examples.join("demo.rs"), "fn main() {}").unwrap();
        let loaded =
            load_repo_docs("Demo", "main", "abc123", &root).expect("docs should load");
        assert_eq!(loaded.name, "Demo");
        assert_eq!(loaded.branch, "main");
        assert_eq!(loaded.commit, "abc123");
        let names: Vec<&str> = loaded.files.iter().map(|f| f.arc_path.as_str()).collect();
        assert_eq!(
            names,
            vec!["Demo/Examples/demo.rs", "Demo/Wiki/MAIN.md", "Demo/Wiki/sub/Page.md"]
        );
        assert_eq!(split_counts(&loaded), (2, 1));
        // Missing MAIN.md means skip.
        fs::remove_file(wiki.join("MAIN.md")).unwrap();
        assert!(load_repo_docs("Demo", "main", "abc123", &root).is_none());
        // Missing folders mean skip.
        assert!(load_repo_docs("Demo", "main", "abc123", &root.join("nowhere")).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn lang_files_parse() {
        for lang in ["en_us", "de_de"] {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("lang")
                .join(format!("{lang}.json"));
            let text = fs::read_to_string(&path).unwrap();
            let (code, map) = JSONSerialization::parse_lang_file(&text).unwrap();
            assert_eq!(code, lang);
            assert!(map.contains_key("app.title"));
            assert!(map.contains_key("status.done"));
        }
    }
}
