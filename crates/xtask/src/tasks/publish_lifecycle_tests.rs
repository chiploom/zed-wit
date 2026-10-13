//! Offline end-to-end publishing contract qualification.
//! These tests only use disposable Git repositories and synthetic GitHub data.
//! No gh executable, GitHub credential, remote release, or production CD run is used.
use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bare: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "zed-wit-publish-lifecycle-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create isolated publishing fixture: {e}"),
            }
        };
        let repo = root.join("working");
        let bare = root.join("remote.git");
        fs::create_dir(&repo).unwrap();
        git(&["init", "--bare", "-q", bare.to_str().unwrap()], &root).unwrap();
        git(&["init", "-q", "-b", "main"], &repo).unwrap();
        git(&["config", "user.name", "Lifecycle Fixture"], &repo).unwrap();
        git(&["config", "user.email", "fixture@example.invalid"], &repo).unwrap();
        git(&["config", "commit.gpgSign", "false"], &repo).unwrap();
        let hooks = root.join("empty-hooks");
        fs::create_dir(&hooks).unwrap();
        git(
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
            &repo,
        )
        .unwrap();

        fs::create_dir_all(repo.join("src")).unwrap();
        fs::create_dir_all(repo.join("crates/wit-language-server/src")).unwrap();
        fs::write(repo.join(".gitignore"), "/target\n").unwrap();
        fs::write(repo.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
        fs::write(
            repo.join("crates/wit-language-server/src/lib.rs"),
            "pub fn fixture() {}\n",
        )
        .unwrap();
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"zed-wit\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\"crates/wit-language-server\"]\nresolver = \"3\"\n",
        ).unwrap();
        fs::write(
            repo.join("crates/wit-language-server/Cargo.toml"),
            "[package]\nname = \"wit-language-server\"\nversion = \"0.1.2\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(repo.join("extension.toml"), "version = \"0.1.0\"\n").unwrap();
        fs::write(
            repo.join("CHANGELOG.md"),
            "# Changelog\n\n## Unreleased\n\n- fixture\n",
        )
        .unwrap();
        command("cargo", &["generate-lockfile", "--offline"], &repo).unwrap();
        git(&["add", "."], &repo).unwrap();
        git(&["commit", "--no-verify", "-qm", "initial"], &repo).unwrap();
        git(&["remote", "add", "origin", bare.to_str().unwrap()], &repo).unwrap();
        git(&["push", "-q", "origin", "main"], &repo).unwrap();
        Self { root, repo, bare }
    }

    fn commit(&self, title: &str) -> String {
        git(&["add", "."], &self.repo).unwrap();
        git(&["commit", "--no-verify", "-qm", title], &self.repo).unwrap();
        git(&["rev-parse", "HEAD"], &self.repo).unwrap()
    }

    fn remote_ref(&self, branch: &str) -> String {
        git(
            &[
                "ls-remote",
                "--heads",
                self.bare.to_str().unwrap(),
                &format!("refs/heads/{branch}"),
            ],
            &self.repo,
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn mocked_main_rules() -> String {
    let checks = REQUIRED_RELEASE_CI_CONTEXTS
        .iter()
        .map(|name| json!({"context": name, "integration_id": GITHUB_ACTIONS_INTEGRATION_ID}))
        .collect::<Vec<_>>();
    [
        json!({"type":"pull_request", "parameters":{"required_approving_review_count":0}}),
        json!({"type":"required_linear_history"}),
        json!({"type":"required_status_checks", "parameters":{
            "strict_required_status_checks_policy":true,
            "required_status_checks":checks
        }}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

fn exercise_lifecycle(scope: Scope) {
    let fixture = Fixture::new();
    let repo = &fixture.repo;
    let base = git(&["rev-parse", "HEAD"], repo).unwrap();
    let before = manifest_version(repo, scope).unwrap();
    let candidate = before.bump(Bump::Patch).unwrap();
    let tag = scope.tag(candidate);
    let branch = format!("release-prep/{}-{tag}", scope.name());
    let notes = format!("docs/releases/{}/v{}.md", scope.name(), candidate.value());
    let plan = Plan {
        current: before,
        candidate,
        tag: tag.clone(),
        branch: branch.clone(),
        notes: notes.clone(),
        main_sha: base.clone(),
    };

    // Prepare exactly the same manifests, Cargo.lock, notes and changelog
    // as the real frontend. A second preparation must not overwrite files.
    check_prepare_paths(repo, scope, &notes).unwrap();
    git(&["switch", "-q", "-c", &branch], repo).unwrap();
    apply_local_release_files(repo, scope, &plan).unwrap();
    assert!(check_prepare_paths(repo, scope, &notes).is_err());
    assert!(notes_reviewed(repo, scope, candidate).is_err());
    let reviewed_notes = format!(
        "# {} {}\n\nThis change qualifies a completely isolated release lifecycle \
        without using external services or publishing credentials. \
        It verifies the version transition, immutable provenance, \
        policy gates, release identity and finalization rules.\n",
        scope.title(),
        candidate.value()
    );
    fs::write(repo.join(&notes), reviewed_notes).unwrap();
    let changelog = util::read_nonempty(&repo.join("CHANGELOG.md")).unwrap();
    fs::write(
        repo.join("CHANGELOG.md"),
        changelog.replace(
            PREPARATION_MARKER,
            "Reviewed release qualification using isolated fixtures.",
        ),
    )
    .unwrap();
    notes_reviewed(repo, scope, candidate).unwrap();

    // Exercise the actual post-review submit implementation against a
    // disposable Git remote and fake PR API. Its first attempt intentionally
    // loses the PR-create response after the branch was pushed.
    let reviewed_sha = fixture.commit("reviewed release preparation");
    let delta = git(&["diff", "--name-only", &base, &reviewed_sha], repo).unwrap();
    validate_changed_files(&delta, scope, candidate).unwrap();
    let ref_name = format!("refs/heads/{branch}");
    let push_url = fixture.bare.to_str().unwrap();
    assert_eq!(
        classify_preparation_branch(&fixture.remote_ref(&branch), &branch, &reviewed_sha).unwrap(),
        PreparationPush::PushNew
    );
    let mut first_calls = Vec::new();
    let failed = submit_preparation_pr(
        repo,
        scope,
        candidate,
        &base,
        push_url,
        |args| {
            first_calls.push(args.join(" "));
            match args {
                ["pr", "list", ..] => Ok("[]".to_owned()),
                ["pr", "create", ..] => Err("simulated lost PR creation response".into()),
                _ => Err(format!("unexpected mock PR API request: {args:?}")),
            }
        },
        || Ok(fixture.remote_ref("main").split_whitespace().next() == Some(base.as_str())),
    );
    assert!(failed.is_err());
    assert_eq!(first_calls.len(), 2);
    assert_eq!(
        classify_preparation_branch(&fixture.remote_ref(&branch), &branch, &reviewed_sha).unwrap(),
        PreparationPush::ExistingExactCommit
    );

    // A new submit invocation may reuse only the identical existing branch.
    // Production must not push again or silently overwrite it.
    let mut retry_calls = Vec::new();
    submit_preparation_pr(
        repo,
        scope,
        candidate,
        &base,
        push_url,
        |args| {
            retry_calls.push(args.join(" "));
            match args {
                ["pr", "list", ..] => Ok("[]".to_owned()),
                ["pr", "create", ..] => {
                    assert!(args.contains(&"main"));
                    assert!(args.contains(&branch.as_str()));
                    Ok("https://github.com/chiploom/zed-wit/pull/123".into())
                }
                ["api", endpoint] if *endpoint == format!("repos/{REPO}/pulls/123") => Ok(json!({
                    "base":{"ref":"main"},
                    "head":{"ref":branch,"repo":{"full_name":REPO},"sha":reviewed_sha},
                })
                .to_string()),
                _ => Err(format!("unexpected mock PR API request: {args:?}")),
            }
        },
        || panic!("retry must not issue another Git push"),
    )
    .unwrap();
    assert_eq!(retry_calls.len(), 3);
    assert_eq!(
        classify_preparation_branch(&fixture.remote_ref(&branch), &branch, &reviewed_sha).unwrap(),
        PreparationPush::ExistingExactCommit
    );
    // A same-SHA push is a no-op and may legitimately succeed. Advance
    // this disposable checkout so the second push actually tries to update
    // an already-allocated ref; the expected-absent lease must reject it.
    git(
        &[
            "commit",
            "--allow-empty",
            "--no-verify",
            "-qm",
            "competing commit",
        ],
        repo,
    )
    .unwrap();
    assert_ne!(git(&["rev-parse", "HEAD"], repo).unwrap(), reviewed_sha);
    assert!(
        git(
            &[
                "push",
                "--porcelain",
                &format!("--force-with-lease={ref_name}:"),
                push_url,
                &format!("HEAD:{ref_name}"),
            ],
            repo
        )
        .is_err()
    );
    assert_eq!(
        classify_preparation_branch(&fixture.remote_ref(&branch), &branch, &reviewed_sha).unwrap(),
        PreparationPush::ExistingExactCommit
    );
    // The production submission function must also refuse an advanced
    // local candidate when the remote already has the reviewed branch.
    let conflict = submit_preparation_pr(
        repo,
        scope,
        candidate,
        &base,
        push_url,
        |args| match args {
            ["pr", "list", ..] => Ok("[]".into()),
            _ => panic!("conflicting branch must not reach PR creation"),
        },
        || panic!("conflicting branch must not enter the push path"),
    );
    assert!(conflict.is_err());
    git(&["reset", "--hard", &reviewed_sha], repo).unwrap();

    // Existing GitHub PRs block duplicate creation before branch mutation.
    let duplicate = submit_preparation_pr(
        repo,
        scope,
        candidate,
        &base,
        push_url,
        |args| match args {
            ["pr", "list", ..] => Ok(json!([{"number":123}]).to_string()),
            _ => panic!("duplicate PR may not trigger writes"),
        },
        || panic!("duplicate PR may not trigger Git pushes"),
    );
    assert!(duplicate.is_err());

    let mut records = Vec::new();
    for name in delta.lines() {
        let patch = if name == scope.manifest() || name == "extension.toml" {
            git(
                &[
                    "diff",
                    "--no-ext-diff",
                    "--unified=0",
                    &base,
                    &reviewed_sha,
                    "--",
                    name,
                ],
                repo,
            )
            .unwrap()
        } else {
            String::new()
        };
        records.push(json!({
            "filename":name,
            "status":if name == notes {"added"} else {"modified"},
            "patch":patch,
        }));
    }
    let pr_files = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let reviewed_prior = reviewed_pr_file_versions(&pr_files, scope, candidate).unwrap();
    assert_eq!(reviewed_prior, before);

    // Simulate a real squash merge. The head SHA and merge SHA deliberately
    // differ, as they do for actual GitHub squash merges.
    git(&["switch", "-q", "main"], repo).unwrap();
    git(&["merge", "--squash", &branch], repo).unwrap();
    let merged_sha = fixture.commit("merge reviewed release preparation");
    assert_ne!(reviewed_sha, merged_sha);
    git(&["push", "-q", "origin", "main"], repo).unwrap();
    assert_eq!(
        fixture.remote_ref("main").split_whitespace().next(),
        Some(merged_sha.as_str())
    );
    let pr = json!({
        "merged":true,
        "base":{"ref":"main"},
        "head":{"ref":branch, "sha":reviewed_sha, "repo":{"full_name":REPO}},
        "merge_commit_sha":merged_sha,
        "commits":1,
        "changed_files":records.len()
    });
    let (merged, reviewed_head, commits) =
        verify_merged_preparation_pr(&pr, scope, candidate).unwrap();
    assert_eq!(merged, merged_sha);
    assert_eq!(reviewed_head, reviewed_sha);
    assert_eq!(commits, 1);
    let historical =
        historical_predecessor_version(repo, scope, &merged, commits, false, candidate).unwrap();
    assert_eq!(historical, reviewed_prior);
    let on_reviewed_head = git_manifest_version(repo, &reviewed_sha, scope, false).unwrap();
    let on_merge = git_manifest_version(repo, &merged, scope, false).unwrap();
    ensure_release_transition(
        historical,
        on_reviewed_head,
        on_merge,
        manifest_version(repo, scope).unwrap(),
    )
    .unwrap();
    if scope == Scope::Extension {
        let predecessor =
            historical_predecessor_version(repo, scope, &merged, commits, true, candidate).unwrap();
        let on_head = git_manifest_version(repo, &reviewed_sha, scope, true).unwrap();
        let on_merged = git_manifest_version(repo, &merged, scope, true).unwrap();
        let current = version_in_manifest(
            &util::read_nonempty(&repo.join("extension.toml")).unwrap(),
            true,
        )
        .unwrap();
        ensure_release_transition(predecessor, on_head, on_merged, current).unwrap();
    }
    require_exact_release_source(&merged_sha, &merged).unwrap();
    assert!(require_exact_release_source(&reviewed_sha, &merged).is_err());

    // Mock all GitHub authorization gates before a simulated dispatch.
    validate_protected_main_rules(&mocked_main_rules()).unwrap();
    let tag_rules = json!({
        "id":37,"target":"tag","enforcement":"active",
        "bypass_actors":[],
        "conditions":{"ref_name":{"include":["refs/tags/v*"],"exclude":[]}},
        "rules":[{"type":"update"},{"type":"deletion"},{"type":"non_fast_forward"}]
    })
    .to_string();
    validate_fetched_release_tag_rulesets(&json!({"id":37}).to_string(), |id| {
        assert_eq!(id, 37);
        Ok(tag_rules.clone())
    })
    .unwrap();
    let environment = json!({
        "name":"release","protection_rules":[],
        "deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}
    });
    let only_main = json!({
        "total_count":1,
        "branch_policies":[{"name":"main","type":"branch"}]
    })
    .to_string();
    validate_release_environment_protection(&environment, Some(&only_main)).unwrap();
    assert!(
        validate_release_environment_protection(
            &environment,
            Some(&only_main.replace("\"main\"", "\"main*\"")),
        )
        .is_err()
    );
    validate_resume_candidate(scope, candidate, &BTreeSet::new(), false).unwrap();
    let dispatch_lock =
        try_acquire_dispatch_lock(&repo.join(".git/lifecycle-dispatch.lock")).unwrap();
    assert!(try_acquire_dispatch_lock(&repo.join(".git/lifecycle-dispatch.lock")).is_err());
    // Drive the *same* request/dispatch/poll/finalize function used by
    // resume(), with a strict local transport that refuses unknown calls.
    let mut requests = Vec::new();
    let mut polls = 0;
    let mut sleep_count = 0;
    let title = format!("CD / publish / {} / {tag}", scope.name());
    let run_url = "https://github.com/chiploom/zed-wit/actions/runs/42";
    dispatch_protected_cd(
        scope,
        &tag,
        &merged_sha,
        false,
        3,
        |args| {
            requests.push(args.join(" "));
            match args {
                ["api", endpoint]
                    if *endpoint
                        == format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}") =>
                {
                    Ok(json!({"state":"active"}).to_string())
                }
                ["api", "--paginate", "--jq", _, endpoint]
                    if *endpoint
                        == format!(
                            "repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/runs?per_page=100"
                        ) =>
                {
                    Ok(String::new())
                }
                ["api", "-X", "POST", ..] => {
                    assert!(args.contains(&"return_run_details=true"));
                    assert!(args.contains(&"ref=main"));
                    assert!(args.iter().any(|v| *v == format!("inputs[tag]={tag}")));
                    assert!(
                        args.iter()
                            .any(|v| *v == format!("inputs[expected_sha]={merged_sha}"))
                    );
                    assert!(args.contains(&"inputs[operation]=publish"));
                    Ok(json!({"workflow_run_id":42,"html_url":run_url}).to_string())
                }
                ["api", endpoint] if *endpoint == format!("repos/{REPO}/actions/runs/42") => {
                    polls += 1;
                    Ok(json!({
                        "id":42,"head_sha":merged_sha,"head_branch":"main",
                        "event":"workflow_dispatch",
                        "path":".github/workflows/release.yml@main",
                        "display_title":title,
                        "status":if polls == 1 {"queued"} else {"completed"},
                        "conclusion":if polls == 1 {Value::Null} else {json!("success")}
                    })
                    .to_string())
                }
                ["api", endpoint] if *endpoint == format!("repos/{REPO}/releases/tags/{tag}") => {
                    Ok(json!({"draft":false,"tag_name":tag,"immutable":true}).to_string())
                }
                _ => Err(format!("unexpected fixture GitHub request: {args:?}")),
            }
        },
        |_| sleep_count += 1,
    )
    .unwrap();
    assert_eq!(polls, 2);
    assert_eq!(sleep_count, 1);
    assert_eq!(requests.len(), 6);
    assert!(requests[2].contains("POST"));
    assert!(requests[5].contains(&format!("releases/tags/{tag}")));
    let completed_run = format!("42\t{merged_sha}\tworkflow_dispatch\tcompleted\tsuccess\t{title}");
    assert!(validate_prior_cd_runs(&completed_run, &merged_sha, scope, &tag, false).is_err());
    drop(dispatch_lock);
    assert!(try_acquire_dispatch_lock(&repo.join(".git/lifecycle-dispatch.lock")).is_ok());
    assert!(git(&["status", "--porcelain"], repo).unwrap().is_empty());
}

#[test]
fn lsp_prepare_submit_squash_resume_mocked_cd_lifecycle() {
    exercise_lifecycle(Scope::Lsp);
}

#[test]
fn extension_prepare_submit_squash_resume_mocked_cd_lifecycle() {
    exercise_lifecycle(Scope::Extension);
}

#[test]
fn protected_cd_transport_failures_do_not_qualify_publication() {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Failure {
        InactiveWorkflow,
        MalformedWorkflow,
        ConcurrentRun,
        MissingRetryHistory,
        FailedDispatch,
        AmbiguousDispatch,
        MismatchedRun,
        NonimmutableRelease,
        PendingTimeout,
    }
    let scenarios = [
        Failure::InactiveWorkflow,
        Failure::MalformedWorkflow,
        Failure::ConcurrentRun,
        Failure::MissingRetryHistory,
        Failure::FailedDispatch,
        Failure::AmbiguousDispatch,
        Failure::MismatchedRun,
        Failure::NonimmutableRelease,
        Failure::PendingTimeout,
    ];
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let tag = "v0.1.3";
    for scenario in scenarios {
        let mut requests = Vec::new();
        let mut sleeps = 0;
        let result = dispatch_protected_cd(
            Scope::Lsp,
            tag,
            sha,
            scenario == Failure::MissingRetryHistory,
            2,
            |args| {
                requests.push(args.join(" "));
                match args {
                    ["api", endpoint] if *endpoint == format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}") => {
                        match scenario {
                            Failure::MalformedWorkflow => Ok("{".to_owned()),
                            Failure::InactiveWorkflow => Ok(json!({"state":"disabled_manually"}).to_string()),
                            _ => Ok(json!({"state":"active"}).to_string()),
                        }
                    }
                    ["api", "--paginate", "--jq", _, endpoint] if *endpoint == format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/runs?per_page=100") => {
                        if scenario == Failure::ConcurrentRun {
                            Ok(format!("2\t{sha}\tworkflow_dispatch\tqueued\tunknown\tCD / publish / lsp / {tag}"))
                        } else {
                            Ok(String::new())
                        }
                    }
                    ["api", "-X", "POST", ..] => match scenario {
                        Failure::FailedDispatch => Err("simulated dispatch transport failure".into()),
                        Failure::AmbiguousDispatch => Ok("{}".into()),
                        _ => Ok(json!({
                            "workflow_run_id":42,
                            "html_url":"https://github.com/chiploom/zed-wit/actions/runs/42"
                        }).to_string()),
                    },
                    ["api", endpoint] if *endpoint == format!("repos/{REPO}/actions/runs/42") => {
                        Ok(json!({
                            "id":42,
                            "head_sha":if scenario == Failure::MismatchedRun {"bad-sha"} else {sha},
                            "head_branch":"main",
                            "event":"workflow_dispatch",
                            "path":".github/workflows/release.yml",
                            "display_title":format!("CD / publish / lsp / {tag}"),
                            "status":if scenario == Failure::PendingTimeout {"queued"} else {"completed"},
                            "conclusion":if scenario == Failure::PendingTimeout {Value::Null} else {json!("success")}
                        }).to_string())
                    }
                    ["api", endpoint] if *endpoint == format!("repos/{REPO}/releases/tags/{tag}") => {
                        Ok(json!({
                            "draft":false,
                            "tag_name":tag,
                            "immutable":scenario != Failure::NonimmutableRelease
                        }).to_string())
                    }
                    _ => Err(format!("unexpected mocked API endpoint: {args:?}")),
                }
            },
            |_| sleeps += 1,
        );
        assert!(result.is_err(), "unsafe scenario {scenario:?} succeeded");
        assert!(
            !requests.iter().any(|v| v.contains("releases/tags/"))
                || scenario == Failure::NonimmutableRelease,
            "failed scenario {scenario:?} queried publication state unexpectedly"
        );
        let posted = requests.iter().any(|v| v.contains(" POST "));
        if matches!(
            scenario,
            Failure::InactiveWorkflow
                | Failure::MalformedWorkflow
                | Failure::ConcurrentRun
                | Failure::MissingRetryHistory
        ) {
            assert!(
                !posted,
                "{scenario:?} dispatched despite unsafe precondition"
            );
        }
        assert_eq!(
            sleeps,
            if scenario == Failure::PendingTimeout {
                2
            } else {
                0
            },
            "unexpected polling side effects in {scenario:?}"
        );
    }
}

#[test]
fn failed_cd_run_allows_explicit_retry_without_claiming_publication() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let tag = "v0.1.3";
    let mut endpoints = Vec::new();
    dispatch_protected_cd(
        Scope::Lsp,
        tag,
        sha,
        true,
        0,
        |args| {
            endpoints.push(args.join(" "));
            match args {
                ["api", endpoint] if *endpoint == format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}") => {
                    Ok(json!({"state":"active"}).to_string())
                }
                ["api", "--paginate", "--jq", _, endpoint] if *endpoint == format!("repos/{REPO}/actions/workflows/{RELEASE_WORKFLOW}/runs?per_page=100") => {
                    Ok(format!("41\t{sha}\tworkflow_dispatch\tcompleted\tfailure\tCD / publish / lsp / {tag}"))
                }
                ["api", "-X", "POST", ..] => Ok(json!({
                    "workflow_run_id":42,
                    "html_url":"https://github.com/chiploom/zed-wit/actions/runs/42"
                }).to_string()),
                _ => Err("retry fixture refused extra request".to_owned()),
            }
        },
        |_| panic!("nonwaiting CD dispatch cannot sleep"),
    )
    .unwrap();
    assert_eq!(endpoints.len(), 3);
    assert!(endpoints[2].contains(" POST "));
}
