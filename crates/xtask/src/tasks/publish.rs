//! Version-aware publishing frontend. Protected CD is the only publisher.
//! All planning is read-only; write transitions require explicit confirmation.
use crate::util;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    thread,
    time::Duration,
};

const REPO: &str = "chiploom/zed-wit";
const RELEASE_WORKFLOW: &str = "release.yml";
const PREPARATION_MARKER: &str = "TODO: Replace with reviewed release details";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}
impl Version {
    fn parse(raw: &str) -> Result<Self, String> {
        let pieces = raw.split('.').collect::<Vec<_>>();
        if pieces.len() != 3 {
            return Err(format!("expected stable X.Y.Z SemVer, found {raw:?}"));
        }
        let mut numbers = [0u64; 3];
        for (part, output) in pieces.into_iter().zip(&mut numbers) {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(format!("invalid stable SemVer component in {raw:?}"));
            }
            *output = part
                .parse()
                .map_err(|_| format!("SemVer component overflows u64: {raw:?}"))?;
        }
        Ok(Self {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
        })
    }
    fn bump(self, bump: Bump) -> Result<Self, String> {
        match bump {
            Bump::Patch => Ok(Self {
                patch: self.patch.checked_add(1).ok_or("SemVer patch overflow")?,
                ..self
            }),
            Bump::Minor => Ok(Self {
                minor: self.minor.checked_add(1).ok_or("SemVer minor overflow")?,
                patch: 0,
                ..self
            }),
            Bump::Major => Ok(Self {
                major: self.major.checked_add(1).ok_or("SemVer major overflow")?,
                minor: 0,
                patch: 0,
            }),
        }
    }
    fn value(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    Lsp,
    Extension,
}
impl Scope {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "lsp" => Ok(Self::Lsp),
            "extension" => Ok(Self::Extension),
            _ => Err(format!("invalid --scope {s:?}; expected lsp or extension")),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Lsp => "lsp",
            Self::Extension => "extension",
        }
    }
    fn tag(self, v: Version) -> String {
        match self {
            Self::Lsp => format!("v{}", v.value()),
            Self::Extension => format!("v-extension-{}", v.value()),
        }
    }
    fn manifest(self) -> &'static str {
        match self {
            Self::Lsp => "crates/wit-language-server/Cargo.toml",
            Self::Extension => "Cargo.toml",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Lsp => "WIT Language Server",
            Self::Extension => "Zed WIT extension",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bump {
    Patch,
    Minor,
    Major,
}
impl Bump {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "patch" => Ok(Self::Patch),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            _ => Err(format!(
                "invalid bump {s:?}; expected patch, minor or major"
            )),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Plan,
    Prepare,
    Submit,
    Resume,
}
#[derive(Debug)]
struct Options {
    scope: Scope,
    bump: Bump,
    operation: Operation,
    wait: bool,
    pr: Option<u64>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut scope = None;
    let mut bump = None;
    let mut operation = Operation::Plan;
    let mut explicit_op = false;
    let mut confirm = false;
    let mut wait = false;
    let mut dry_run = false;
    let mut pr = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scope" | "--bump" | "--pr" => {
                let key = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{key} needs a value"))?;
                if value.starts_with("--") {
                    return Err(format!("{key} needs a value"));
                }
                match key {
                    "--scope" if scope.is_none() => scope = Some(Scope::parse(value)?),
                    "--bump" if bump.is_none() => bump = Some(Bump::parse(value)?),
                    "--pr" if pr.is_none() => {
                        let number = value
                            .parse::<u64>()
                            .map_err(|_| "--pr requires a positive PR number".to_owned())?;
                        if number == 0 {
                            return Err("--pr must not be zero".into());
                        }
                        pr = Some(number);
                    }
                    _ => return Err(format!("duplicate {key}")),
                }
            }
            "--dry-run" if !dry_run => dry_run = true,
            "--confirm" if !confirm => confirm = true,
            "--wait" if !wait => wait = true,
            "--prepare" | "--submit" | "--resume" if !explicit_op => {
                explicit_op = true;
                operation = match args[index].as_str() {
                    "--prepare" => Operation::Prepare,
                    "--submit" => Operation::Submit,
                    _ => Operation::Resume,
                };
            }
            unknown => return Err(format!("unknown or duplicate publish option {unknown:?}")),
        }
        index += 1;
    }
    let scope = scope.ok_or("--scope lsp|extension is required")?;
    if dry_run && (operation != Operation::Plan || confirm || wait) {
        return Err("--dry-run cannot be combined with actions or confirmation".into());
    }
    if confirm && operation == Operation::Plan {
        return Err("--confirm requires --prepare, --submit or --resume".into());
    }
    if operation != Operation::Plan && !confirm {
        return Err(
            "remote/write actions require --confirm; default planning changes nothing".into(),
        );
    }
    if wait && !(operation == Operation::Resume && confirm) {
        return Err("--wait is only supported with --resume --confirm".into());
    }
    if pr.is_some() != (operation == Operation::Resume) {
        return Err("--pr is required exactly for --resume".into());
    }
    if bump.is_some() && matches!(operation, Operation::Submit | Operation::Resume) {
        return Err("--bump is only valid during planning or preparation".into());
    }
    Ok(Options {
        scope,
        bump: bump.unwrap_or(Bump::Patch),
        operation,
        wait,
        pr,
    })
}

#[derive(Debug)]
struct Plan {
    current: Version,
    candidate: Version,
    tag: String,
    branch: String,
    notes: String,
    main_sha: String,
}

fn changed_manifest_version_from_patch(patch: &str, candidate: Version)
    -> Result<Version, String>
{
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for line in patch.lines() {
        let bucket = match line.as_bytes().first() {
            Some(b'-') if !line.starts_with("---") => &mut removed,
            Some(b'+') if !line.starts_with("+++") => &mut added,
            _ => continue,
        };
        let content = line[1..].trim();
        if let Some(version) = content.strip_prefix("version = ") {
            let raw = version
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or("release manifest patch version must be quoted stable SemVer")?;
            bucket.push(Version::parse(raw)?);
        } else {
            // Extra modifications to an allowed manifest require separate review.
            return Err("release preparation changed unexpected manifest content".into());
        }
    }
    if removed.len() != 1 || added.len() != 1
        || added[0] != candidate
        || !valid_version_transition(removed[0], candidate)
    {
        return Err("reviewed PR manifest patch does not contain exactly the intended version transition".into());
    }
    Ok(removed[0])
}
fn reviewed_pr_file_versions(
    records: &str, scope: Scope, candidate: Version,
) -> Result<Version, String> {
    let mut filenames = BTreeSet::new();
    let mut primary = None;
    let mut extension_previous = None;
    for record in records.lines() {
        let row: Value = serde_json::from_str(record)
            .map_err(|_| "GitHub returned malformed PR file history".to_owned())?;
        let name = row["filename"].as_str()
            .ok_or("GitHub PR file record omitted filename")?;
        if !filenames.insert(name.to_owned()) {
            return Err("GitHub returned duplicate PR file records".into());
        }
        if name == scope.manifest() || (scope == Scope::Extension && name == "extension.toml") {
            if row["status"].as_str() != Some("modified") {
                return Err("release manifest in PR must modify a preexisting file".into());
            }
            let patch = row["patch"].as_str().filter(|s| !s.is_empty())
                .ok_or("GitHub PR manifest patch is missing; cannot prove version transition")?;
            let predecessor = changed_manifest_version_from_patch(patch, candidate)?;
            if name == scope.manifest() {
                primary = Some(predecessor);
            } else {
                extension_previous = Some(predecessor);
            }
        }
    }
    let names = filenames.into_iter().collect::<Vec<_>>().join("\n");
    validate_changed_files(&names, scope, candidate)?;
    let previous = primary.ok_or("reviewed PR lacked a versioned manifest patch")?;
    if scope == Scope::Extension && extension_previous != Some(previous) {
        return Err("reviewed extension manifests have different predecessor versions".into());
    }
    Ok(previous)
}
fn version_in_manifest(
    source: &str,
    scope: Scope,
    extension_file: bool,
) -> Result<Version, String> {
    let doc: toml::Value = toml::from_str(source)
        .map_err(|_| "release provenance manifest is not valid TOML".to_owned())?;
    let value = if extension_file {
        doc.get("version")
    } else {
        doc.get("package")
            .and_then(|package| package.get("version"))
    };
    let raw = value
        .and_then(toml::Value::as_str)
        .ok_or("release provenance manifest omitted its version")?;
    Version::parse(raw)
}
fn valid_version_transition(previous: Version, candidate: Version) -> bool {
    [Bump::Patch, Bump::Minor, Bump::Major]
        .iter()
        .any(|bump| previous.bump(*bump).is_ok_and(|next| next == candidate))
}
fn ensure_release_transition(
    previous: Version,
    reviewed_head: Version,
    merged: Version,
    current: Version,
) -> Result<(), String> {
    if !valid_version_transition(previous, reviewed_head)
        || reviewed_head != merged
        || merged != current
    {
        return Err(format!(
            "release PR did not introduce the exact reviewed version bump: before={}, PR={}, merged={}, current={}",
            previous.value(),
            reviewed_head.value(),
            merged.value(),
            current.value()
        ));
    }
    Ok(())
}
fn remote_file_at(root: &Path, sha: &str, file: &str) -> Result<String, String> {
    if !valid_sha(sha) {
        return Err("invalid immutable PR source SHA".into());
    }
    // GitHub's documented raw media type returns the contents, not JSON or a
    // base64 blob. All paths are fixed scope-specific repo paths.
    gh(
        &[
            "api",
            "-H",
            "Accept: application/vnd.github.raw+json",
            &format!("repos/{REPO}/contents/{file}?ref={sha}"),
        ],
        root,
    )
}
fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn git_manifest_version(
    root: &Path,
    sha: &str,
    scope: Scope,
    extension_file: bool,
) -> Result<Version, String> {
    if !valid_sha(sha) {
        return Err("invalid release Git commit SHA".into());
    }
    let name = if extension_file {
        "extension.toml"
    } else {
        scope.manifest()
    };
    let content = git(&["show", &format!("{sha}:{name}")], root)?;
    version_in_manifest(&content, scope, extension_file)
}
fn historical_predecessor_version(
    root: &Path,
    scope: Scope,
    merge_sha: &str,
    commit_count: u64,
    extension_file: bool,
    candidate: Version,
) -> Result<Version, String> {
    let first_parent = git(&["rev-parse", &format!("{merge_sha}^")], root)?;
    let immediate = git_manifest_version(root, &first_parent, scope, extension_file)?;
    if valid_version_transition(immediate, candidate) {
        return Ok(immediate);
    }
    if commit_count > 1 {
        let ancestor = git(&["rev-parse", &format!("{merge_sha}~{commit_count}")], root)?;
        return git_manifest_version(root, &ancestor, scope, extension_file);
    }
    Ok(immediate)
}
fn verify_pr_version_transition(
    root: &Path,
    scope: Scope,
    candidate: Version,
    reviewed_predecessor: Version,
    merge_sha: &str,
    pr_head_sha: &str,
    pr_commits: u64,
) -> Result<(), String> {
    if !valid_sha(merge_sha) || !valid_sha(pr_head_sha) || !(1..=250).contains(&pr_commits) {
        return Err("release PR provenance has invalid commit identifiers or count".into());
    }
    let current = manifest_version(root, scope)?;
    let reviewed_head = version_in_manifest(
        &remote_file_at(root, pr_head_sha, scope.manifest())?,
        scope,
        false,
    )?;
    let merged = git_manifest_version(root, merge_sha, scope, false)?;
    // Merge and squash commits introduce the entire PR diff at the immediate
    // first parent; a rebase merge introduces N sequential PR commits.
    let previous =
        historical_predecessor_version(root, scope, merge_sha, pr_commits, false, candidate)?;
    if previous != reviewed_predecessor {
        return Err("merged history predecessor does not match the actual PR version diff".into());
    }
    ensure_release_transition(previous, reviewed_head, merged, current)?;
    if scope == Scope::Extension {
        let checked_head = version_in_manifest(
            &remote_file_at(root, pr_head_sha, "extension.toml")?,
            scope,
            true,
        )?;
        let merged_extension = git_manifest_version(root, merge_sha, scope, true)?;
        let prior_extension =
            historical_predecessor_version(root, scope, merge_sha, pr_commits, true, candidate)?;
        let current_extension = version_in_manifest(
            &util::read_nonempty(&root.join("extension.toml"))?,
            scope,
            true,
        )?;
        ensure_release_transition(
            prior_extension,
            checked_head,
            merged_extension,
            current_extension,
        )?;
        if current_extension != candidate {
            return Err("extension release manifest does not match candidate".into());
        }
    }
    if current != candidate {
        return Err("current main release version does not match the merged PR".into());
    }
    Ok(())
}

