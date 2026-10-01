"""Collect full native dependency and Rust standard-library redistribution notices."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location("dependency_policy", ROOT / "scripts/check-dependencies.py")
policy = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(policy)
NOTICE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")


def read_text(file):
    if not file.is_file() or not file.stat().st_size:
        raise ValueError(f"Missing or empty license text: {file}")
    return file.read_text(encoding="utf-8")


def package_notices(package):
    directory = Path(package["manifest_path"]).parent
    if package["id"].startswith("path+") and directory.is_relative_to(ROOT):
        return [(name, read_text(ROOT / name)) for name in ("LICENSE-MIT", "LICENSE-APACHE")]
    candidates = set()
    for candidate in directory.iterdir():
        if candidate.name.upper().startswith(NOTICE_PREFIXES):
            if candidate.is_dir():
                candidates.update(p for p in candidate.rglob("*") if p.is_file())
            elif candidate.is_file():
                candidates.add(candidate)
    if package.get("license_file"):
        explicit = (directory / package["license_file"]).resolve()
        if not explicit.is_relative_to(directory.resolve()):
            raise ValueError(f"License path leaves package: {package['name']}")
        candidates.add(explicit)
    if candidates:
        return [(str(p.relative_to(directory)), read_text(p)) for p in sorted(candidates)]
    vcs_file = directory / ".cargo_vcs_info.json"
    vcs = json.loads(read_text(vcs_file))["git"]["sha1"] if vcs_file.exists() else None
    if (package["name"], package["version"], vcs) == ("topiary-core", "0.7.3", "75ce8324ebaef45e00a964f110ed18ca3ed80235"):
        return [("upstream root LICENSE at " + vcs, read_text(ROOT / ".github/licenses/topiary-core-0.7.3/LICENSE"))]
    if (package["name"], package["version"], vcs) == ("backtrace-ext", "0.2.1", "043c95350875a36be6cd755dcef21a44a52ec2cc"):
        return [
            ("Cargo.toml.orig (upstream license declaration)", read_text(directory / "Cargo.toml.orig")),
            ("README.md (upstream attribution)", read_text(directory / "README.md")),
            ("Apache-2.0 canonical text; selected from declared MIT OR Apache-2.0", read_text(ROOT / "LICENSE-APACHE")),
        ]
    raise ValueError(f"No license files for {package['name']} {package['version']}; review upstream source")


def render(target, data, sysroot):
    packages = policy.check(data)
    rust_docs = sysroot / "share/doc/rust"
    standard_library = read_text(rust_docs / "COPYRIGHT-library.html")
    license_dir = rust_docs / "licenses"
    if not license_dir.is_dir():
        raise ValueError("Pinned Rust distribution is missing its license directory")
    sections = [
        "WIT language server redistribution notices",
        f"Target: {target}",
        "Cargo.lock SHA256: " + hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "Scope: target-filtered native normal/build dependency closure; dev-only edges excluded.",
        "Build dependencies and workspace feature unification may conservatively include code not linked into the executable.",
        "License choices below preserve every AND obligation. Other offered alternatives are not required by these choices.",
        "Platform system libraries supplied by the operating system are not bundled by this project.",
        "\nPROJECT AND DIRECTLY COPIED MATERIAL\n" + read_text(ROOT / "THIRD_PARTY_NOTICES.md"),
    ]
    for package in packages:
        sections.extend([
            "\n" + "=" * 78,
            f"PACKAGE: {package['name']} {package['version']}",
            "Source: " + (package.get("source") or "workspace: https://github.com/chiploom/zed-wit"),
            "Repository: " + (package.get("repository") or "not declared"),
            "Authors: " + ", ".join(package.get("authors", [])),
            "Declared license: " + package["license"],
            "Selected license: " + policy.license_choice(package),
        ])
        for name, text in package_notices(package):
            sections.extend(["\n--- " + name + " ---\n", text])
    sections.extend([
        "\n" + "=" * 78,
        "RUST STANDARD LIBRARY: full notices from the pinned toolchain distribution",
        "The standard library offers MIT OR Apache-2.0; retain the embedded third-party notices and exceptions below.",
        "COPYRIGHT-library.html (complete upstream document):\n" + standard_library,
    ])
    license_files = sorted(p for p in license_dir.iterdir() if p.is_file())
    if not license_files:
        raise ValueError("Pinned Rust distribution has no standard-library license texts")
    for file in license_files:
        sections.extend(["\n--- Rust distribution licenses/" + file.name + " ---\n", read_text(file)])
    return "\n".join(sections) + "\n", len(packages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=sorted(policy.TARGETS))
    parser.add_argument("--output", type=Path, default=Path("dist"))
    args = parser.parse_args()
    try:
        sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], cwd=ROOT, text=True).strip())
        content, count = render(args.target, policy.metadata(args.target), sysroot)
        suffix = ".exe" if "windows" in args.target else ""
        args.output.mkdir(parents=True, exist_ok=True)
        output = args.output / f"wit-language-server-{args.target}{suffix}.licenses.txt"
        with output.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(content)
        print(json.dumps({"target": args.target, "packages": count, "output": str(output), "bytes": output.stat().st_size}))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"License collection failed: {error}\n")


if __name__ == "__main__":
    main()
