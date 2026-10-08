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