fn manifest_version(root: &Path, scope: Scope) -> Result<Version, String> {
    let path = root.join(scope.manifest());
    let manifest: toml::Value = toml::from_str(&util::read_nonempty(&path)?)
        .map_err(|e| format!("parse {}: {e}", path.display()))?;
    let read = |value: &toml::Value| -> Result<Version, String> {
        let raw = value
            .get("package")
            .and_then(|v| v.get("version"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("missing package.version in {}", path.display()))?;
        Version::parse(raw)
    };
    let version = read(&manifest)?;
    if scope == Scope::Extension {
        let extension: toml::Value =
            toml::from_str(&util::read_nonempty(&root.join("extension.toml"))?)
                .map_err(|e| format!("parse extension.toml: {e}"))?;
        let other_raw = extension
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "extension.toml omitted version".to_owned())?;
        let other = Version::parse(other_raw)?;
        if version != other {
            return Err("extension manifest versions disagree".into());
        }
    }
    Ok(version)
}

// A failed read never falls back to a guessed ref. These functions do not write.
fn command(program: &str, args: &[&str], root: &Path) -> Result<String, String> {
    util::command_output(program, args, root)
}
fn gh(args: &[&str], root: &Path) -> Result<String, String> {
    command("gh", args, root).map_err(|_| {
        format!(
            "GitHub CLI request failed for {}; verify gh auth, permissions and connectivity",
            REPO
        )
    })
}
fn origin_is_expected(origin: &str) -> bool {
    matches!(
        origin.trim_end_matches('/'),
        "git@github.com:chiploom/zed-wit"
            | "git@github.com:chiploom/zed-wit.git"
            | "https://github.com/chiploom/zed-wit"
            | "https://github.com/chiploom/zed-wit.git"
            | "ssh://git@github.com/chiploom/zed-wit"
            | "ssh://git@github.com/chiploom/zed-wit.git"
    )
}
fn check_remote(root: &Path) -> Result<String, String> {
    let origin = command("git", &["remote", "get-url", "origin"], root)?;
    if !origin_is_expected(&origin) {
        return Err("origin must be the canonical chiploom/zed-wit repository; refusing ambiguous remote identity".into());
    }
    let repo: Value = serde_json::from_str(&gh(
        &[
            "repo",
            "view",
            REPO,
            "--json",
            "nameWithOwner,defaultBranchRef",
        ],
        root,
    )?)
    .map_err(|e| format!("parse gh repository metadata: {e}"))?;
    if repo["nameWithOwner"].as_str() != Some(REPO)
        || repo["defaultBranchRef"]["name"].as_str() != Some("main")
    {
        return Err("expected chiploom/zed-wit with protected default branch main".into());
    }
    let refs = command("git", &["ls-remote", "origin", "refs/heads/main"], root)?;
    let sha = refs
        .split_whitespace()
        .next()
        .filter(|x| x.len() == 40 && x.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("cannot resolve remote main SHA")?;
    Ok(sha.to_owned())
}
fn validate_worktree(root: &Path) -> Result<(), String> {
    let status = command("git", &["status", "--porcelain"], root)?;
    if !status.is_empty() {
        return Err("publish requires a clean working tree".into());
    }
    Ok(())
}
fn check_release_preconditions(root: &Path) -> Result<(), String> {
    let toolchain: toml::Value =
        toml::from_str(&util::read_nonempty(&root.join("rust-toolchain.toml"))?)
            .map_err(|e| format!("parse pinned toolchain: {e}"))?;
    if toolchain["toolchain"]["channel"].as_str() != Some("1.99.0") {
        return Err("unexpected Rust toolchain pin; review before publishing".into());
    }
    let rustc = command("rustc", &["--version"], root)?;
    if !rustc.starts_with("rustc 1.99.0 ") {
        return Err(format!(
            "publish requires pinned rustc 1.99.0; found {rustc}"
        ));
    }
    let changelog = util::read_nonempty(&root.join("CHANGELOG.md"))?;
    if !changelog.contains("# Changelog") || !changelog.contains("## Unreleased") {
        return Err("CHANGELOG.md must contain Changelog and Unreleased headings".into());
    }
    // A failed authentication check must not print GH_TOKEN or token-shaped
    // stderr. No CLI invocation below includes user-supplied shell fragments.
    gh(&["auth", "status"], root)?;
    Ok(())
}

fn release_tag_history(root: &Path) -> Result<BTreeSet<String>, String> {
    let refs = command("git", &["ls-remote", "--tags", "origin"], root)?;
    let mut tags = BTreeSet::new();
    for row in refs.lines() {
        if let Some(reference) = row.split_whitespace().nth(1)
            && let Some(name) = reference.strip_prefix("refs/tags/")
        {
            tags.insert(name.trim_end_matches("^{}").to_owned());
        }
    }
    // Include drafts and published releases, not just visible tag refs.
    // Deleted immutable releases cannot be enumerated: CD is the final gate.
    let released = gh(
        &[
            "api",
            "--paginate",
            "--jq",
            ".[].tag_name",
            "repos/chiploom/zed-wit/releases?per_page=100",
        ],
        root,
    )?;
    tags.extend(
        released
            .lines()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned),
    );
    Ok(tags)
}
fn validate_candidate(
    scope: Scope,
    candidate: Version,
    tags: &BTreeSet<String>,
) -> Result<(), String> {
    let desired = scope.tag(candidate);
    if tags.contains(&desired) {
        return Err(format!(
            "candidate tag {desired} is already allocated; never reuse it"
        ));
    }
    for tag in tags {
        let raw = match scope {
            Scope::Lsp => tag
                .strip_prefix('v')
                .filter(|s| !s.starts_with("extension-")),
            Scope::Extension => tag.strip_prefix("v-extension-"),
        };
        if let Some(raw) = raw
            && let Ok(existing) = Version::parse(raw)
            && existing >= candidate
        {
            return Err(format!(
                "candidate {desired} is not newer than known remote {tag}; choose a fresh version"
            ));
        }
    }
    Ok(())
}
fn ensure_main_branch(root: &Path) -> Result<(), String> {
    if current_branch(root)? != "main" {
        return Err(
            "publish planning requires a checked-out main branch; switch to main before preparing"
                .into(),
        );
    }
    Ok(())
}
fn plan(root: &Path, scope: Scope, bump: Bump, remote: bool) -> Result<Plan, String> {
    if remote {
        ensure_main_branch(root)?;
        check_release_preconditions(root)?;
    }
    let current = manifest_version(root, scope)?;
    let candidate = current.bump(bump)?;
    let tag = scope.tag(candidate);
    let notes = format!("docs/releases/{}/v{}.md", scope.name(), candidate.value());
    if fs::symlink_metadata(root.join(&notes)).is_ok() {
        return Err(format!("candidate release notes already exist: {notes}"));
    }
    let main_sha = if remote {
        validate_worktree(root)?;
        let remote_sha = check_remote(root)?;
        if command("git", &["rev-parse", "HEAD"], root)? != remote_sha {
            return Err(
                "publish must begin at the current remote main commit; fetch and switch to main"
                    .into(),
            );
        }
        validate_candidate(scope, candidate, &release_tag_history(root)?)?;
        remote_sha
    } else {
        String::new()
    };
    Ok(Plan {
        current,
        candidate,
        tag: tag.clone(),
        branch: format!("release-prep/{}-{tag}", scope.name()),
        notes,
        main_sha,
    })
}
fn print_plan(scope: Scope, plan: &Plan) -> Result<(), String> {
    println!("{}", serde_json::to_string_pretty(&json!({
        "phase": "planned",
        "scope": scope.name(),
        "version": plan.current.value(),
        "candidate_version": plan.candidate.value(),
        "candidate_tag": plan.tag,
        "release_branch": plan.branch,
        "default_branch_sha": plan.main_sha,
        "version_files": if scope == Scope::Lsp {
            vec!["crates/wit-language-server/Cargo.toml"]
        } else { vec!["Cargo.toml", "extension.toml"] },
        "other_preparation_files": ["Cargo.lock", "CHANGELOG.md", &plan.notes],
        "next_action": format!("cargo xtask publish --scope {} --prepare --confirm", scope.name()),
        "remote_writes": false,
        "publication": "not requested",
        "reservation_warning": "Deleted immutable release tags are not discoverable via ordinary tag/release listings; CD must independently reject reuse.",
    })).map_err(|e| format!("serialize publish plan: {e}"))?);
    Ok(())
}

