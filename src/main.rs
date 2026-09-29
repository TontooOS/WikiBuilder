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
use std::time::Duration;

sdk::preinclude!();

use crate::ArchiveKit::{CompressionLevel, ZipWriter, ZipWriterOptions};
use crate::FishFile::{FishDocument, FishValue};
use crate::Foundation::date::{Date, ISO8601DateFormatter};
use crate::Foundation::serialization::{JSONSerialization, JsonValue};
use crate::NetworkKit::http::HttpRequest;

/// Current builder version, also recorded in `manifest.fico`.
pub const BUILDER_VERSION: &str = "26.1.0";
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

/// One entry of a `contents/` directory listing.
#[derive(Debug, Clone)]
struct ContentEntry {
    name: String,
    path: String,
    is_dir: bool,
    download_url: Option<String>,
}

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

fn api_get(url: &str, timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    let resp = HttpRequest::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .timeout(timeout)
        .send()
        .map_err(|e| format!("request failed for {url}: {e:?}"))?;
    Ok((resp.status, resp.bytes().to_vec()))
}

fn json_text(bytes: &[u8]) -> Result<JsonValue, String> {
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("response is not UTF-8: {e}"))?;
    JsonValue::parse(&text).map_err(|e| format!("invalid JSON: {e:?}"))
}

