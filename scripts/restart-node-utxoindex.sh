#!/bin/bash
# Graceful restart of ONE TN10 node with its exact flags. Never touches node data.
# usage: restart-node.sh n0|n1
set -u
N=$1; case $N in n0) JP=18210;; n1) JP=18220;; *) exit 1;; esac
LOG=/workspace/tn10-break-test-2026-09-25/logs/restart-utxoindex-$N.log
PID=$(for p in $(ls /proc | grep -E '^[0-9]+$'); do a=$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null); case "$a" in /workspace/artifacts/kaspa-tn10/bin/kaspad*"--appdir=/tmp/kaspa-data-tn10-$N "*) echo $p;; esac; done | head -1)
[ -n "$PID" ] || { echo "no kaspad for $N"; exit 1; }
CMD=$(tr '\0' '\n' < /proc/$PID/cmdline); CWD=$(readlink /proc/$PID/cwd)
ts() { date +%s.%N | cut -c1-14; }
echo "$(date -Iseconds) pid=$PID cmd: $(echo $CMD)" >> $LOG
T0=$(ts); kill -INT $PID; echo "$(date -Iseconds) SIGINT sent" >> $LOG
for i in $(seq 1 600); do kill -0 $PID 2>/dev/null || break; sleep 0.5; done
kill -0 $PID 2>/dev/null && { echo "$(date -Iseconds) still alive after 300s, NOT forcing; abort" >> $LOG; exit 2; }
T1=$(ts); echo "$(date -Iseconds) exited after $(echo "$T1-$T0"|bc) s" >> $LOG
cd "$CWD"; [ -n "${ASYNC:-}" ] && CMD=$(echo "$CMD" | sed "s/^--async-threads=.*/--async-threads=$ASYNC/"); echo "$CMD" | grep -qx -- "--utxoindex" || CMD="$CMD"$'\n'"--utxoindex"; mapfile -t ARGS <<< "$CMD"
nohup "${ARGS[@]}" >> /tmp/kaspa-logs-tn10-$N/stdout.log 2>&1 < /dev/null &
NP=$!; echo $NP > /tmp/relqunch-fleet/tn10-$N.pid; T2=$(ts)
echo "$(date -Iseconds) started new pid=$NP (pid file updated)" >> $LOG
up=""; syn=""; tip=""
for i in $(seq 1 7200); do
  sleep 1
  r=$(/workspace/venv-grpc/bin/python - <<PY 2>/dev/null
import asyncio,json,time,websockets
async def m():
  async with websockets.connect("ws://127.0.0.1:$JP",open_timeout=2) as ws:
    async def c(i,mm,p={}):
      await ws.send(json.dumps({"id":i,"method":mm,"params":p}))
      while True:
        r=json.loads(await asyncio.wait_for(ws.recv(),5))
        if r.get("id")==i: return r.get("params") or {}
    s=await c(1,"getSyncStatus"); d=await c(2,"getBlockDagInfo"); b=await c(3,"getBlock",{"hash":d["sink"],"includeTransactions":False})
    print(int(bool(s.get("isSynced"))), round(time.time()-int(b["block"]["header"]["timestamp"])/1000,1), d["virtualDaaScore"])
asyncio.run(m())
PY
)
  [ -z "$r" ] && continue
  now=$(ts); set -- $r
  [ -z "$up" ] && { up=$(echo "$now-$T2"|bc); echo "$(date -Iseconds) RPC up after ${up}s (synced=$1 sink_age=$2 daa=$3)" >> $LOG; }
  [ -z "$syn" ] && [ "$1" = 1 ] && { syn=$(echo "$now-$T2"|bc); echo "$(date -Iseconds) isSynced after ${syn}s (sink_age=$2)" >> $LOG; }
  if [ -z "$tip" ] && [ "$1" = 1 ] && python3 -c "import sys; sys.exit(0 if float('$2')<5 else 1)"; then tip=$(echo "$now-$T2"|bc); echo "$(date -Iseconds) at tip (sink_age<5s) after ${tip}s, daa=$3" >> $LOG; break; fi
  [ $((i % 15)) = 0 ] && echo "$(date -Iseconds) waiting: synced=$1 sink_age=$2 daa=$3" >> $LOG
done
echo "$(date -Iseconds) summary: shutdown_s=$(echo "$T1-$T0"|bc) rpc_up_s=$up synced_s=$syn tip_s=$tip" >> $LOG
