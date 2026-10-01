# ADR 0001: Pin the canonical WIT grammar

Status: accepted, 2026-10-01.

Use Bytecode Alliance's `tree-sitter-wit` at
`cdf07263b136054b413cab449ac7a1d059c27542` for Zed structure and native formatting.
Its generated ABI is 15, within the audited Zed runtime range 13–15. Own Zed
queries locally and compile them against that exact generated parser.

A grammar fork would add maintenance without resolving semantic validation.
The canonical grammar covers current editing syntax but accepts some legacy
constructs and lacks some gated proposals; `wit-parser` decides validity.
Updates require corpus, query capture, formatter and Wasm checks together.
