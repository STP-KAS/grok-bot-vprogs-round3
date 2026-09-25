#!/bin/bash
# own vprog runner (release) on vprogs master; env overrides pass through
export RISC0_DEV_MODE=1 GD_MODE=${GD_MODE:-run} GD_ELF=/workspace/tmp/grok-deskfloor.elf GD_RATEFILE=/tmp/gd-rate
export RUST_LOG=${RUST_LOG:-info,vprogs_zk_vm=trace}
exec /workspace/tmp/bin/grok-desk-runner
