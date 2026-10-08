//! Version-aware publishing frontend. Protected CD is the only publisher.
//! All planning is read-only; write transitions require explicit confirmation.
use crate::util;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
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
        Ok(Self { major: numbers[0], minor: numbers[1], patch: numbers[2] })
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
enum Scope { Lsp, Extension }
impl Scope {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "lsp" => Ok(Self::Lsp),
            "extension" => Ok(Self::Extension),
            _ => Err(format!("invalid --scope {s:?}; expected lsp or extension")),
        }
    }
    fn name(self) -> &'static str {
        match self { Self::Lsp => "lsp", Self::Extension => "extension" }
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
enum Bump { Patch, Minor, Major }
impl Bump {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "patch" => Ok(Self::Patch),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            _ => Err(format!("invalid bump {s:?}; expected patch, minor or major")),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation { Plan, Prepare, Submit, Resume }
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
                let value = args.get(index).ok_or_else(|| format!("{key} needs a value"))?;
                if value.starts_with("--") { return Err(format!("{key} needs a value")); }
                match key {
                    "--scope" if scope.is_none() => scope = Some(Scope::parse(value)?),
                    "--bump" if bump.is_none() => bump = Some(Bump::parse(value)?),
                    "--pr" if pr.is_none() => {
                        let number = value.parse::<u64>()
                            .map_err(|_| "--pr requires a positive PR number".to_owned())?;
                        if number == 0 { return Err("--pr must not be zero".into()); }
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
        return Err("remote/write actions require --confirm; default planning changes nothing".into());
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

fn manifest_version(root: &Path, scope: Scope) -> Result<Version, String> {
    let path = root.join(scope.manifest());
    let manifest: toml::Value = toml::from_str(&util::read_nonempty(&path)?)
        .map_err(|e| format!("parse {}: {e}", path.display()))?;
    let read = |value: &toml::Value| -> Result<Version, String> {
        let raw = value.get("package").and_then(|v| v.get("version"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("missing package.version in {}", path.display()))?;
        Version::parse(raw)
    };
    let version = read(&manifest)?;
    if scope == Scope::Extension {
        let extension: toml::Value = toml::from_str(
            &util::read_nonempty(&root.join("extension.toml"))?
        ).map_err(|e| format!("parse extension.toml: {e}"))?;
        let other = extension.get("version").and_then(toml::Value::as_str)
            .ok_or("extension.toml omitted version")
            .and_then(Version::parse)?;
        if version != other { return Err("extension manifest versions disagree".into()); }
    }
    Ok(version)
}

// A failed read never falls back to a guessed ref. These functions do not write.
fn command(program: &str, args: &[&str], root: &Path) -> Result<String, String> {
    util::command_output(program, args, root)
}
fn gh(args: &[&str], root: &Path) -> Result<String, String> {
    command("gh", args, root)
        .map_err(|_| format!("GitHub CLI request failed for {}; verify gh auth, permissions and connectivity", REPO))
}
fn check_remote(root: &Path) -> Result<String, String> {
    let repo: Value = serde_json::from_str(&gh(
        &["repo", "view", REPO, "--json", "nameWithOwner,defaultBranchRef"], root
    )?).map_err(|e| format!("parse gh repository metadata: {e}"))?;
    if repo["nameWithOwner"].as_str() != Some(REPO)
        || repo["defaultBranchRef"]["name"].as_str() != Some("main")
    {
        return Err("expected chiploom/zed-wit with protected default branch main".into());
    }
    let refs = command("git", &["ls-remote", "origin", "refs/heads/main"], root)?;
    let sha = refs.split_whitespace().next()
        .filter(|x| x.len() == 40 && x.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("cannot resolve remote main SHA")?;
    Ok(sha.to_owned())
}
fn validate_worktree(root: &Path) -> Result<(), String> {
    let status = command("git", &["status", "--porcelain"], root)?;
    if !status.is_empty() { return Err("publish requires a clean working tree".into()); }
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
    let released = gh(&[
        "api", "--paginate", "--jq", ".[].tag_name",
        "repos/chiploom/zed-wit/releases?per_page=100",
    ], root)?;
    tags.extend(released.lines().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned));
    Ok(tags)
}
fn validate_candidate(scope: Scope, candidate: Version, tags: &BTreeSet<String>) -> Result<(), String> {
    let desired = scope.tag(candidate);
    if tags.contains(&desired) {
        return Err(format!("candidate tag {desired} is already allocated; never reuse it"));
    }
    for tag in tags {
        let raw = match scope {
            Scope::Lsp => tag.strip_prefix('v').filter(|s| !s.starts_with("extension-")),
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
fn plan(root: &Path, scope: Scope, bump: Bump, remote: bool) -> Result<Plan, String> {
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
            return Err("publish must begin at the current remote main commit; fetch and switch to main".into());
        }
        validate_candidate(scope, candidate, &release_tag_history(root)?)?;
        remote_sha
    } else {
        String::new()
    };
    Ok(Plan {
        current, candidate, tag: tag.clone(),
        branch: format!("release-prep/{}-{tag}", scope.name()),
        notes, main_sha,
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
    if !replaced { return Err("manifest version field missing".into()); }
    Ok(output)
}
fn update_manifest(path: &Path, previous: Version, next: Version, table: Option<&str>)
    -> Result<(), String>
{
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
        return Err("publish must use a checked-out main branch; no detached or feature branch".into());
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
    if git(&["show-ref", "--verify", &format!("refs/heads/{}", plan.branch)], root).is_ok() {
        return Err(format!("local release-preparation branch already exists: {}", plan.branch));
    }
    if !git(&["ls-remote", "--heads", "origin", &plan.branch], root)?.is_empty() {
        return Err(format!("remote release-preparation branch already exists: {}", plan.branch));
    }
    print_plan(scope, &plan)?;
    git(&["switch", "-c", &plan.branch], root)?;
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
        fs::create_dir_all(parent)
            .map_err(|e| format!("create release notes directory: {e}"))?;
    }
    let mut file = OpenOptions::new()
        .write(true).create_new(true).open(&notes)
        .map_err(|e| format!("create {}: {e}", notes.display()))?;
    writeln!(file, "# {} {}\n\n{}\n", scope.title(), plan.candidate.value(), PREPARATION_MARKER)
        .map_err(|e| format!("write release notes: {e}"))?;
    let changelog = root.join("CHANGELOG.md");
    let body = util::read_nonempty(&changelog)?;
    if !body.contains("## Unreleased\n") {
        return Err("CHANGELOG.md has no Unreleased heading".into());
    }
    let update = format!(
        "## Unreleased\n\n### {} ({})\n\n- {}\n",
        plan.tag, scope.name(), PREPARATION_MARKER
    );
    fs::write(&changelog, body.replacen("## Unreleased\n", &update, 1))
        .map_err(|e| format!("update changelog: {e}"))?;

    // Cargo updates the workspace package version in Cargo.lock. This is
    // deliberately the non-locked invocation; submit later verifies --locked.
    command("cargo", &["check", "--workspace", "--offline"], root)?;
    println!("Prepared {} at {}. Edit release notes and changelog, review changes, run tests, and commit locally.", plan.tag, plan.branch);
    println!("No branch was pushed and no pull request or release was created.");
    println!("After reviewing and committing: cargo xtask publish --scope {} --submit --confirm", scope.name());
    Ok(())
}

fn notes_reviewed(root: &Path, scope: Scope, version: Version) -> Result<(), String> {
    let notes = root.join(format!(
        "docs/releases/{}/v{}.md", scope.name(), version.value()
    ));
    let text = util::read_nonempty(&notes)?;
    if text.contains(PREPARATION_MARKER) || text.contains("TODO") || text.trim().len() < 100 {
        return Err(format!("release notes need substantive human review: {}", notes.display()));
    }
    let changelog = util::read_nonempty(&root.join("CHANGELOG.md"))?;
    if !changelog.contains(&format!("### {} ({})", scope.tag(version), scope.name()))
        || changelog.contains(PREPARATION_MARKER)
    {
        return Err("changelog must contain reviewed scoped release entry".into());
    }
    Ok(())
}
fn validate_changed_files(
    paths: &str,
    scope: Scope,
    version: Version,
) -> Result<(), String> {
    let mut allowed = BTreeSet::from([
        "Cargo.lock".to_owned(),
        "CHANGELOG.md".to_owned(),
        scope.manifest().to_owned(),
        format!("docs/releases/{}/v{}.md", scope.name(), version.value()),
    ]);
    if scope == Scope::Extension { allowed.insert("extension.toml".into()); }
    for file in paths.lines() {
        if !allowed.contains(file) {
            return Err(format!("release preparation includes unexpected change: {file}"));
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
    git(&["merge-base", "--is-ancestor", &remote_sha, "HEAD"], root)
        .map_err(|_| "release branch is stale or origin/main was not fetched; update deliberately".to_owned())?;
    let delta = git(&["diff", "--name-only", &remote_sha, "HEAD"], root)?;
    validate_changed_files(&delta, scope, version)?;
    if delta.is_empty() { return Err("release preparation branch has no changes".into()); }
    notes_reviewed(root, scope, version)?;
    command("cargo", &["metadata", "--no-deps", "--format-version", "1", "--locked"], root)?;
    crate::tasks::release_ops::release_check_inner(scope.name(), &tag)?;
    crate::tasks::validation::verify(&[])?;

    validate_candidate(scope, version, &release_tag_history(root)?)?;
    let existing = gh(
        &["pr", "list", "-R", REPO, "--state", "all", "--head", &branch,
          "--json", "number,headRefName,baseRefName"], root
    )?;
    let prs: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse existing preparation PRs: {e}"))?;
    if !prs.as_array().is_some_and(Vec::is_empty) {
        return Err("a release preparation PR already exists; review or resume it rather than duplicate".into());
    }
    if !git(&["ls-remote", "--heads", "origin", &branch], root)?.is_empty() {
        return Err("release branch is already on origin; inspect and resume its PR manually".into());
    }
    // These two remote writes are allowed only after explicit --submit --confirm.
    git(&["push", "--set-upstream", "origin", &format!("HEAD:refs/heads/{branch}")], root)?;
    let title = format!("Prepare {} release {}", scope.title(), tag);
    let body = format!(
        "Release preparation for {}.\n\n- Scope: {}\n- Candidate: {}\n- Human-reviewed notes: docs/releases/{}/v{}.md\n\n**No tag or GitHub Release is published by this PR.** Merge through protected review, then resume with cargo xtask publish --scope {} --resume --pr <number> --confirm.",
        tag, scope.name(), tag, scope.name(), version.value(), scope.name()
    );
    let created = gh(&[
        "pr", "create", "-R", REPO, "--base", "main", "--head", &branch,
        "--title", &title, "--body", &body
    ], root)?;
    println!("Release preparation PR submitted: {}", created.trim());
    println!("No publication requested. Merge through normal review and CD policy.");
    Ok(())
}

fn resume(root: &Path, scope: Scope, pr: u64, wait: bool) -> Result<(), String> {
    let main_sha = confirm_remote_main(root)?;
    let version = manifest_version(root, scope)?;
    let tag = scope.tag(version);
    let branch = format!("release-prep/{}-{tag}", scope.name());
    notes_reviewed(root, scope, version)?;
    crate::tasks::release_ops::release_check_inner(scope.name(), &tag)?;

    let info: Value = serde_json::from_str(&gh(
        &["api", &format!("repos/{REPO}/pulls/{pr}")], root
    )?).map_err(|e| format!("parse preparation PR: {e}"))?;
    if info["merged"].as_bool() != Some(true)
        || info["base"]["ref"].as_str() != Some("main")
        || info["head"]["ref"].as_str() != Some(branch.as_str())
    {
        return Err("release preparation PR is not merged on main for this exact tag".into());
    }
    let merge_sha = info["merge_commit_sha"].as_str()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("merged preparation PR omitted merge commit SHA")?;
    git(&["merge-base", "--is-ancestor", merge_sha, &main_sha], root)
        .map_err(|_| "merged preparation commit is not in current main".to_owned())?;

    let known = release_tag_history(root)?;
    if known.contains(&tag) {
        // Even an existing draft requires operator inspection; never dispatch
        // into ambiguous immutable/deleted tag state automatically.
        return Err(format!("tag {tag} already exists; inspect draft/tag history and protected CD manually"));
    }
    // Require the configured workflow to be enabled; do not create an alternate.
    let workflow: Value = serde_json::from_str(&gh(
        &["api", &format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}")], root
    )?).map_err(|e| format!("parse protected CD workflow: {e}"))?;
    if workflow["state"].as_str() != Some("active") {
        return Err("protected CD workflow is not active".into());
    }

    // Conservatively refuse an uncorrelatable repeated dispatch from the same
    // protected source commit. The user must inspect previous runs manually.
    let runs: Value = serde_json::from_str(&gh(
        &["api", &format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/runs?per_page=100")], root
    )?).map_err(|e| format!("parse existing CD workflow runs: {e}"))?;
    if !runs["workflow_runs"].as_array().is_some_and(|rows| rows.iter().all(|run| {
        run["head_sha"].as_str() != Some(main_sha.as_str())
            || run["event"].as_str() != Some("workflow_dispatch")
    })) {
        return Err("a prior CD workflow dispatch exists for this main SHA or run history is ambiguous; inspect before any retry".into());
    }

    // GitHub's 2026 workflow_dispatch response gives the exact run ID.
    // Never fall back to scanning the latest run after an ambiguous dispatch.
    let response = gh(&[
        "api", "-X", "POST",
        &format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/dispatches"),
        "-F", "return_run_details=true", "-f", "ref=main",
        "-f", "inputs[operation]=publish",
        "-f", &format!("inputs[scope]={}", scope.name()),
        "-f", &format!("inputs[tag]={tag}"),
    ], root).map_err(|_| "CD dispatch may have been accepted; check GitHub Actions before retrying".to_owned())?;
    let result: Value = serde_json::from_str(&response)
        .map_err(|_| "CD dispatch returned no run ID; inspect GitHub Actions before retrying".to_owned())?;
    let id = result["workflow_run_id"].as_u64()
        .filter(|id| *id > 0)
        .ok_or("CD dispatch omitted workflow_run_id; inspect GitHub Actions before retrying")?;
    let run_url = result["html_url"].as_str()
        .filter(|url| url.starts_with("https://github.com/chiploom/zed-wit/actions/runs/"))
        .ok_or("CD dispatch returned an unexpected run URL")?;
    println!("Protected CD dispatch requested for {tag}: {run_url} (run {id}).");
    if !wait {
        println!("Status: requested/pending. This is not a published release.");
        return Ok(());
    }
    for _ in 0..180 {
        let run: Value = serde_json::from_str(&gh(
            &["api", &format!("repos/{REPO}/actions/runs/{id}")], root
        )?).map_err(|e| format!("parse exact workflow run {id}: {e}"))?;
        match run["status"].as_str() {
            Some("completed") => {
                if run["conclusion"].as_str() != Some("success") {
                    return Err(format!(
                        "protected CD run {id} finished with {:?}, not published",
                        run["conclusion"].as_str()
                    ));
                }
                let release: Value = serde_json::from_str(&gh(
                    &["api", &format!("repos/{REPO}/releases/tags/{tag}")], root
                )?).map_err(|e| format!("verify published release after CD success: {e}"))?;
                if release["draft"].as_bool() != Some(false)
                    || release["tag_name"].as_str() != Some(tag.as_str())
                    || release["immutable"].as_bool() != Some(true)
                {
                    return Err("CD run passed, but release publication/immutability could not be verified".into());
                }
                println!("Published immutable release {tag}: {run_url}");
                return Ok(());
            }
            Some("queued" | "in_progress" | "requested" | "waiting" | "pending") => {}
            _ => return Err(format!("CD run {id} has unknown status; inspect {run_url}")),
        }
        thread::sleep(Duration::from_secs(10));
    }
    Err(format!("timed out watching CD run {id}; outcome pending: {run_url}"))
}