fn str_field(value: &JsonValue, field: &str) -> Option<String> {
    value.get(field).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// List all public repositories of `org` (follows `?page=` pagination).
fn list_repos(org: &str, timeout: Duration) -> Result<Vec<(String, String)>, String> {
    let mut repos = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("{GITHUB_API}/orgs/{org}/repos?per_page=100&page={page}");
        let (status, body) = api_get(&url, timeout)?;
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
                let branch = str_field(item, "default_branch").unwrap_or_else(|| "main".to_string());
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

/// Parse one `contents/` listing response into entries.
fn parse_content_entries(value: &JsonValue) -> Vec<ContentEntry> {
    let mut out = Vec::new();
    let items: &[JsonValue] = match value {
        JsonValue::Array(items) => items,
        _ => return out,
    };
    for item in items {
        let Some(name) = str_field(item, "name") else { continue };
        let Some(path) = str_field(item, "path") else { continue };
        let kind = str_field(item, "type").unwrap_or_default();
        out.push(ContentEntry {
            name,
            path,
            is_dir: kind == "dir",
            download_url: str_field(item, "download_url"),
        });
    }
    out
}

/// Recursively collect every file below `api_path` (`repos/{org}/{repo}/contents/...`).
fn collect_contents(
    api_path: &str,
    branch: &str,
    timeout: Duration,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let url = format!("{GITHUB_API}/{api_path}?ref={branch}");
    let (status, body) = api_get(&url, timeout)?;
    if status == 404 {
        return Err("NOT_FOUND".to_string());
    }
    if !(200..300).contains(&status) {
        return Err(format!("contents request failed (HTTP {status})"));
    }
    let root = json_text(&body)?;
    for entry in parse_content_entries(&root) {
        if entry.is_dir {
            collect_contents(
                &format!("{api_path}/{}", entry.name),
                branch,
                timeout,
                out,
            )?;
        } else if let Some(download) = entry.download_url {
            out.push((entry.path, download));
        }
    }
    Ok(())
}

/// Resolve the head commit SHA of `branch` (`unknown` when unreachable).
fn branch_head(org: &str, repo: &str, branch: &str, timeout: Duration) -> String {
    let url = format!("{GITHUB_API}/repos/{org}/{repo}/branches/{branch}");
    let Ok((status, body)) = api_get(&url, timeout) else {
        return "unknown".to_string();
    };
    if !(200..300).contains(&status) {
        return "unknown".to_string();
    }
    let Ok(root) = json_text(&body) else {
        return "unknown".to_string();
    };
    root.get("commit")
        .and_then(|c| c.get("sha"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Download the `wiki/` folder of one repo.
/// Returns `Ok(None)` when the repo has no wiki or no `wiki/MAIN.md`.
fn fetch_repo_wiki(
    org: &str,
    repo: &str,
    branch: &str,
    timeout: Duration,
) -> Result<Option<RepoWiki>, String> {
    let base = format!("repos/{org}/{repo}/contents/wiki");
    let mut raw: Vec<(String, String)> = Vec::new();
    match collect_contents(&base, branch, timeout, &mut raw) {
        Err(e) if e == "NOT_FOUND" => return Ok(None),
        Err(e) => return Err(format!("{repo}: {e}")),
        Ok(()) => {}
    }
    // Requirement: only repos whose code contains `wiki/MAIN.md` are bundled.
    let has_main = raw.iter().any(|(path, _)| path == "wiki/MAIN.md");
    if !has_main {
        return Ok(None);
    }
    let mut files = Vec::with_capacity(raw.len());
    for (path, download) in raw {
        let (status, body) = api_get(&download, timeout)?;
        if !(200..300).contains(&status) {
            return Err(format!("{repo}: download of {path} failed (HTTP {status})"));
        }
        let rel = path.strip_prefix("wiki/").unwrap_or(&path);
        files.push(WikiFile {
            arc_path: format!("{repo}/{rel}"),
            data: body,
        });
    }
    files.sort_by(|a, b| a.arc_path.cmp(&b.arc_path));
    Ok(Some(RepoWiki {
        name: repo.to_string(),
        branch: branch.to_string(),
        commit: branch_head(org, repo, branch, timeout),
        files,
    }))
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

/// Short version string: 7-char commit prefix, else the branch name.
fn repo_version(repo: &RepoWiki) -> String {
    if repo.commit.len() >= 7 {
        repo.commit[..7].to_string()
    } else {
        repo.branch.clone()
    }
}

/// Build the `manifest.fico` document: all repos, versions and build date.
fn build_manifest(org: &str, repos: &[RepoWiki], skipped: &[String]) -> FishDocument {
    let mut doc = FishDocument::new();
    doc.set("wiki.builder_version", BUILDER_VERSION);
    doc.set("wiki.build_date", ISO8601DateFormatter::string_from(&Date::now()));
    doc.set("wiki.org", org);
    doc.set("wiki.repo_count", repos.len() as i64);
    doc.set("wiki.skipped_count", skipped.len() as i64);
    doc.set("wiki.bundle", DEFAULT_OUT);
    let names: Vec<FishValue> = repos.iter().map(|r| FishValue::from(r.name.clone())).collect();
    doc.set("wiki.repos", FishValue::from(names));
    for repo in repos {
        let base = format!("repo.{}", sanitize_fico_key(&repo.name));
        doc.set(&format!("{base}.name"), repo.name.clone());
        doc.set(&format!("{base}.version"), repo_version(repo));
        doc.set(&format!("{base}.branch"), repo.branch.clone());
        doc.set(&format!("{base}.commit"), repo.commit.clone());
        doc.set(&format!("{base}.wiki_files"), repo.files.len() as i64);
        doc.set(&format!("{base}.status"), "ok");
    }
    for name in skipped {
        let base = format!("repo.{}", sanitize_fico_key(name));
        doc.set(&format!("{base}.name"), name.clone());
        doc.set(&format!("{base}.version"), "none");
        doc.set(&format!("{base}.status"), "skipped_no_main");
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
    let timeout = Duration::from_secs(args.timeout);
    println!("{}", lang.tf("status.fetch_repos", &[("org", &args.org)]));
    let listed = list_repos(&args.org, timeout)?;
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
    for (name, default_branch) in &listed {
        let branch = args.branch.as_ref().unwrap_or(default_branch);
        print!(
            "{}",
            lang.tf("status.fetch_wiki", &[("repo", name), ("branch", branch)])
        );
        match fetch_repo_wiki(&args.org, name, branch, timeout)? {
            Some(wiki) => {
                println!(
                    " {}",
                    lang.tf("status.repo_ok", &[("files", &wiki.files.len().to_string())])
                );
                repos.push(wiki);
            }
            None => {
                println!(" {}", lang.t("status.repo_skip"));
                skipped.push(name.clone());
            }
        }
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    skipped.sort();

    println!("{}", lang.t("status.write_manifest"));
    let manifest = build_manifest(&args.org, &repos, &skipped);
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
            files: vec![WikiFile {
                arc_path: "ArchiveKit/MAIN.md".to_string(),
                data: b"# wiki".to_vec(),
            }],
        }];
        let doc = build_manifest("TontooOS", &repos, &["EmptyRepo".to_string()]);
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
                arc_path: "ArchiveKit/MAIN.md".to_string(),
                data: b"# wiki".to_vec(),
            }],
        }];
        let manifest = build_manifest("TontooOS", &repos, &[]);
        let bytes = pack_bundle(&manifest, &repos).unwrap();
        let entries = crate::ArchiveKit::zip_unpack(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"manifest.fico"));
        assert!(names.contains(&"ArchiveKit/MAIN.md"));
        // 0 compression: every file entry is Stored.
        for entry in &entries {
            if !entry.is_dir() {
                assert_eq!(entry.method, crate::ArchiveKit::ZipMethod::Stored);
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
