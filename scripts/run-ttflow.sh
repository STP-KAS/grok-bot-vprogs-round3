#!/bin/bash
cd /workspace/vprog-tictactoe
export TTFLOW_PRIVATE_KEY=$(python3 -c "import json;print(json.load(open('/home/box/secure/tn10-break-wallets.json'))['W5']['key'])")
S=$(curl -s 127.0.0.1:9880/api/state)
export TT_COVENANT_ID=$(echo "$S" | python3 -c "import sys,json;print(json.load(sys.stdin)['covenant_id'])")
export TT_LANE_ID=${TT_LANE_ID:-130597584}
export TT_WRPC_URL=ws://127.0.0.1:17210 TT_NETWORK=tn10 RISC0_DEV_MODE=1 TT_DA_URL=http://127.0.0.1:9880
export TT_PROGRAM_ELF=/workspace/vprog-tictactoe/guest/compiled/program.elf RUST_LOG=${RUST_LOG:-info}
exec /workspace/tmp/bin/${BIN:-ttflow}
