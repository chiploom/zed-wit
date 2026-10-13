//! Domain-grouped local developer commands. Command routing belongs in cli.rs.
pub(crate) mod build;
pub(crate) mod common;
pub(crate) mod maintenance;
pub(crate) mod performance;
pub(crate) mod release_ops;
pub(crate) mod validation;

#[cfg(test)]
mod tests {
    use super::{
        build::release_build,
        common::bool_option,
        maintenance::{safe_generated_path, update_grammar},
        performance::coverage,
        release_ops::{prepare_release, release_check},
    };
    use crate::util;
    use std::collections::BTreeMap;
    #[test]
    fn all_approved_commands_are_registered() {
        const APPROVED: [&str; 18] = [
            "release-build",
            "release",
            "verify",
            "test",
            "build",
            "check",
            "dev",
            "test-all",
            "release-check",
            "doctor",
            "clean",
            "test-lsp",
            "test-extension",
            "install-dev",
            "bench",
            "coverage",
            "changelog-check",
            "update-grammar",
        ];
        assert_eq!(crate::cli::COMMAND_HELP.len(), 27);
        for cmd in APPROVED {
            assert!(
                crate::cli::COMMAND_HELP
                    .iter()
                    .any(|(name, _)| *name == cmd)
            );
            assert!(
                crate::cli::dispatch(cmd, &["--help".into()]).is_ok(),
                "{cmd}"
            );
        }
    }
    #[test]
    fn unknown_command_is_rejected() {
        assert!(crate::cli::dispatch("publish", &[]).is_err());
        assert!(crate::cli::dispatch("watch", &[]).is_err());
    }
    #[test]
    fn boolean_options_are_strict() {
        let mut options = BTreeMap::from([("execute".into(), "maybe".into())]);
        assert!(bool_option(&mut options, "execute", false).is_err());
        let mut options = BTreeMap::from([("execute".into(), "true".into())]);
        assert!(bool_option(&mut options, "execute", false).unwrap());
    }
    #[test]
    fn generated_cleanup_paths_are_allowlisted() {
        let root = util::repo_root();
        assert!(safe_generated_path(&root, "target/coverage").is_ok());
        assert!(safe_generated_path(&root, ".git").is_err());
        assert!(safe_generated_path(&root, "target/../src").is_err());
    }
    #[test]
    fn release_commands_validate_options_before_spawning() {
        assert!(release_check(&["--scope".into(), "lsp".into()]).is_err());
        assert!(release_build(&["--target".into(), "unknown".into()]).is_err());
        assert!(prepare_release(&["--scope".into(), "extension".into()]).is_err());
    }
    #[test]
    fn coverage_rejects_traversal_before_using_optional_tool() {
        assert!(coverage(&["--output".into(), "target/coverage/../Cargo.toml".into()]).is_err());
        assert!(coverage(&["--output".into(), "/tmp/test.lcov".into()]).is_err());
    }
    #[test]
    fn grammar_candidate_input_is_validated_without_network() {
        assert!(update_grammar(&["--candidate".into(), "bad".into()]).is_err());
    }
}
