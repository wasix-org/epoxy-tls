#!/usr/bin/env bash
set -euo pipefail

readonly ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly NATIVE_DIR="${ROOT_DIR}/.wasmer-build/native"

# Keep the production WASIX lockfile/registry settings intact. Sources are
# symlinked so native checks always exercise the current working tree.
python3 - "${ROOT_DIR}" "${NATIVE_DIR}" <<'PY'
from pathlib import Path
import shutil
import sys

root, native = map(Path, sys.argv[1:])
native.mkdir(parents=True, exist_ok=True)
manifest = (root / "Cargo.toml").read_text()
start = manifest.index("[patch.crates-io]")
end = manifest.index("[workspace.lints", start)
(native / "Cargo.toml").write_text(manifest[:start] + manifest[end:])
for name in ("server", "wisp"):
    package = native / name
    package.mkdir(exist_ok=True)
    shutil.copyfile(root / name / "Cargo.toml", package / "Cargo.toml")
    for entry in ("src", "build.rs"):
        source = root / name / entry
        destination = package / entry
        if source.exists() and not destination.exists():
            destination.symlink_to(source)
PY

# Cargo discovers .cargo/config from the cwd, even with --manifest-path.
# Run outside the repository to avoid its WASIX registry replacement.
cd "${TMPDIR:-/tmp}"
# Match the production lock: newer vergen 9.1 uses an incompatible vergen-lib
# with vergen-git2 1.0.7. This pin only affects the ignored native test workspace.
cargo +nightly update --manifest-path "${NATIVE_DIR}/Cargo.toml" -p vergen --precise 9.0.6
cargo +nightly test --manifest-path "${NATIVE_DIR}/Cargo.toml" -p epoxy-server keepalive -- "$@"
