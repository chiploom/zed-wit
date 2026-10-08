//! Exercise the benchmark argument parser in regular Cargo workspace tests.
//! The benchmark itself uses harness = false, so #[test] functions inside
//! benches/args.rs would otherwise be skipped by normal workspace test runs.
#[path = "../benches/args.rs"]
mod args;
