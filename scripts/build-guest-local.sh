#!/bin/bash
# Build a vprogs RISC0 guest locally (no Docker) with the rzup risc0 toolchain, same flags as zk/backend/risc0/build-guests.sh.
# usage: build-guest-local.sh <manifest-path> <bin-name> <out.elf>
set -euo pipefail
export PATH=$HOME/.cargo/bin:$HOME/.risc0/bin:$PATH CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/cargo-target-guest} RISC0_FEATURE_bigint2=
export RUSTFLAGS='-Cpasses=lower-atomic -Clink-arg=-Ttext=0x00200800 -Clink-arg=--fatal-warnings -Cpanic=abort --cfg getrandom_backend="custom"'
cargo +risc0 build -j2 --release ${LOCKED---locked} --target riscv32im-risc0-zkvm-elf --manifest-path "$1"
cp "$CARGO_TARGET_DIR/riscv32im-risc0-zkvm-elf/release/$2" "$3"; ls -la "$3"; sha256sum "$3"
