#!/bin/bash
# Local tic-tac-toe runner (exec mode, fresh lane) on n0. Key read from secure file, never printed.
cd /workspace/vprog-tictactoe
export TT_PRIVATE_KEY=$(python3 -c "import json;print(json.load(open('/home/box/secure/tn10-break-wallets.json'))['W2']['key'])")
export TT_WRPC_URL=${TT_WRPC_URL:-ws://127.0.0.1:17210} TT_NETWORK=tn10 TT_PROVE=0 RISC0_DEV_MODE=1
export TT_PROGRAM_ELF=/workspace/vprog-tictactoe/guest/compiled/program.elf
export TT_BATCH_ELF=/workspace/vprogs-rc/zk/backend/risc0/batch-processor/compiled/program.elf
export TT_AGGREGATOR_ELF=/workspace/vprogs-rc/zk/backend/risc0/batch-aggregator/compiled/program.elf
export TT_DATA_DIR=/workspace/tmp/ttd-data TT_DA_BIND=127.0.0.1:9880
export RUST_LOG=${RUST_LOG:-info}
exec /workspace/tmp/bin/ttd