pub(crate) fn publish(args: &[String]) -> Result<(), String> {
    let opts = parse_args(args)?;
    let root = util::repo_root();
    match opts.operation {
        Operation::Plan => {
            let next = plan(&root, opts.scope, opts.bump, true)?;
            print_plan(opts.scope, &next)
        }
        Operation::Prepare => prepare(&root, opts.scope, opts.bump),
        Operation::Submit => submit(&root, opts.scope),
        Operation::Resume => resume(&root, opts.scope, opts.pr.expect("validated"), opts.wait),
    }
}

// Keep manifest formatting intact: modify only the requested key in its TOML table.
fn replace_manifest_version(
    original: &str,
    previous: Version,
    next: Version,
    table: Option<&str>,
) -> Result<String, String> {
    let mut active = table.is_none();
    let mut replaced = false;
    let mut output = String::with_capacity(original.len() + 10);
    let old = format!("version = \"{}\"", previous.value());
    let new = format!("version = \"{}\"", next.value());
    for line in original.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            active = table.is_some_and(|expected| trimmed == expected);
        }
        if active && trimmed.starts_with("version = ") {
            if replaced || !trimmed.starts_with(&old) || trimmed != old {
                return Err("unexpected or duplicated manifest version field".into());
            }
            output.push_str(&line.replacen(&old, &new, 1));
            replaced = true;
        } else {
            output.push_str(line);
        }
    }
    if !replaced {
        return Err("manifest version field missing".into());
    }
    Ok(output)
}
fn checked_release_path(root: &Path, file: &Path, existing_file: bool) -> Result<(), String> {
    let relative = file
        .strip_prefix(root)
        .map_err(|_| "release preparation path escapes repository root".to_owned())?;
    let mut inspected = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err("release preparation path cannot be repository root".into());
    }
    for (position, component) in components.iter().enumerate() {
        match component {
            std::path::Component::Normal(value) => inspected.push(value),
            _ => return Err("release preparation path contains traversal".into()),
        }
        let last = position == components.len() - 1;
        match fs::symlink_metadata(&inspected) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "release preparation refuses symlink: {}",
                        inspected.display()
                    ));
                }
                if last {
                    if !existing_file || !metadata.is_file() {
                        return Err(format!(
                            "release file has unexpected type or exists: {}",
                            inspected.display()
                        ));
                    }
                } else if !metadata.is_dir() {
                    return Err(format!(
                        "release parent is not directory: {}",
                        inspected.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if existing_file {
                    return Err(format!(
                        "required release path missing: {}",
                        inspected.display()
                    ));
                }
            }
            Err(error) => return Err(format!("inspect {}: {error}", inspected.display())),
        }
    }
    Ok(())
}
fn check_prepare_paths(root: &Path, scope: Scope, notes: &str) -> Result<(), String> {
    for file in ["CHANGELOG.md", "Cargo.lock", scope.manifest()] {
        checked_release_path(root, &root.join(file), true)?;
    }
    if scope == Scope::Extension {
        checked_release_path(root, &root.join("extension.toml"), true)?;
    }
    checked_release_path(root, &root.join(notes), false)
}

fn update_manifest(
    path: &Path,
    previous: Version,
    next: Version,
    table: Option<&str>,
) -> Result<(), String> {
    let original = util::read_nonempty(path)?;
    let updated = replace_manifest_version(&original, previous, next, table)?;
    fs::write(path, updated).map_err(|e| format!("update {}: {e}", path.display()))
}
fn git(args: &[&str], root: &Path) -> Result<String, String> {
    command("git", args, root)
}
fn current_branch(root: &Path) -> Result<String, String> {
    git(&["symbolic-ref", "--quiet", "--short", "HEAD"], root)
}
fn confirm_remote_main(root: &Path) -> Result<String, String> {
    validate_worktree(root)?;
    let remote_sha = check_remote(root)?;
    if current_branch(root)? != "main" {
        return Err(
            "publish must use a checked-out main branch; no detached or feature branch".into(),
        );
    }
    if git(&["rev-parse", "HEAD"], root)? != remote_sha {
        return Err("main differs from latest origin/main; synchronize before publishing".into());
    }
    Ok(remote_sha)
}
fn prepare(root: &Path, scope: Scope, bump: Bump) -> Result<(), String> {
    let plan = plan(root, scope, bump, true)?;
    confirm_remote_main(root)?;
    // Avoid overwriting or resurrecting a local or remote preparation branch.
    if git(
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/{}", plan.branch),
        ],
        root,
    )
    .is_ok()
    {
        return Err(format!(
            "local release-preparation branch already exists: {}",
            plan.branch
        ));
    }
    if !git(&["ls-remote", "--heads", "origin", &plan.branch], root)?.is_empty() {
        return Err(format!(
            "remote release-preparation branch already exists: {}",
            plan.branch
        ));
    }
    check_prepare_paths(root, scope, &plan.notes)?;
    if !util::read_nonempty(&root.join("CHANGELOG.md"))?.contains("## Unreleased\n") {
        return Err("CHANGELOG.md has no Unreleased heading; no preparation branch created".into());
    }
    print_plan(scope, &plan)?;
    git(&["switch", "-c", &plan.branch], root)?;
    apply_local_release_files(root, scope, &plan)?;
    println!(
        "Prepared {} at {}. Edit release notes and changelog, review changes, run tests, and commit locally.",
        plan.tag, plan.branch
    );
    println!("No branch was pushed and no pull request or release was created.");
    println!(
        "After reviewing and committing: cargo xtask publish --scope {} --submit --confirm",
        scope.name()
    );
    Ok(())
}

fn apply_local_release_files(root: &Path, scope: Scope, plan: &Plan) -> Result<(), String> {
    let paths = if scope == Scope::Lsp {
        vec![(root.join(scope.manifest()), Some("[package]"))]
    } else {
        vec![
            (root.join("Cargo.toml"), Some("[package]")),
            (root.join("extension.toml"), None),
        ]
    };
    for (path, table) in paths {
        update_manifest(&path, plan.current, plan.candidate, table)?;
    }

    let notes = root.join(&plan.notes);
    if let Some(parent) = notes.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create release notes directory: {e}"))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&notes)
        .map_err(|e| format!("create {}: {e}", notes.display()))?;
    writeln!(
        file,
        "# {} {}\n\n{}\n",
        scope.title(),
        plan.candidate.value(),
        PREPARATION_MARKER
    )
    .map_err(|e| format!("write release notes: {e}"))?;
    let changelog = root.join("CHANGELOG.md");
    let body = util::read_nonempty(&changelog)?;
    if !body.contains("## Unreleased\n") {
        return Err("CHANGELOG.md has no Unreleased heading".into());
    }
    let update = format!(
        "## Unreleased\n\n### {} ({})\n\n- {}\n",
        plan.tag,
        scope.name(),
        PREPARATION_MARKER
    );
    fs::write(&changelog, body.replacen("## Unreleased\n", &update, 1))
        .map_err(|e| format!("update changelog: {e}"))?;

    // Cargo updates the workspace package version in Cargo.lock. This is
    // deliberately the non-locked invocation; submit later verifies --locked.
    command("cargo", &["check", "--workspace", "--offline"], root)?;
    Ok(())
}

