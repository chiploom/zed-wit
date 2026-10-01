"""Check the locked dependency policy and native semantic parser boundary."""

import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc",
}
GRAMMAR_REV = "cdf07263b136054b413cab449ac7a1d059c27542"
# Explicitly reviewed expressions. Unknown expressions require a human decision.
LICENSE_CHOICES = {
    "MIT OR Apache-2.0": "MIT",
    "Apache-2.0 OR MIT": "MIT",
    "MIT/Apache-2.0": "MIT",
    "MIT": "MIT",
    "Apache-2.0": "Apache-2.0",
    "Unicode-3.0": "Unicode-3.0",
    "Zlib": "Zlib",
    "ISC": "ISC",
    "Unlicense OR MIT": "MIT",
    "MIT OR Zlib OR Apache-2.0": "MIT",
    "0BSD OR MIT OR Apache-2.0": "MIT",
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later": "MIT",
    "Apache-2.0 WITH LLVM-exception": "Apache-2.0 WITH LLVM-exception",
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT": "MIT",
    "(MIT OR Apache-2.0) AND Unicode-3.0": "MIT AND Unicode-3.0",
}


def metadata(target=None):
    command = ["cargo", "metadata", "--locked", "--format-version", "1"]
    if target:
        command.extend(["--filter-platform", target])
    return json.loads(subprocess.check_output(command, cwd=ROOT, text=True))


def native_packages(data):
    packages = {package["id"]: package for package in data["packages"]}
    nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
    roots = [p["id"] for p in packages.values() if p["name"] == "wit-language-server" and p["id"] in data["workspace_members"]]
    if len(roots) != 1:
        raise ValueError("Expected one workspace wit-language-server package")
    pending, seen = list(roots), set()
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        pending.extend(dep["pkg"] for dep in nodes[package_id]["deps"]
                       if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"]))
    return sorted((packages[key] for key in seen), key=lambda p: (p["name"], p["version"]))


def license_choice(package):
    expression = package.get("license")
    if expression not in LICENSE_CHOICES:
        raise ValueError(f"Unreviewed license for {package['name']} {package['version']}: {expression!r}")
    if package["name"] == "backtrace-ext" and package["version"] == "0.2.1":
        return "Apache-2.0"
    return LICENSE_CHOICES[expression]


def check(data):
    for package in data["packages"]:
        license_choice(package)
        source = package.get("source")
        if source and not source.startswith("registry+https://github.com/rust-lang/crates.io-index"):
            if package["name"] != "tree-sitter-wit" or not source.startswith("git+https://github.com/bytecodealliance/tree-sitter-wit?") or not source.endswith("#" + GRAMMAR_REV):
                raise ValueError(f"Unreviewed dependency source: {source}")
    native = native_packages(data)
    native_parsers = {p["version"] for p in native if p["name"] == "wit-parser"}
    if native_parsers != {"0.260.0"}:
        raise ValueError(f"Native semantic parser drift: {sorted(native_parsers)}")
    tree_sitters = {p["version"] for p in data["packages"] if p["name"] == "tree-sitter"}
    if tree_sitters != {"0.26.11"}:
        raise ValueError(f"Tree-sitter runtime drift: {sorted(tree_sitters)}")
    parser_versions = {p["version"] for p in data["packages"] if p["name"] == "wit-parser"}
    if parser_versions != {"0.227.1", "0.260.0"}:
        raise ValueError(f"Workspace parser versions changed; review adapter bindings: {sorted(parser_versions)}")
    return native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=sorted(TARGETS))
    args = parser.parse_args()
    try:
        data = metadata(args.target)
        native = check(data)
        print(json.dumps({
            "target": args.target or "all-platforms",
            "workspace_packages": len(data["packages"]),
            "native_and_build_packages": len(native),
            "native_wit_parser": "0.260.0",
            "adapter_binding_wit_parser": "0.227.1",
            "tree_sitter": "0.26.11",
            "licenses": sorted({license_choice(p) for p in data["packages"]}),
            "result": "passed",
        }, indent=2))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Dependency check failed: {error}\n")


if __name__ == "__main__":
    main()
