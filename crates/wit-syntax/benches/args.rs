//! Strict command-line parsing for our custom, harness-free WIT benchmark.
//! Cargo adds `--bench` when invoking custom benchmark harnesses.

const USAGE: &str = "Usage: cargo bench -p wit-syntax --bench parse -- [--iterations <count>]";

pub(crate) fn parse_iterations(args: &[String]) -> Result<usize, String> {
    let mut saw_bench = false;
    let mut options = Vec::with_capacity(args.len());

    for arg in args {
        if arg == "--bench" {
            if saw_bench {
                return Err(format!("duplicate --bench argument; {USAGE}"));
            }
            saw_bench = true;
        } else {
            options.push(arg.as_str());
        }
    }

    let iterations = match options.as_slice() {
        [] => 2_000,
        ["--iterations", count] => count
            .parse::<usize>()
            .map_err(|error| format!("invalid --iterations {count:?}: {error}; {USAGE}"))?,
        _ => {
            return Err(format!(
                "unexpected benchmark arguments: {options:?}; {USAGE}"
            ));
        }
    };

    if !(10..=1_000_000).contains(&iterations) {
        return Err(format!(
            "--iterations must be between 10 and 1000000; {USAGE}"
        ));
    }

    Ok(iterations)
}

#[cfg(test)]
mod tests {
    use super::parse_iterations;

    fn parse(args: &[&str]) -> Result<usize, String> {
        parse_iterations(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn cargo_custom_harness_invocations_are_supported() {
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
    fn rejects_duplicate_unknown_and_malformed_options() {
        for args in [
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
            assert!(parse(&args).is_err(), "unexpectedly accepted {args:?}");
        }
    }
}