fn notes_reviewed(root: &Path, scope: Scope, version: Version) -> Result<(), String> {
    let notes = root.join(format!(
        "docs/releases/{}/v{}.md",
        scope.name(),
        version.value()
    ));
    checked_release_path(root, &notes, true)?;
    checked_release_path(root, &root.join("CHANGELOG.md"), true)?;
    let text = util::read_nonempty(&notes)?;
    if text.contains(PREPARATION_MARKER) || text.contains("TODO") || text.trim().len() < 100 {
        return Err(format!(
            "release notes need substantive human review: {}",
            notes.display()
        ));
    }
    let changelog = util::read_nonempty(&root.join("CHANGELOG.md"))?;
    if !changelog.contains(&format!("### {} ({})", scope.tag(version), scope.name()))
        || changelog.contains(PREPARATION_MARKER)
    {
        return Err("changelog must contain reviewed scoped release entry".into());
    }
    Ok(())
}
fn validate_changed_files(paths: &str, scope: Scope, version: Version) -> Result<(), String> {
    let mut allowed = BTreeSet::from([
        "Cargo.lock".to_owned(),
        "CHANGELOG.md".to_owned(),
        scope.manifest().to_owned(),
        format!("docs/releases/{}/v{}.md", scope.name(), version.value()),
    ]);
    if scope == Scope::Extension {
        allowed.insert("extension.toml".into());
    }
    let changed = paths.lines().collect::<BTreeSet<_>>();
    for file in &changed {
        if !allowed.contains(*file) {
            return Err(format!(
                "release preparation includes unexpected change: {file}"
            ));
        }
    }
    // A release-preparation PR must contain the actual version bump, not just
    // a correctly named branch with notes. Keep lockfile and changelog
    // changes part of the reviewed commit contract.
    for required in &allowed {
        if !changed.contains(required.as_str()) {
            return Err(format!(
                "release preparation is missing required change: {required}"
            ));
        }
    }
    Ok(())
}
fn submit(root: &Path, scope: Scope) -> Result<(), String> {
    validate_worktree(root)?;
    let version = manifest_version(root, scope)?;
    let tag = scope.tag(version);
    let branch = format!("release-prep/{}-{tag}", scope.name());
    if current_branch(root)? != branch {
        return Err(format!("expected release-preparation branch {branch}"));
    }
    let remote_sha = check_remote(root)?;
    // The remote main revision must be known and be an ancestor. Do not
    // silently rebase a candidate whose reviewed contents might change.
    git(&["merge-base", "--is-ancestor", &remote_sha, "HEAD"], root).map_err(|_| {
        "release branch is stale or origin/main was not fetched; update deliberately".to_owned()
    })?;
    let delta = git(&["diff", "--name-only", &remote_sha, "HEAD"], root)?;
    validate_changed_files(&delta, scope, version)?;
    if delta.is_empty() {
        return Err("release preparation branch has no changes".into());
    }
    for (file, extension_file) in [
        (scope.manifest(), false),
        ("extension.toml", true),
    ] {
        if extension_file && scope != Scope::Extension {
            continue;
        }
        let patch = git(&[
            "diff", "--no-ext-diff", "--unified=0", &remote_sha, "HEAD",
            "--", file,
        ], root)?;
        let patched_predecessor = changed_manifest_version_from_patch(&patch, version)?;
        let actual_predecessor = git_manifest_version(
            root, &remote_sha, scope, extension_file,
        )?;
        if patched_predecessor != actual_predecessor {
            return Err(format!(
                "release manifest version diff does not match remote main: {file}"
            ));
        }
    }
    notes_reviewed(root, scope, version)?;
    command(
        "cargo",
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
        root,
    )?;
    crate::tasks::release_ops::release_check_inner(scope.name(), &tag)?;
    crate::tasks::validation::verify(&[])?;

    validate_candidate(scope, version, &release_tag_history(root)?)?;
    let existing = gh(
        &[
            "pr",
            "list",
            "-R",
            REPO,
            "--state",
            "all",
            "--head",
            &branch,
            "--json",
            "number,headRefName,baseRefName",
        ],
        root,
    )?;
    let prs: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse existing preparation PRs: {e}"))?;
    if !prs.as_array().is_some_and(Vec::is_empty) {
        return Err(
            "a release preparation PR already exists; review or resume it rather than duplicate"
                .into(),
        );
    }
    if !git(&["ls-remote", "--heads", "origin", &branch], root)?.is_empty() {
        return Err(
            "release branch is already on origin; inspect and resume its PR manually".into(),
        );
    }
    // These two remote writes are allowed only after explicit --submit --confirm.
    git(
        &[
            "push",
            "--set-upstream",
            "origin",
            &format!("HEAD:refs/heads/{branch}"),
        ],
        root,
    )?;
    let title = format!("Prepare {} release {}", scope.title(), tag);
    let body = format!(
        "Release preparation for {}.\n\n- Scope: {}\n- Candidate: {}\n- Human-reviewed notes: docs/releases/{}/v{}.md\n\n**No tag or GitHub Release is published by this PR.** Merge through protected review, then resume with cargo xtask publish --scope {} --resume --pr <number> --confirm.",
        tag,
        scope.name(),
        tag,
        scope.name(),
        version.value(),
        scope.name()
    );
    let created = gh(
        &[
            "pr", "create", "-R", REPO, "--base", "main", "--head", &branch, "--title", &title,
            "--body", &body,
        ],
        root,
    )?;
    println!("Release preparation PR submitted: {}", created.trim());
    println!("No publication requested. Merge through normal review and CD policy.");
    Ok(())
}

fn protected_dispatch_args(scope: Scope, tag: &str, main_sha: &str) -> Vec<String> {
    vec![
        "api".into(),
        "-X".into(),
        "POST".into(),
        "-H".into(),
        "X-GitHub-Api-Version: 2026-03-10".into(),
        format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/dispatches"),
        "-F".into(),
        "return_run_details=true".into(),
        "-f".into(),
        "ref=main".into(),
        "-f".into(),
        format!("inputs[expected_sha]={main_sha}"),
        "-f".into(),
        "inputs[operation]=publish".into(),
        "-f".into(),
        format!("inputs[scope]={}", scope.name()),
        "-f".into(),
        format!("inputs[tag]={tag}"),
    ]
}
fn verify_merged_preparation_pr(
    info: &Value, scope: Scope, version: Version,
) -> Result<(String, String, u64), String> {
    let branch = format!("release-prep/{}-{}", scope.name(), scope.tag(version));
    if info["merged"].as_bool() != Some(true)
        || info["base"]["ref"].as_str() != Some("main")
        || info["head"]["ref"].as_str() != Some(branch.as_str())
        || info["head"]["repo"]["full_name"].as_str() != Some(REPO)
    {
        return Err("release preparation PR is not merged on main for this exact tag".into());
    }
    let merge_sha = info["merge_commit_sha"].as_str()
        .filter(|sha| valid_sha(sha))
        .ok_or("merged preparation PR omitted valid merge commit SHA")?;
    let head_sha = info["head"]["sha"].as_str()
        .filter(|sha| valid_sha(sha))
        .ok_or("release PR omitted valid reviewed head SHA")?;
    let commits = info["commits"].as_u64()
        .filter(|count| (1..=250).contains(count))
        .ok_or("release PR omitted a valid commit count")?;
    Ok((merge_sha.to_owned(), head_sha.to_owned(), commits))
}
fn parse_dispatch_identity(response: &str) -> Result<(u64, String), String> {
    let result: Value = serde_json::from_str(response).map_err(|_| {
        "CD dispatch returned no usable run identity; inspect Actions before retrying".to_owned()
    })?;
    let id = result["workflow_run_id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or("CD dispatch omitted workflow_run_id; inspect GitHub Actions before retrying")?;
    let url = result["html_url"]
        .as_str()
        .ok_or("CD dispatch omitted run URL")?;
    let expected = format!("https://github.com/{REPO}/actions/runs/{id}");
    if url != expected {
        return Err("CD dispatch returned an unexpected or mismatched run URL".into());
    }
    Ok((id, url.to_owned()))
}

fn parse_remote_tag_commit(rows: &str, tag: &str) -> Result<String, String> {
    let exact = format!("refs/tags/{tag}");
    let peeled = format!("{exact}^{{}}");
    let mut direct = None;
    let mut commit = None;
    for line in rows.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 || !valid_sha(fields[0]) {
            return Err("tag lookup returned an invalid object reference".into());
        }
        match fields[1] {
            reference if reference == exact => {
                if direct.replace(fields[0]).is_some() {
                    return Err("duplicate tag object reference".into());
                }
            }
            reference if reference == peeled => {
                if commit.replace(fields[0]).is_some() {
                    return Err("duplicate peeled tag reference".into());
                }
            }
            _ => return Err("tag lookup returned an unrelated reference".into()),
        }
    }
    if direct.is_none() {
        return Err("resumable draft lacks the expected Git tag".into());
    }
    // A lightweight tag has only a direct ref; annotated tags also have a
    // peeled ^{} entry. As in CD, the commit (not the annotated tag object)
    // is the relevant release identity.
    Ok(commit
        .or(direct)
        .expect("direct tag reference validated")
        .to_owned())
}

