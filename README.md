# grok-bot-vprogs round 3: vprogs under a TN10 transaction storm

Private report by Grok (acting for stp), 25-26 Sep 2026 (times are CEST).
Related reports: [grok-bot-vprogs](https://github.com/STP-KAS/grok-bot-vprogs) (branch `tn10-break-report`) and [grok-bot-vprogs-round2](https://github.com/STP-KAS/grok-bot-vprogs-round2).

## What
- Restarted our TN10 node n0 with `--utxoindex`, which vprogs needs, and measured the cost.
- Wrote and ran our **own vprog** on **vprogs master** (f9b84a8). It has a new RISC0 guest, `grok-deskfloor` (account = `[bal|floor|ops|last_tag]`; ops SetFloor, Credit, Debit with a floor rule, and Move), plus a release load runner, `grok-desk-runner`.
- Ran the **tic-tac-toe** vprog locally: `ttd` in exec mode on a fresh lane on n0, plus our `ttloop`, which plays full games in parallel and waits for the DA to report a final state.
- Did all of this while our own storm was hammering TN10 (1k TPS background, or full throttle at about 4.4-5.6k tx/s accepted), at a 10x fee (about 1000 sompi/g).

## Why
To find out whether a vprog stays usable when L1 is flooded by higher-fee traffic, which failure modes appear, and what they cost.

## How
- Node: rusty-kaspa kaspad on tn10, single node n0, 8 cores, 16 GB RAM.
- Storm: `tps-supervisor.py` + `rwstorm.mjs` (round 2 tooling), with guards: mempool hard brake at 90k, disk floor 8G, RAM floor 1G.
- Own vprog: `guest/grok-deskfloor` and `runner/grok-desk-runner`.
  - The runner opens a fresh dev covenant and lane (`RISC0_DEV_MODE=1`, exec-only, no proofs). 16 issuer keys each hold 20 UTXOs.
  - Move kinds: credit, debit, floor, move, and `bad`, a deliberately impossible debit that must never execute.
  - Latency = submit to the `vprogs_zk_vm` `executed:` trace, matched by an 8-byte tag. Analysis: `scripts/analyze-gd.py`.
  - `GD_FEE` = normal (upstream Wallet: `normal_buckets[0]`) | priority | x&lt;M&gt; | fixed sompi/g.
- tic-tac-toe: `vprog-tictactoe` at its pinned vprogs branch (= release-candidate 3a61c0b).
  - `ttd` runs with TT_PROVE=0. `ttflow` ran Init, then `ttloop` (`tictactoe/`) ran 12 workers, each with its own funded key, playing one game at a time.

## Timeline (CEST)
| time | event |
|---|---|
| 23:52:21 | storm HALT, mempool drained 42k -> 0 in about 40 s |
| 23:52:39 | n0 SIGINT (clean exit in 1.5 s), restarted with `--utxoindex` |
| 23:52:42-00:11:17 | utxoindex resync 1115 s, RPC and P2P down, no progress output |
| 00:11:52 | n0 at tip again (catch-up 35 s). Node downtime **1153 s** |
| 00:13-00:15 | storm back at 1k, one clipped 2x burst (mempool max 84k, drained in 12 s) |
| 00:15:37 | storm at full throttle, 10x fee |
| 00:23-00:28 | own vprog t2 (debug build, normal fee) under the 10x storm: moves starve |
| 00:32-00:38 | storm silently stopped: monitor RAM guard tripped during parallel release builds (see Flaws) |
| 00:38 | storm set to 1k background ("vprogs full gusto") |
| 00:42:44 | `ttd` fresh lane 130597584. 00:43:21 `ttflow` Init plus a scripted match, all executed |
| 00:45-00:54 | gd-r1 (priority) ramp 20 -> 160 moves/s. ttloop 4 + 8 workers |
| 00:54:35-00:58:50 | contention 1: storm full + gd-r1 priority + ttloop |
| 01:00:26-01:06:52 | contention 2: gd-r2 x2 fee, storm full from 01:01:40 |
| 01:10 | continuous: gd-r3 (x2, 60/s) + ttloop at 1k storm background |

## Results
### n0 `--utxoindex` restart
Downtime 1153 s, of which 1115 s was the utxoindex resync. The utxoindex dir is **13 GB**, and free disk dropped from 40G to about 11G during the resync. No mempool loss, because we drained first.

### Own vprog (`grok-deskfloor`, vprogs master, release)
| run | L1 load | fee | moves/s (issued) | exec p50 / p90 / p99 | fails | notes |
|---|---|---|---|---|---|---|
| t1 (debug) | ~1k | normal 573 | 6 (10 target) | ~25 s (warm-up) | 0 | bad 0/19 executed |
| t2 (debug) | full 10x | normal 573 | ~8 (20 target) | 83 s / 101 s / - | 4,871 nofund | moves starve below the storm fee |
| **r1** | 1k bg | priority | **130-138** (CPU ceiling) | **3.1 s / 4.1 s / 5.6 s** | 0 at 1k | 56,145 submitted, 53,271 executed, bad 0/2,778 executed |
| r1 | full 10x | priority (~4.5k sompi/g) | 100-130 for ~50 s, then 0 | - | 3,436 nofund | issuers went broke: about 900 TKAS burned in 11.5 min |
| **r2** | full 10x | x2 (~1.1k) | **22-34** (80 target) | **9.5 s / 18.6 s / 23 s** | 5,953 nofund | UTXOs sit in flight |

- The ceiling at 1k background is CPU: runner about 390%, ttd about 240%, kaspad plus miners on 8 cores, load about 24.
- Floor rule correct: no `bad` move ever executed (0 of 3,518 across r1 and r2).

### tic-tac-toe (local ttd, 12 ttloop workers)
| L1 load | games finished | GAME_FAIL | game time p50 / p90 | timeouts |
|---|---|---|---|---|
| 1k bg | 419 | 251 | 14.1 s / 16.1 s (7 steps x 2 s delay) | 0 |
| full 10x | **5** | **1,322** | 15.1 s | 0 |

- Every game whose txs landed was reported final by the DA within 0-2 s of the last step.
- All failures come from one cause: `already spent by transaction ... in the mempool`. ttflow's Wallet pays the normal bucket fee (532 sompi/g), which is below the storm fee. The previous step's tx stays in the mempool, and the next step picks the same UTXO again.
- With step delay 0, 12 of 14 games failed even at low load.
- At 01:07 all 8 ttloop-c workers panicked in `carrier.rs:62` because their funding UTXOs had fragmented below the 1 TKAS deposit (upstream issue 8). They restarted as ttloop-d with a 0.4 TKAS deposit and 0.2 stake.
- Final totals are in `logs/ttloop-*.games.log`.

## Conclusion
- The vprogs execution path held up: bridge reorgs were handled, the floor rule was never violated, execution latency was about 3 s at 130 moves/s, and the DA was final within 2 s.
- The weak point is the **L1 fee and UTXO layer of the client tooling**:
  - the default fee always loses to a flood;
  - "priority" wins but costs 4.5x the flood fee;
  - the carrier path reuses UTXOs that are still in the mempool.
- An attacker paying about 1000 sompi/g can stall default-configured vprog clients (tic-tac-toe: 5 games in about 10 min of full storm). A client that pays more keeps working, at a cost set by the attacker.

## Flaws
Ours:
- The monitor STOP file is sticky. A RAM dip to 753 MB during two parallel release builds stopped the storm at 00:32 without notice, and supervisor.jsonl kept logging the last accepted TPS (about 2.3k) for 6 minutes. The same trap fired earlier on tip age during catch-up.
- Before our fix, `rwstorm.mjs` `token()` looped forever at rate 0, so HALT was ignored.
- `tps-supervisor.py` auto-restart dropped `--utxoindex`.
- ttloop run A used step delay 0, and ttloop pays the default fee (no fee knob yet).
- r2 was killed before its drain finished, so its not-executed tail is overstated.
- Single node. There was an external sender (another agent) on tn10, and its traffic is not separated out here.

Upstream: see [`upstream-issues/README.md`](upstream-issues/README.md). In short: stale master, no MSRV/toolchain/protoc docs, non-reproducible ELFs, a fee policy with no bump/RBF, UTXO reuse on the carrier path, 3 RPCs per tx, and a utxoindex resync with no progress output, and a carrier-builder panic on fragmented UTXOs.

## Ideas
- Fee strategy in `vprogs_l1_wallet`: capped priority (e.g. min(priority, k x normal)), plus an RBF bump after N seconds in the mempool.
- In-flight UTXO tracking for the carrier path, or chaining on the unconfirmed change output.
- Batch several activity ixs per L1 tx to spread the fee under floods.
- A `ttd` `/api/metrics` endpoint (exec lag, pending bundles).

## Reproduce
1. kaspad tn10 with `--utxoindex`. Budget about 20 min and 13 GB for the first index build.
2. vprogs master plus `patches/vprogs-master-workspace.diff`; copy `guest/grok-deskfloor` to `zk/backend/risc0/` and `runner/grok-desk-runner` to `examples/`.
   - Toolchain: rustup >= 1.91 (we used 1.94), rzup risc0, and `PROTOC=/usr/bin/protoc`.
   - Guest: `scripts/build-guest-local.sh`. Runner: `cargo build --release -p grok-desk-runner`.
3. Fund issuers with `GD_MODE=fund`, then run with `scripts/run-gd.sh` (GD_FEE, GD_RATE, GD_ISSUER_COUNT, `/tmp/gd-rate` for live rate changes). Analyze with `scripts/analyze-gd.py LOG out.json`.
4. tic-tac-toe: apply `tictactoe/ttloop-scenario.diff`, add `ttloop.rs` to `driver/src/bin/`, build `-p vprog-tictactoe-node -p vprog-tictactoe-driver --release`. Then `scripts/run-ttd.sh`, `scripts/run-ttflow.sh` (Init), and `BIN=ttloop scripts/run-ttflow.sh`.

Keys live outside the repo in 0600 files. No keys or secrets are in this repo.
