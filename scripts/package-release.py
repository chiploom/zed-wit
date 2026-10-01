import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

TARGETS = {
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
}


def package(target, output):
    root = Path(__file__).resolve().parents[1]
    suffix = ".exe" if "windows" in target else ""
    source = root / "target" / target / "release" / f"wit-language-server{suffix}"
    if not source.is_file() or source.stat().st_size == 0:
        raise ValueError(f"Build the release server for {target} before packaging")
    if source.stat().st_size > 128 * 1024 * 1024:
        raise ValueError("Release exceeds the adapter's 128 MiB binary size limit")
    manifests = [root / "Cargo.toml", root / "crates/wit-language-server/Cargo.toml", root / "extension.toml"]
    values = [tomllib.loads(path.read_text()) for path in manifests]
    versions = {values[0]["package"]["version"], values[1]["package"]["version"], values[2]["version"]}
    if len(versions) != 1:
        raise ValueError("Extension, adapter and native server versions differ")
    output.mkdir(parents=True, exist_ok=True)
    name = f"wit-language-server-{target}{suffix}"
    artifact = output / name
    if any((output / f"{name}{ending}").exists() for ending in ("", ".sha256", ".provenance.json")):
        raise ValueError(f"Refusing to overwrite release artifacts for {target}")
    revision = subprocess.run(["git", "rev-parse", "--verify", "HEAD"], cwd=root, text=True, capture_output=True)
    sha = revision.stdout.strip() if revision.returncode == 0 else None
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True))
    if os.environ.get("GITHUB_ACTIONS") == "true" and (dirty or sha is None):
        raise ValueError("Release CI checkout must be clean and committed")
    shutil.copy2(source, artifact)
    digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
    (output / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="utf-8")
    provenance = {
        "artifact": name,
        "sha256": digest,
        "version": next(iter(versions)),
        "target": target,
        "source_revision": sha,
        "source_dirty": dirty,
        "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
        "rustc": subprocess.check_output(["rustc", "--version", "--verbose"], cwd=root, text=True).strip(),
        "build_command": f"cargo build -p wit-language-server --release --locked --target {target}",
        "workflow_run": os.environ.get("GITHUB_RUN_ID"),
    }
    (output / f"{name}.provenance.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    print(artifact)


def main():
    parser = argparse.ArgumentParser(description="Package an already built WIT server with checksum and build metadata.")
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--output", type=Path, default=Path("dist"))
    args = parser.parse_args()
    try:
        package(args.target, args.output)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Packaging failed: {error}\n")


if __name__ == "__main__":
    main()