fn validate_prior_cd_runs(
    runs: &str,
    main_sha: &str,
    scope: Scope,
    tag: &str,
    matching_draft: bool,
) -> Result<(), String> {
    let candidate_validate = format!("CD / validate / {} / {tag}", scope.name());
    let candidate_publish = format!("CD / publish / {} / {tag}", scope.name());
    let mut ids = BTreeSet::new();
    let mut known_failed_publish = false;
    for row in runs.lines() {
        let columns = row.split('\t').collect::<Vec<_>>();
        let [id, sha, event, status, conclusion, title] = <[&str; 6]>::try_from(columns.as_slice())
            .map_err(|_| "CD run history has malformed records; inspect manually")?;
        let id = id
            .parse::<u64>()
            .map_err(|_| "CD run history has an invalid run ID")?;
        if id == 0 || !ids.insert(id) {
            return Err("CD run history contains missing or duplicate run IDs".into());
        }
        if sha != main_sha || event != "workflow_dispatch" {
            continue;
        }
        if status != "completed" {
            return Err(format!(
                "active or ambiguous CD dispatch {id} on validated main SHA"
            ));
        }
        if title == candidate_validate {
            // A completed dry run does not reserve a release tag, regardless
            // of whether its validation succeeded.
            if conclusion == "unknown" {
                return Err("validation CD run has unknown conclusion".into());
            }
            continue;
        }
        if title == candidate_publish {
            match conclusion {
                "failure" | "cancelled" | "timed_out" => {
                    known_failed_publish = true;
                }
                "success" => {
                    return Err(format!(
                        "CD run {id} reports publication success; do not redispatch even if a draft exists"
                    ));
                }
                _ => {
                    return Err(format!(
                        "CD run {id} conclusion {conclusion:?} is not a verified retryable failure"
                    ));
                }
            }
            continue;
        }
        let known_other_release = title
            .strip_prefix("CD / validate / ")
            .or_else(|| title.strip_prefix("CD / publish / "))
            .is_some_and(|suffix| {
                suffix.starts_with("lsp / v") || suffix.starts_with("extension / v-extension-")
            });
        if !known_other_release {
            return Err(format!(
                "CD run {id} does not identify a known operation and release scope"
            ));
        }
    }
    if matching_draft && !known_failed_publish {
        return Err(
            "unpublished release draft has no verified failed, cancelled or timed-out CD run; inspect recovery manually".into()
        );
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum WorkflowProgress {
    Pending,
    Succeeded,
}
fn classify_workflow_run(
    run: &Value,
    id: u64,
    main_sha: &str,
    scope: Scope,
    tag: &str,
) -> Result<WorkflowProgress, String> {
    let title = format!("CD / publish / {} / {tag}", scope.name());
    if run["id"].as_u64() != Some(id)
        || run["head_sha"].as_str() != Some(main_sha)
        || run["head_branch"].as_str() != Some("main")
        || run["event"].as_str() != Some("workflow_dispatch")
        || !matches!(
            run["path"].as_str(),
            Some(".github/workflows/release.yml" | ".github/workflows/release.yml@main")
        )
        || run["display_title"].as_str() != Some(title.as_str())
    {
        return Err("exact CD run identity differs from confirmed dispatch".into());
    }
    match run["status"].as_str() {
        Some("queued" | "in_progress" | "requested" | "waiting" | "pending") => {
            Ok(WorkflowProgress::Pending)
        }
        Some("completed") if run["conclusion"].as_str() == Some("success") => {
            Ok(WorkflowProgress::Succeeded)
        }
        Some("completed") => Err(format!(
            "protected CD run {id} completed with {:?}, not published",
            run["conclusion"].as_str()
        )),
        _ => Err(format!(
            "CD run {id} returned an unrecognized workflow status"
        )),
    }
}
fn verify_published_release(release: &Value, tag: &str) -> Result<(), String> {
    if release["draft"].as_bool() == Some(false)
        && release["tag_name"].as_str() == Some(tag)
        && release["immutable"].as_bool() == Some(true)
    {
        Ok(())
    } else {
        Err(
            "CD run completed, but the expected non-draft immutable release was not verified"
                .into(),
        )
    }
}

fn resume(root: &Path, scope: Scope, pr: u64, wait: bool) -> Result<(), String> {
    check_release_preconditions(root)?;
    let main_sha = confirm_remote_main(root)?;
    let version = manifest_version(root, scope)?;
    let tag = scope.tag(version);
    notes_reviewed(root, scope, version)?;
    crate::tasks::release_ops::release_check_inner(scope.name(), &tag)?;
    // A filename-only Cargo.lock change is not proof of consistency. Cargo
    // must accept the complete current workspace without regenerating it.
    command(
        "cargo",
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
        root,
    )?;

    let info: Value =
        serde_json::from_str(&gh(&["api", &format!("repos/{REPO}/pulls/{pr}")], root)?)
            .map_err(|e| format!("parse preparation PR: {e}"))?;
    let (merge_sha, head_sha, pr_commits) =
        verify_merged_preparation_pr(&info, scope, version)?;

    let reviewed_files = gh(
        &[
            "api",
            "--paginate",
            "--jq",
            ".[] | {filename: .filename, status: .status, patch: .patch} | @json",
            &format!("repos/{REPO}/pulls/{pr}/files?per_page=100"),
        ],
        root,
    )?;
    let expected_file_count = info["changed_files"].as_u64()
        .ok_or("merged release PR omitted its changed file count")?;
    let downloaded_file_count = reviewed_files.lines().count() as u64;
    if expected_file_count != downloaded_file_count {
        return Err(format!(
            "incomplete GitHub PR file listing: expected {expected_file_count}, received {downloaded_file_count}"
        ));
    }
    let reviewed_predecessor = reviewed_pr_file_versions(&reviewed_files, scope, version)?;
    git(&["merge-base", "--is-ancestor", &merge_sha, &main_sha], root)
        .map_err(|_| "merged preparation commit is not in current main".to_owned())?;

    verify_pr_version_transition(
        root, scope, version, reviewed_predecessor, &merge_sha, &head_sha, pr_commits,
    )?;

    let known = release_tag_history(root)?;
    let mut matching_draft = false;
    if known.contains(&tag) {
        // The protected CD workflow can resume only an unpublished draft bound
        // to the exact validated release revision. A tag with no draft, or a
        // published release, is never eligible for local retries.
        let releases = gh(
            &[
                "api",
                "--paginate",
                "--jq",
                &format!(".[] | select(.tag_name == \"{tag}\") | [.draft, .tag_name] | @tsv"),
                &format!("repos/{REPO}/releases?per_page=100"),
            ],
            root,
        )?;
        let draft_rows = releases.lines().collect::<Vec<_>>();
        if draft_rows.len() != 1 || draft_rows[0] != format!("true\t{tag}") {
            return Err(format!(
                "tag {tag} has no unambiguous unpublished draft; manual review required"
            ));
        }
        let remote_tag = git(
            &[
                "ls-remote",
                "--tags",
                "origin",
                &format!("refs/tags/{tag}"),
                &format!("refs/tags/{tag}^{{}}"),
            ],
            root,
        )?;
        let tagged_sha = parse_remote_tag_commit(&remote_tag, &tag)?;
        if tagged_sha != main_sha {
            return Err(
                "resumable draft Git tag does not match the exact validated main SHA".into(),
            );
        }
        matching_draft = true;
    }
    // Require the configured workflow to be enabled; do not create an alternate.
    let workflow: Value = serde_json::from_str(&gh(
        &[
            "api",
            &format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}"),
        ],
        root,
    )?)
    .map_err(|e| format!("parse protected CD workflow: {e}"))?;
    if workflow["state"].as_str() != Some("active") {
        return Err("protected CD workflow is not active".into());
    }

    // Conservatively refuse an uncorrelatable repeated dispatch from the same
    // protected source commit. The user must inspect previous runs manually.
    let runs = gh(
        &[
            "api",
            "--paginate",
            "--jq",
            ".workflow_runs[] | [.id, .head_sha, .event, .status, (.conclusion // \"unknown\"), .display_title] | @tsv",
            &format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/runs?per_page=100"),
        ],
        root,
    )?;
    validate_prior_cd_runs(&runs, &main_sha, scope, &tag, matching_draft)?;

    // GitHub's 2026 workflow_dispatch response gives the exact run ID.
    // Never fall back to scanning the latest run after an ambiguous dispatch.
    let dispatch = protected_dispatch_args(scope, &tag, &main_sha);
    let dispatch_refs = dispatch.iter().map(String::as_str).collect::<Vec<_>>();
    let response = gh(&dispatch_refs, root)
    .map_err(|_| {
        "CD dispatch may have been accepted; check GitHub Actions before retrying".to_owned()
    })?;
    let (id, run_url) = parse_dispatch_identity(&response)?;
    println!("Protected CD dispatch requested for {tag}: {run_url} (run {id}).");
    if !wait {
        println!("Status: requested/pending. This is not a published release.");
        return Ok(());
    }
    for _ in 0..180 {
        let run: Value = serde_json::from_str(&gh(
            &["api", &format!("repos/{REPO}/actions/runs/{id}")],
            root,
        )?)
        .map_err(|e| format!("parse exact workflow run {id}: {e}"))?;
        match classify_workflow_run(&run, id, &main_sha, scope, &tag)? {
            WorkflowProgress::Pending => {}
            WorkflowProgress::Succeeded => {
                let release: Value = serde_json::from_str(&gh(
                    &["api", &format!("repos/{REPO}/releases/tags/{tag}")],
                    root,
                )?)
                .map_err(|e| format!("parse published release after CD success: {e}"))?;
                verify_published_release(&release, &tag)?;
                println!("Published immutable release {tag}: {run_url}");
                return Ok(());
            }
        }
        thread::sleep(Duration::from_secs(10));
    }
    Err(format!(
        "timed out watching CD run {id}; outcome pending: {run_url}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn strict_stable_semver_and_checked_bumps() {
        let zero = Version::parse("0.1.2").unwrap();
        assert_eq!(zero.bump(Bump::Patch).unwrap().value(), "0.1.3");
        assert_eq!(zero.bump(Bump::Minor).unwrap().value(), "0.2.0");
        assert_eq!(zero.bump(Bump::Major).unwrap().value(), "1.0.0");
        let higher = Version::parse("12.34.56").unwrap();
        assert_eq!(higher.bump(Bump::Minor).unwrap().value(), "12.35.0");
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.0.0",
            "0.01.0",
            "0.0.01",
            "1.2.3-rc.1",
            "1.2.3+meta",
            "-1.2.3",
            "1.2.a",
            "18446744073709551616.1.1",
        ] {
            assert!(Version::parse(bad).is_err(), "accepted {bad:?}");
        }
        assert!(
            Version::parse("0.0.18446744073709551615")
                .unwrap()
                .bump(Bump::Patch)
                .is_err()
        );
        assert!(
            Version::parse("0.18446744073709551615.1")
                .unwrap()
                .bump(Bump::Minor)
                .is_err()
        );
        assert!(
            Version::parse("18446744073709551615.0.0")
                .unwrap()
                .bump(Bump::Major)
                .is_err()
        );
    }

    #[test]
    fn scope_separation_and_historic_collisions() {
        let version = Version::parse("0.1.3").unwrap();
        assert_eq!(Scope::Lsp.tag(version), "v0.1.3");
        assert_eq!(Scope::Extension.tag(version), "v-extension-0.1.3");
        assert_ne!(Scope::Lsp.manifest(), Scope::Extension.manifest());
        let mut visible = BTreeSet::from(["v0.1.1".into(), "v-extension-0.1.2".into()]);
        assert!(validate_candidate(Scope::Lsp, version, &visible).is_ok());
        assert!(validate_candidate(Scope::Extension, version, &visible).is_ok());
        visible.insert("v0.1.3".into());
        assert!(validate_candidate(Scope::Lsp, version, &visible).is_err());
        assert!(validate_candidate(Scope::Extension, version, &visible).is_ok());
        visible.insert("v-extension-0.2.0".into());
        assert!(validate_candidate(Scope::Extension, version, &visible).is_err());
    }

    #[test]
    fn parse_requires_explicit_confirm_and_rejects_ambiguous_stages() {
        let planned = parse_args(&args(&["--scope", "lsp"])).unwrap();
        assert_eq!(planned.operation, Operation::Plan);
        assert_eq!(planned.bump, Bump::Patch);
        assert_eq!(
            parse_args(&args(&["--scope", "extension", "--bump", "minor"]))
                .unwrap()
                .bump,
            Bump::Minor
        );
        let approved = parse_args(&args(&[
            "--scope",
            "lsp",
            "--resume",
            "--pr",
            "25",
            "--confirm",
            "--wait",
        ]))
        .unwrap();
        assert_eq!(approved.operation, Operation::Resume);
        assert_eq!(approved.pr, Some(25));
        assert!(approved.wait);
        for invalid in [
            vec!["--scope", "lsp", "--confirm"],
            vec!["--scope", "lsp", "--prepare"],
            vec!["--scope", "lsp", "--submit"],
            vec!["--scope", "lsp", "--resume", "--pr", "3"],
            vec!["--scope", "lsp", "--wait"],
            vec!["--scope", "lsp", "--dry-run", "--confirm"],
            vec!["--scope", "lsp", "--prepare", "--confirm", "--dry-run"],
            vec!["--scope", "lsp", "--resume", "--confirm", "--pr", "0"],
            vec!["--scope", "lsp", "--resume", "--confirm"],
            vec!["--scope", "lsp", "--prepare", "--confirm", "--pr", "3"],
            vec![
                "--scope",
                "lsp",
                "--resume",
                "--confirm",
                "--pr",
                "3",
                "--bump",
                "major",
            ],
            vec!["--scope", "lsp", "--scope", "extension"],
            vec![
                "--scope",
                "lsp",
                "--resume",
                "--submit",
                "--confirm",
                "--pr",
                "3",
            ],
            vec!["--scope", "lsp", "--unknown"],
        ] {
            assert!(parse_args(&args(&invalid)).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn version_edits_are_scope_bounded_and_preserve_other_fields() {
        let old = Version::parse("0.1.2").unwrap();
        let next = Version::parse("0.1.3").unwrap();
        let input = "[package]\nname = \"server\"\nversion = \"0.1.2\"\n\n[dependencies]\nversion = \"99.0.0\"\n";
        let changed = replace_manifest_version(input, old, next, Some("[package]")).unwrap();
        assert!(changed.contains("version = \"0.1.3\""));
        assert!(changed.contains("[dependencies]\nversion = \"99.0.0\""));
        let extension =
            "id = \"wit\"\nversion = \"0.1.2\"\n\n[grammars.wit]\nversion = \"other\"\n";
        let edited = replace_manifest_version(extension, old, next, None).unwrap();
        assert!(edited.starts_with("id = \"wit\"\nversion = \"0.1.3\""));
        assert!(edited.contains("[grammars.wit]\nversion = \"other\""));
        assert!(replace_manifest_version(input, next, old, Some("[package]")).is_err());
    }

    #[test]
    fn only_canonical_git_remotes_are_authorized() {
        for valid in [
            "git@github.com:chiploom/zed-wit.git",
            "https://github.com/chiploom/zed-wit",
            "https://github.com/chiploom/zed-wit.git",
        ] {
            assert!(origin_is_expected(valid));
        }
        for invalid in [
            "https://github.com/attacker/zed-wit",
            "https://evil.example/chiploom/zed-wit",
            "https://token@github.com/chiploom/zed-wit",
            "git@github.com:chiploom/zed-wit-other",
        ] {
            assert!(!origin_is_expected(invalid));
        }
    }

    #[test]
    fn historical_version_transition_handles_merge_squash_and_rebase_layouts() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-version-history-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create temporary Git fixture: {e}"),
            }
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove temporary Git history");
            }
        }
        let _cleanup = Cleanup(root.clone());
        git(&["init", "-q", "-b", "main"], &root).unwrap();
        fs::create_dir_all(root.join("crates/wit-language-server")).unwrap();
        let manifest = root.join(Scope::Lsp.manifest());
        let write_version = |version: &str| {
            fs::write(
                &manifest,
                format!("[package]\nname = \"fixture\"\nversion = \"{version}\"\n"),
            )
            .unwrap()
        };
        let commit = |label: &str| {
            git(&["add", "."], &root).unwrap();
            command(
                "git",
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "-qm",
                    label,
                ],
                &root,
            )
            .unwrap();
            git(&["rev-parse", "HEAD"], &root).unwrap()
        };
        write_version("0.1.2");
        let origin = commit("base");
        let candidate = Version::parse("0.1.3").unwrap();

        // Squash: one new commit on main regardless of the number of
        // original commits from the feature branch.
        write_version("0.1.3");
        let squash = commit("squash");
        assert_eq!(
            historical_predecessor_version(&root, Scope::Lsp, &squash, 3, false, candidate,)
                .unwrap(),
            Version::parse("0.1.2").unwrap()
        );

        git(&["reset", "--hard", &origin], &root).unwrap();
        // Rebase: the version change can be in an earlier rebased commit.
        write_version("0.1.3");
        let _ = commit("first rebased");
        fs::write(root.join("notes.txt"), "reviewed notes").unwrap();
        let last_rebase = commit("second rebased");
        assert_eq!(
            historical_predecessor_version(&root, Scope::Lsp, &last_rebase, 2, false, candidate,)
                .unwrap(),
            Version::parse("0.1.2").unwrap()
        );

        git(&["reset", "--hard", &origin], &root).unwrap();
        git(&["switch", "-q", "-c", "release-fixture"], &root).unwrap();
        write_version("0.1.3");
        let _ = commit("feature version");
        git(&["switch", "-q", "main"], &root).unwrap();
        command(
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "merge",
                "--no-ff",
                "-qm",
                "merge PR",
                "release-fixture",
            ],
            &root,
        )
        .unwrap();
        let merged = git(&["rev-parse", "HEAD"], &root).unwrap();
        assert_eq!(
            historical_predecessor_version(&root, Scope::Lsp, &merged, 1, false, candidate,)
                .unwrap(),
            Version::parse("0.1.2").unwrap()
        );
    }

    #[test]
    fn release_paths_reject_filesystem_escapes_and_existing_notes() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create release file fixture: {e}"),
            }
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove release file fixture");
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir_all(root.join("docs/releases")).unwrap();
        fs::write(root.join("CHANGELOG.md"), "notes").unwrap();
        assert!(checked_release_path(&root, &root.join("CHANGELOG.md"), true).is_ok());
        assert!(
            checked_release_path(
                &root,
                &root.join("docs/releases/extension/v0.1.1.md"),
                false
            )
            .is_ok()
        );
        assert!(checked_release_path(&root, &root.join("CHANGELOG.md"), false).is_err());
        assert!(checked_release_path(&root, &root.join("docs/../CHANGELOG.md"), true).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root, root.join("docs/releases/escape")).unwrap();
            assert!(
                checked_release_path(&root, &root.join("docs/releases/escape/publish.md"), false)
                    .is_err()
            );
        }
    }

    #[test]
    fn real_lightweight_and_annotated_refs_peel_to_same_commit() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-tags-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create temporary Git fixture: {e}"),
            }
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove temporary Git tag fixture");
            }
        }
        let _cleanup = Cleanup(root.clone());
        git(&["init", "-q", "-b", "main"], &root).unwrap();
        command(
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "base",
            ],
            &root,
        )
        .unwrap();
        let commit = git(&["rev-parse", "HEAD"], &root).unwrap();
        git(&["tag", "v0.1.3"], &root).unwrap();
        command(
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "tag",
                "-a",
                "v0.1.4",
                "-m",
                "annotated",
            ],
            &root,
        )
        .unwrap();
        for tag in ["v0.1.3", "v0.1.4"] {
            let listing = command(
                "git",
                &[
                    "ls-remote",
                    "--tags",
                    root.to_str().unwrap(),
                    &format!("refs/tags/{tag}"),
                    &format!("refs/tags/{tag}^{{}}"),
                ],
                &root,
            )
            .unwrap();
            assert_eq!(parse_remote_tag_commit(&listing, tag).unwrap(), commit);
        }
    }

    #[test]
    fn disposable_release_preparation_updates_manifest_lock_and_review_files() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-preparation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create temporary preparation repo: {e}"),
            }
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove temporary preparation repo");
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("crates/wit-language-server/src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
        fs::write(
            root.join("crates/wit-language-server/src/lib.rs"),
            "pub fn fixture() {}\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), 
            "[package]\nname = \"zed-wit\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\"crates/wit-language-server\"]\nresolver = \"3\"\n"
        ).unwrap();
        fs::write(
            root.join(Scope::Lsp.manifest()),
            "[package]\nname = \"wit-language-server\"\nversion = \"0.1.2\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("extension.toml"), "version = \"0.1.0\"\n").unwrap();
        fs::write(
            root.join("CHANGELOG.md"),
            "# Changelog\n\n## Unreleased\n\n- fixture\n",
        )
        .unwrap();
        command("cargo", &["generate-lockfile", "--offline"], &root).unwrap();
        git(&["init", "-q", "-b", "main"], &root).unwrap();
        git(&["add", "."], &root).unwrap();
        command(
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "base",
            ],
            &root,
        )
        .unwrap();
        let before = git(&["rev-parse", "HEAD"], &root).unwrap();
        let version = Version::parse("0.1.2").unwrap();
        let candidate = version.bump(Bump::Patch).unwrap();
        let tag = Scope::Lsp.tag(candidate);
        let plan = Plan {
            current: version,
            candidate,
            tag: tag.clone(),
            branch: format!("release-prep/lsp-{tag}"),
            notes: "docs/releases/lsp/v0.1.3.md".into(),
            main_sha: before.clone(),
        };
        check_prepare_paths(&root, Scope::Lsp, &plan.notes).unwrap();
        git(&["switch", "-q", "-c", &plan.branch], &root).unwrap();
        apply_local_release_files(&root, Scope::Lsp, &plan).unwrap();
        assert_eq!(manifest_version(&root, Scope::Lsp).unwrap(), candidate);
        assert_eq!(
            manifest_version(&root, Scope::Extension).unwrap(),
            Version::parse("0.1.0").unwrap()
        );
        let lock = util::read_nonempty(&root.join("Cargo.lock")).unwrap();
        assert!(lock.contains("name = \"wit-language-server\"\nversion = \"0.1.3\""));
        assert!(
            util::read_nonempty(&root.join(&plan.notes))
                .unwrap()
                .contains(PREPARATION_MARKER)
        );
        assert!(
            util::read_nonempty(&root.join("CHANGELOG.md"))
                .unwrap()
                .contains(&tag)
        );
        assert!(!git(&["status", "--porcelain"], &root).unwrap().is_empty());
        assert_eq!(git(&["rev-parse", &before], &root).unwrap(), before);
        assert!(check_prepare_paths(&root, Scope::Lsp, &plan.notes).is_err());
    }

    #[test]
    fn planning_rejects_disposable_feature_branch_at_the_same_main_commit() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-publish-main-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create disposable Git repo: {error}"),
            }
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove disposable Git repo");
            }
        }
        let _cleanup = Cleanup(root.clone());
        git(&["init", "-q", "-b", "main"], &root).unwrap();
        command(
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "initial",
            ],
            &root,
        )
        .unwrap();
        let main_sha = git(&["rev-parse", "HEAD"], &root).unwrap();
        ensure_main_branch(&root).unwrap();
        git(&["switch", "-q", "-c", "fixture-feature"], &root).unwrap();
        assert_eq!(git(&["rev-parse", "HEAD"], &root).unwrap(), main_sha);
        assert!(ensure_main_branch(&root).is_err());
    }

    #[test]
    fn reviewed_pr_patch_must_contain_only_exact_scoped_version_transition() {
        let candidate = Version::parse("0.1.3").unwrap();
        let expected = Version::parse("0.1.2").unwrap();
        let old = "@@ -2,3 +2,3 @@\n-version = \"0.1.2\"\n+version = \"0.1.3\"\n";
        assert_eq!(changed_manifest_version_from_patch(old, candidate).unwrap(), expected);
        for invalid in [
            "@@ -1,2 +1,2 @@\n-name = \"renamed\"\n+name = \"another\"\n",
            "@@ -1,2 +1,2 @@\n-version = \"0.1.3\"\n+version = \"0.1.3\"\n",
            "@@ -1,2 +1,2 @@\n-version = \"0.1.2\"\n+version = \"0.1.4\"\n",
            "@@ -1,2 +1,2 @@\n-version = \"0.1.2\"\n+version = \"0.1.3\"\n-dependencies = \"x\"\n+dependencies = \"y\"\n",
            "",
        ] {
            assert!(changed_manifest_version_from_patch(invalid, candidate).is_err());
        }
        let required = [
            ("crates/wit-language-server/Cargo.toml", "modified", Some(old)),
            ("Cargo.lock", "modified", None),
            ("CHANGELOG.md", "modified", None),
            ("docs/releases/lsp/v0.1.3.md", "added", None),
        ];
        let serialize = |items: &[(&str, &str, Option<&str>)]| {
            items.iter().map(|(name, status, patch)| {
                serde_json::json!({"filename": name, "status": status, "patch": patch}).to_string()
            }).collect::<Vec<_>>().join("\n")
        };
        assert_eq!(reviewed_pr_file_versions(&serialize(&required), Scope::Lsp, candidate).unwrap(), expected);
        let tampered = [
            ("crates/wit-language-server/Cargo.toml", "modified", Some("@@ -1 +1 @@\n-version = \"0.1.3\"\n+version = \"0.1.3\"")),
            ("Cargo.lock", "modified", None), ("CHANGELOG.md", "modified", None),
            ("docs/releases/lsp/v0.1.3.md", "added", None),
        ];
        assert!(reviewed_pr_file_versions(&serialize(&tampered), Scope::Lsp, candidate).is_err());
        let missing = [
            ("crates/wit-language-server/Cargo.toml", "modified", None),
            ("Cargo.lock", "modified", None), ("CHANGELOG.md", "modified", None),
            ("docs/releases/lsp/v0.1.3.md", "added", None),
        ];
        assert!(reviewed_pr_file_versions(&serialize(&missing), Scope::Lsp, candidate).is_err());
    }

    #[test]
    fn pr_filenames_cannot_substitute_for_a_reviewed_version_transition() {
        let old = Version::parse("0.1.2").unwrap();
        let next = Version::parse("0.1.3").unwrap();
        assert!(ensure_release_transition(old, next, next, next).is_ok());
        assert!(ensure_release_transition(next, next, next, next).is_err());
        assert!(ensure_release_transition(old, old, next, next).is_err());
        assert!(ensure_release_transition(old, next, old, next).is_err());
        assert!(ensure_release_transition(old, next, next, old).is_err());
        assert!(
            ensure_release_transition(old, Version::parse("0.2.0").unwrap(), next, next).is_err()
        );
        assert!(valid_version_transition(
            old,
            Version::parse("0.2.0").unwrap()
        ));
        assert!(valid_version_transition(
            old,
            Version::parse("1.0.0").unwrap()
        ));
        assert!(!valid_version_transition(
            old,
            Version::parse("0.1.4").unwrap()
        ));
    }

    #[test]
    fn preparation_limits_changed_files_by_scope_and_version() {
        let v = Version::parse("0.1.3").unwrap();
        assert!(validate_changed_files(
            "crates/wit-language-server/Cargo.toml\nCargo.lock\nCHANGELOG.md\ndocs/releases/lsp/v0.1.3.md",
            Scope::Lsp, v
        ).is_ok());
        assert!(validate_changed_files(
            "Cargo.toml\nextension.toml\nCargo.lock\nCHANGELOG.md\ndocs/releases/extension/v0.1.3.md",
            Scope::Extension, v
        ).is_ok());
        assert!(
            validate_changed_files("CHANGELOG.md\ndocs/releases/lsp/v0.1.3.md", Scope::Lsp, v)
                .is_err()
        );
        assert!(
            validate_changed_files(
                "Cargo.toml\nCHANGELOG.md\ndocs/releases/extension/v0.1.3.md",
                Scope::Extension,
                v
            )
            .is_err()
        );
        assert!(validate_changed_files("extension.toml", Scope::Lsp, v).is_err());
        assert!(validate_changed_files(".github/workflows/release.yml", Scope::Lsp, v).is_err());
        assert!(
            validate_changed_files("crates/wit-language-server/src/main.rs", Scope::Lsp, v)
                .is_err()
        );
    }

    #[test]
    fn lightweight_and_annotated_tags_resolve_to_commit_not_tag_object() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let tag_object = "ffffffffffffffffffffffffffffffffffffffff";
        let lightweight = format!("{commit}\trefs/tags/v0.1.3");
        let annotated = format!("{tag_object}\trefs/tags/v0.1.3\n{commit}\trefs/tags/v0.1.3^{{}}");
        assert_eq!(
            parse_remote_tag_commit(&lightweight, "v0.1.3").unwrap(),
            commit
        );
        assert_eq!(
            parse_remote_tag_commit(&annotated, "v0.1.3").unwrap(),
            commit
        );
        assert!(parse_remote_tag_commit("", "v0.1.3").is_err());
        assert!(parse_remote_tag_commit(&format!("{commit}\trefs/tags/other"), "v0.1.3").is_err());
        assert!(
            parse_remote_tag_commit(
                &format!("{annotated}\n{commit}\trefs/tags/v0.1.3^{{}}"),
                "v0.1.3"
            )
            .is_err()
        );
    }

    #[test]
    fn cd_run_retry_matrix_requires_known_safe_terminal_failures() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let tag = "v0.1.3";
        let row = |id: u64, status: &str, conclusion: &str, operation: &str| {
            format!(
                "{id}\t{sha}\tworkflow_dispatch\t{status}\t{conclusion}\tCD / {operation} / lsp / {tag}"
            )
        };
        assert!(
            validate_prior_cd_runs(
                &row(1, "completed", "success", "validate"),
                sha,
                Scope::Lsp,
                tag,
                false
            )
            .is_ok()
        );
        for conclusion in ["failure", "cancelled", "timed_out"] {
            let failed = row(2, "completed", conclusion, "publish");
            assert!(validate_prior_cd_runs(&failed, sha, Scope::Lsp, tag, true).is_ok());
            assert!(validate_prior_cd_runs(&failed, sha, Scope::Lsp, tag, false).is_ok());
        }
        for conclusion in [
            "success",
            "neutral",
            "skipped",
            "stale",
            "action_required",
            "unknown",
        ] {
            let previous = row(3, "completed", conclusion, "publish");
            assert!(validate_prior_cd_runs(&previous, sha, Scope::Lsp, tag, true).is_err());
        }
        for status in ["queued", "in_progress", "waiting", "pending"] {
            let active = row(4, status, "unknown", "publish");
            assert!(validate_prior_cd_runs(&active, sha, Scope::Lsp, tag, true).is_err());
        }
        let verified_failure = row(5, "completed", "failure", "publish");
        let verified_success = row(6, "completed", "success", "publish");
        assert!(
            validate_prior_cd_runs(
                &format!("{verified_failure}\n{verified_success}"),
                sha,
                Scope::Lsp,
                tag,
                true,
            )
            .is_err()
        );
        assert!(
            validate_prior_cd_runs(
                &format!("{verified_failure}\n{verified_failure}"),
                sha,
                Scope::Lsp,
                tag,
                true,
            )
            .is_err()
        );
        assert!(validate_prior_cd_runs("", sha, Scope::Lsp, tag, true).is_err());
        assert!(validate_prior_cd_runs("malformed", sha, Scope::Lsp, tag, false).is_err());
        let legacy = format!("7\t{sha}\tworkflow_dispatch\tcompleted\tfailure\tCD");
        assert!(validate_prior_cd_runs(&legacy, sha, Scope::Lsp, tag, true).is_err());
        let other_scope = format!(
            "8\t{sha}\tworkflow_dispatch\tcompleted\tsuccess\tCD / publish / extension / v-extension-0.1.1"
        );
        assert!(validate_prior_cd_runs(&other_scope, sha, Scope::Lsp, tag, false).is_ok());
    }

    #[test]
    fn protected_workflow_rejects_changed_main_sha_before_release_validation() {
        let workflow = include_str!("../../../../.github/workflows/release.yml");
        assert!(workflow.contains(
            "run-name: CD / ${{ inputs.operation }} / ${{ inputs.scope }} / ${{ inputs.tag }}"
        ));
        assert!(workflow.contains("expected_sha:"));
        assert!(workflow.contains("EXPECTED_SHA: ${{ inputs.expected_sha }}"));
        assert!(workflow.contains("if [ \"$GITHUB_SHA\" != \"$EXPECTED_SHA\" ]; then"));
        let guard = workflow
            .find("Require expected release commit when provided")
            .unwrap();
        let checkout = workflow.find("uses: actions/checkout@").unwrap();
        assert!(
            guard < checkout,
            "source SHA guard must precede checkout and CD work"
        );
    }

    #[test]
    fn exact_workflow_wait_state_matrix_and_published_release_verification() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let tag = "v0.1.3";
        let mut run = serde_json::json!({
            "id": 19, "head_sha": sha, "head_branch": "main",
            "event": "workflow_dispatch",
            "path": ".github/workflows/release.yml",
            "display_title": "CD / publish / lsp / v0.1.3",
            "status": "waiting", "conclusion": null,
        });
        for status in ["requested", "queued", "waiting", "pending", "in_progress"] {
            run["status"] = status.into();
            assert_eq!(
                classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).unwrap(),
                WorkflowProgress::Pending,
            );
        }
        run["status"] = "completed".into();
        for conclusion in ["failure", "cancelled", "timed_out", "stale", "skipped"] {
            run["conclusion"] = conclusion.into();
            assert!(classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).is_err());
        }
        run["conclusion"] = "success".into();
        assert_eq!(
            classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).unwrap(),
            WorkflowProgress::Succeeded,
        );
        run["head_sha"] = "0000000000000000000000000000000000000000".into();
        assert!(classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).is_err());
        run["head_sha"] = sha.into();
        run["path"] = ".github/workflows/release.yml@main".into();
        assert_eq!(
            classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).unwrap(),
            WorkflowProgress::Succeeded,
        );
        run["path"] = ".github/workflows/another.yml@main".into();
        assert!(classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).is_err());
        run["path"] = ".github/workflows/release.yml@main".into();
        run["id"] = 20.into();
        assert!(classify_workflow_run(&run, 19, sha, Scope::Lsp, tag).is_err());

        let published = serde_json::json!({
            "tag_name": tag, "draft": false, "immutable": true,
        });
        assert!(verify_published_release(&published, tag).is_ok());
        for invalid in [
            serde_json::json!({"tag_name": tag, "draft": true, "immutable": true}),
            serde_json::json!({"tag_name": tag, "draft": false, "immutable": false}),
            serde_json::json!({"tag_name": "v0.1.4", "draft": false, "immutable": true}),
            serde_json::json!({}),
        ] {
            assert!(verify_published_release(&invalid, tag).is_err());
        }
    }

    #[test]
    fn mocked_pr_state_rejects_wrong_merged_identity() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let version = Version::parse("0.1.3").unwrap();
        let mut pr = serde_json::json!({
            "merged": true,
            "base": {"ref": "main"},
            "head": {"ref": "release-prep/lsp-v0.1.3",
                     "sha": sha,
                     "repo": {"full_name": "chiploom/zed-wit"}},
            "merge_commit_sha": sha,
            "commits": 2,
            "changed_files": 4,
        });
        assert_eq!(
            verify_merged_preparation_pr(&pr, Scope::Lsp, version).unwrap().2,
            2
        );
        pr["merged"] = false.into();
        assert!(verify_merged_preparation_pr(&pr, Scope::Lsp, version).is_err());
        pr["merged"] = true.into();
        pr["base"]["ref"] = "wrong".into();
        assert!(verify_merged_preparation_pr(&pr, Scope::Lsp, version).is_err());
        pr["base"]["ref"] = "main".into();
        pr["head"]["repo"]["full_name"] = "someone-else/zed-wit".into();
        assert!(verify_merged_preparation_pr(&pr, Scope::Lsp, version).is_err());
        pr["head"]["repo"]["full_name"] = REPO.into();
        pr["commits"] = 0.into();
        assert!(verify_merged_preparation_pr(&pr, Scope::Lsp, version).is_err());
    }

    #[test]
    fn protected_dispatch_serializes_api_version_and_exact_release_inputs() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let args = protected_dispatch_args(Scope::Lsp, "v0.1.3", sha);
        assert_eq!(args[0..3], ["api", "-X", "POST"]);
        assert!(args.iter().any(|arg| arg == "return_run_details=true"));
        assert!(args.iter().any(|arg| arg == "X-GitHub-Api-Version: 2026-03-10"));
        assert!(args.iter().any(|arg| arg == &format!("inputs[expected_sha]={sha}")));
        assert!(args.iter().any(|arg| arg == "inputs[operation]=publish"));
        assert!(args.iter().any(|arg| arg == "inputs[scope]=lsp"));
        assert!(args.iter().any(|arg| arg == "inputs[tag]=v0.1.3"));
        assert!(args.iter().any(|arg| arg == "ref=main"));
        assert!(!args.iter().any(|arg| arg.starts_with("Authorization:")));
    }

    #[test]
    fn workflow_dispatch_must_identify_one_exact_authenticated_run() {
        let valid = r#"{"workflow_run_id":1234,"html_url":"https://github.com/chiploom/zed-wit/actions/runs/1234"}"#;
        assert_eq!(parse_dispatch_identity(valid).unwrap().0, 1234);
        for response in [
            "",
            "{}",
            r#"{"workflow_run_id":0,"html_url":"https://github.com/chiploom/zed-wit/actions/runs/0"}"#,
            r#"{"workflow_run_id":1234,"html_url":"https://github.com/chiploom/zed-wit/actions/runs/1235"}"#,
            r#"{"workflow_run_id":1234,"html_url":"https://attacker.invalid/chiploom/zed-wit/actions/runs/1234"}"#,
        ] {
            assert!(parse_dispatch_identity(response).is_err());
        }
    }

    #[test]
    fn malformed_arguments_fail_before_network_or_filesystem_access() {
        // These must be rejected by the pure argument phase, regardless of
        // GitHub credentials and regardless of the current working directory.
        assert!(
            publish(&args(&[
                "--scope",
                "lsp",
                "--resume",
                "--confirm",
                "--pr",
                "abc",
            ]))
            .is_err()
        );
        assert!(publish(&args(&["--scope", "invalid"])).is_err());
        assert!(publish(&args(&["--scope", "lsp", "--prepare"])).is_err());
        assert!(publish(&args(&["--scope", "lsp", "--resume", "--confirm"])).is_err());
    }
}
