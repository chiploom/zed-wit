//! Exercise the benchmark argument parser in ordinary workspace tests.
//! Cargo compiles the custom benchmark target with test cfg, but it has
//! harness = false and cannot execute #[test] functions itself.
#[path = "../benches/args.rs"]
mod args;

fn parse(input: &[&str]) -> Result<usize, String> {
    args::parse_iterations(
        &input
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn accepts_cargo_custom_harness_arguments() {
    assert_eq!(parse(&[]).unwrap(), 2_000);
    assert_eq!(parse(&["--bench"]).unwrap(), 2_000);
    assert_eq!(parse(&["--iterations", "2000"]).unwrap(), 2_000);
    assert_eq!(parse(&["--bench", "--iterations", "123"]).unwrap(), 123);
    assert_eq!(parse(&["--iterations", "456", "--bench"]).unwrap(), 456);
    assert_eq!(parse(&["--iterations", "10", "--bench"]).unwrap(), 10);
    assert_eq!(
        parse(&["--bench", "--iterations", "1000000"]).unwrap(),
        1_000_000
    );
}

#[test]
fn rejects_duplicates_unknown_options_and_invalid_iteration_counts() {
    for input in [
        vec!["--bench", "--bench"],
        vec!["--iterations"],
        vec!["--iterations", "abc"],
        vec!["--iterations", "0"],
        vec!["--iterations", "9"],
        vec!["--iterations", "1000001"],
        vec!["--iterations", "20", "--iterations", "30"],
        vec!["--bench", "--unknown"],
        vec!["--iterations", "20", "--unknown"],
    ] {
        assert!(parse(&input).is_err(), "unexpectedly accepted {input:?}");
    }
}
