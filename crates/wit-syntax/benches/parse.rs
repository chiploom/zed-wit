//! Deterministic, dependency-free Tree-sitter WIT parser microbenchmark.
//! Results are local measurements, not cross-machine performance guarantees.
use std::{env, hint::black_box, time::Instant};

const SOURCE: &str = r#"package chiploom:bench@0.1.0;

interface geometry {
    record point {
        x: u32,
        y: u32,
    }
    translate: func(input: point) -> point;
}

world preview {
    export geometry;
}
"#;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let iterations = match args.as_slice() {
        [] => 2000,
        [flag, count] if flag == "--iterations" => count.parse::<usize>()
            .expect("--iterations requires a positive integer"),
        _ => panic!("Usage: cargo bench -p wit-syntax --bench parse -- --iterations <count>"),
    };
    assert!((10..=1_000_000).contains(&iterations), "iterations must be 10..=1000000");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&wit_syntax::language()).expect("pinned WIT grammar");
    let source = SOURCE.as_bytes();
    for _ in 0..100 {
        let tree = parser.parse(black_box(source), None).expect("parser returned no tree");
        assert!(!tree.root_node().has_error(), "invalid benchmark WIT input");
        black_box(tree);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(parser.parse(black_box(source), None).expect("parser returned no tree"));
    }
    let nanos = start.elapsed().as_nanos() / iterations as u128;
    println!("wit-syntax Tree-sitter parse: {iterations} iterations, {nanos} ns/parse, {} input bytes", source.len());
}
