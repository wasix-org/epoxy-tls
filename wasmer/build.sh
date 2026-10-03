#!/usr/bin/env bash
set -euo pipefail

readonly ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly MIO_REPOSITORY="https://github.com/wasix-org/mio.git"
readonly MIO_REVISION="1aea074c67eeeb98ac229660971249579693f248"
readonly MIO_DIR="${ROOT_DIR}/.wasmer-build/mio"
readonly MIO_PATCH="${ROOT_DIR}/wasmer/patches/mio-wasix-nonblocking.patch"
readonly OUTPUT_DIR="${ROOT_DIR}/wasmer/modules"
readonly WASM_TOOLS="${WASM_TOOLS:-wasm-tools}"

if ! cargo wasix --version >/dev/null 2>&1; then
  echo "cargo-wasix is required; install it with: cargo install cargo-wasix" >&2
  exit 1
fi

if ! command -v "${WASM_TOOLS}" >/dev/null 2>&1; then
  echo "wasm-tools is required; install it with: cargo install wasm-tools" >&2
  exit 1
fi

if [[ ! -d "${MIO_DIR}/.git" ]]; then
  mkdir -p "$(dirname "${MIO_DIR}")"
  git clone --filter=blob:none --no-checkout "${MIO_REPOSITORY}" "${MIO_DIR}"
  git -C "${MIO_DIR}" fetch --depth 1 origin "${MIO_REVISION}"
  git -C "${MIO_DIR}" checkout --detach "${MIO_REVISION}"
fi

if [[ "$(git -C "${MIO_DIR}" rev-parse HEAD)" != "${MIO_REVISION}" ]]; then
  echo "${MIO_DIR} is not pinned to ${MIO_REVISION}" >&2
  exit 1
fi

if git -C "${MIO_DIR}" apply --unidiff-zero --reverse --check "${MIO_PATCH}" >/dev/null 2>&1; then
  : # The non-blocking WASIX socket patch is already present.
elif git -C "${MIO_DIR}" diff --quiet && git -C "${MIO_DIR}" diff --cached --quiet; then
  git -C "${MIO_DIR}" apply --unidiff-zero "${MIO_PATCH}"
else
  echo "${MIO_DIR} contains unexpected changes; refusing to overwrite them" >&2
  exit 1
fi

(
  cd "${ROOT_DIR}"
  # Keep cargo-wasix's default features when the caller supplies no flags.
  # Append the restriction last, including when encoded flags take precedence.
  export RUSTFLAGS="${RUSTFLAGS:--C target-feature=+atomics,+simd128,+relaxed-simd,+extended-const} -C target-feature=-wide-arithmetic"
  if [[ -n "${CARGO_ENCODED_RUSTFLAGS+x}" ]]; then
    if [[ -n "${CARGO_ENCODED_RUSTFLAGS}" ]]; then
      CARGO_ENCODED_RUSTFLAGS+=$'\x1f'
    fi
    export CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS}"'-C'$'\x1f''target-feature=-wide-arithmetic'
  fi
  cargo wasix build --release --package epoxy-server --locked
)

# Check actual instructions after cargo-wasix's optimization, including code
# linked from the sysroot and C dependencies. Do not replace a working package
# if a toolchain or optimizer starts emitting wide arithmetic again.
"${WASM_TOOLS}" validate --features=all,-wide-arithmetic \
  "${ROOT_DIR}/target/wasm32-wasmer-wasi/release/epoxy-server.wasm"

mkdir -p "${OUTPUT_DIR}"
cp \
  "${ROOT_DIR}/target/wasm32-wasmer-wasi/release/epoxy-server.wasm" \
  "${OUTPUT_DIR}/wisp-proxy.wasm"

echo "Built ${OUTPUT_DIR}/wisp-proxy.wasm"
