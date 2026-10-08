//! Deterministic, dependency-free Tree-sitter WIT parser microbenchmark.
//! Results are local measurements, not cross-machine performance guarantees.
mod args;

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
    let iterations = args::parse_iterations(&env::args().skip(1).collect::<Vec<_>>())
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        });
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wit_syntax::language())
        .expect("pinned WIT grammar");
    let source = SOURCE.as_bytes();
    for _ in 0..100 {
        let tree = parser
            .parse(black_box(source), None)
            .expect("parser returned no tree");
        assert!(!tree.root_node().has_error(), "invalid benchmark WIT input");
        black_box(tree);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(
            parser
                .parse(black_box(source), None)
                .expect("parser returned no tree"),
        );
    }
    let nanos = start.elapsed().as_nanos() / iterations as u128;
    println!(
        "wit-syntax Tree-sitter parse: {iterations} iterations, {nanos} ns/parse, {} input bytes",
        source.len()
    );
}
