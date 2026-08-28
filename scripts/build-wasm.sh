#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
generated_dir="$project_dir/work/wasm"
published_dir="$project_dir/wasm"

# Rust panic locations can otherwise embed the builder's user name and absolute
# Cargo/Rustup paths in the published WebAssembly binary.
task_user_directory=${HOME:?HOME must be set to build WebAssembly}
task_cargo_directory=${CARGO_HOME:-"$task_user_directory/.cargo"}
task_rustup_directory=${RUSTUP_HOME:-"$task_user_directory/.rustup"}
rust_flag_separator=$(printf '\037')

append_encoded_rust_flag() {
  if [ -n "${CARGO_ENCODED_RUSTFLAGS:-}" ]; then
    CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS}${rust_flag_separator}$1"
  else
    CARGO_ENCODED_RUSTFLAGS=$1
  fi
}

append_encoded_rust_flag "--remap-path-prefix=$project_dir=/workspace"
append_encoded_rust_flag "--remap-path-prefix=$task_cargo_directory=/toolchain/cargo"
append_encoded_rust_flag "--remap-path-prefix=$task_rustup_directory=/toolchain/rustup"
append_encoded_rust_flag "--remap-path-prefix=$task_user_directory=/builder-home"
export CARGO_ENCODED_RUSTFLAGS

wasm-pack build "$project_dir/patch-core" \
  --target web \
  --out-dir "$generated_dir" \
  --release

mkdir -p "$published_dir"
for generated_name in \
  package.json \
  pc98_fat12_patcher_core.d.ts \
  pc98_fat12_patcher_core.js \
  pc98_fat12_patcher_core_bg.wasm \
  pc98_fat12_patcher_core_bg.wasm.d.ts
do
  cp "$generated_dir/$generated_name" "$published_dir/$generated_name"
done
