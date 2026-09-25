#!/bin/bash
# Round3 00:02: disk watchdog during n0 utxoindex resync (TN10 UTXO set is huge; resync grows ~0.75 GB/min).
# <12G free: delete rc debug build /workspace/cargo-target (3.7G, rebuildable). <9.5G free while n0 has no RPC yet (still
# resyncing): SIGINT n0 (clean stop, protects the node from a full disk) and log. Exits when n0 RPC is up (resync done).
R=/workspace/tn10-break-test-2026-09-25/logs/storm/ramp.log
while sleep 5; do
  f=$(df --output=avail -B1M / | tail -1 | tr -dc 0-9); g=$((f/1024))
  if python3 /tmp/relqunch-upgrade/is-synced.py 18210 >/dev/null 2>&1 || timeout 2 bash -c '</dev/tcp/127.0.0.1/18210' 2>/dev/null; then
    echo "$(date '+%Y-%m-%dT%H:%M:%S%z') UTXOINDEX-WATCH n0 RPC up, watchdog exit (free ${g}G)" >> $R; exit 0; fi
  if [ $f -lt 12288 ] && [ -d /workspace/cargo-target ]; then rm -rf /workspace/cargo-target; echo "$(date '+%Y-%m-%dT%H:%M:%S%z') UTXOINDEX-WATCH free ${g}G -> deleted /workspace/cargo-target (rc debug build)" >> $R; fi
  if [ $f -lt 9728 ]; then
    P=$(cat /tmp/relqunch-fleet/tn10-n0.pid); kill -INT $P; echo "$(date '+%Y-%m-%dT%H:%M:%S%z') UTXOINDEX-WATCH free ${g}G < 9.5G during resync -> SIGINT n0 pid $P (disk protection)" >> $R; exit 1; fi
done
