//! Local release preparation; protected publishing lives in CD.
use crate::{dependency_policy, licenses, release, util};
use super::{build::{build, release_build}, common::{finish, opts}};

pub(crate) fn changelog_check_inner(scope: Option<&str>, tag: Option<&str>) -> Result<(), String> {
    let root = util::repo_root();
    let changelog = util::read_nonempty(&root.join("CHANGELOG.md"))?;
    if !changelog.contains("# Changelog") || !changelog.contains("## Unreleased") {
        return Err("CHANGELOG.md needs a changelog heading and Unreleased section".into());
    }
    match (scope, tag) {
        (Some(scope), Some(tag)) => {
            let maybe_version = match scope {
                "lsp" => tag.strip_prefix('v'),
                "extension" => tag.strip_prefix("v-extension-"),
                _ => return Err(format!("unsupported release scope {scope:?}")),
            };
            let version = maybe_version.ok_or_else(|| "tag does not match release scope".to_owned())?;
            if version.split('.').count() != 3
                || !version.split('.').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')))
            {
                return Err(format!("invalid stable SemVer release tag: {tag}"));
            }
            let path = root.join(format!("docs/releases/{scope}/v{version}.md"));
            let notes = util::read_nonempty(&path)?;
            if notes.trim().len() < 20 {
                return Err(format!("release notes too short: {}", path.display()));
            }
            println!("validated release notes: {}", path.display());
        }
        (None, None) => {}
        _ => return Err("--scope and --tag must be supplied together".into()),
    }
    println!("changelog validation passed");
    Ok(())
}

pub(crate) fn changelog_check(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag"])?;
    let scope = options.remove("scope");
    let tag = options.remove("tag");
    finish(options)?;
    changelog_check_inner(scope.as_deref(), tag.as_deref())
}

pub(crate) fn release_check_inner(scope: &str, tag: &str) -> Result<(), String> {
    changelog_check_inner(Some(scope), Some(tag))?;
    release::validate_release(tag, scope)
}

pub(crate) fn release_check(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let tag = util::required_option(&mut options, "tag")?;
    finish(options)?;
    release_check_inner(&scope, &tag)
}

pub(crate) fn prepare_release(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag", "target", "output"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let tag = util::required_option(&mut options, "tag")?;
    let target = options.remove("target");
    let specified_output = options.remove("output");
    if scope == "extension" && specified_output.is_some() {
        return Err("--output is only supported for LSP release preparation".into());
    }
    let output = util::root_relative(
        specified_output.unwrap_or_else(|| "dist".into()).into()
    );
    finish(options)?;
    if scope == "extension" && target.is_some() {
        return Err("extension release preparation does not accept --target".into());
    }
    // Validate before building so invalid candidate releases perform no build work.
    release_check_inner(&scope, &tag)?;
    match scope.as_str() {
        "lsp" => {
            let target = target.ok_or_else(|| "LSP preparation requires --target (native release triple)".to_owned())?;
            dependency_policy::ensure_target(&target)?;
            release_build(&["--target".into(), target.clone()])?;
            release::package_release(&target, &output)?;
            licenses::run(&target, &output)?;
            release::verify_release_target(&target, &output)?;
            println!("Local release artifacts prepared. Complete five-target CD validation and protected publication separately.");
        }
        "extension" => {
            build(&["--kind".into(), "extension".into(), "--release".into(), "true".into()])?;
            println!("Extension Wasm candidate built locally. Published LSP dependency and protected CD publication must be verified separately.");
        }
        _ => return Err(format!("unsupported release scope {scope:?}")),
    }
    Ok(())
}
