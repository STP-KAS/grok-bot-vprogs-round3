# Draft upstream issues (not filed)

Written from the round 3 TN10 test on 25-26 Sep 2026. Each one is a draft for the maintainers to review before anything is filed.

1. **vprogs `master` is stale.** master f9b84a8 (2026-07-28) is 142 commits behind `release-candidate` 3a61c0b (2026-09-24), and rc contains all of master. Downstream apps (vprog-tictactoe) pin a branch, `fix/reorg-boundary-duplicate-bundles`, that points at the rc head. A new builder who clones master gets an API two months old. Suggestion: fast-forward master, or document which branch is canonical.
2. **No `rust-toolchain.toml`, MSRV or protoc docs.** With distro cargo 1.85 the build fails (kaspa 2.0.1 needs rust >= 1.91). `PROTOC` must be set by hand. The risc0 guest needs the rzup toolchain. None of this is documented.
3. **Guest ELFs are not reproducible locally.** Locally built batch-processor and aggregator ELFs are not byte-identical to the committed docker ELFs. That makes it unclear which image IDs a local build commits to.
4. **The fee policy starves under a flood.** `Wallet` (vprogs_l1_wallet) always uses `normal_buckets[0]`. Under a 1000 sompi/g flood that bucket was 532-573 sompi/g, so every vprog activity tx waited behind the flood and the issuer ran out of spendable UTXOs. There is no fee bump, RBF or cap option. Measured:
   - normal fee: exec latency p50 83 s, p90 101 s, 4,871 no-funds failures in about 5 min;
   - `priority_bucket`: about 4.5k sompi/g, 4.5x the flood fee. It works (p50 3.1 s at 130 moves/s) but burned about 900 TKAS in 11.5 min;
   - 2x normal: about 30 moves/s, p50 9.5 s.

   Suggestion: a configurable fee strategy (fixed, multiplier, capped priority) and RBF bumping for stuck activity txs.
5. **The carrier path reuses UTXOs that are already spent in the mempool** (vprogs_l1_wallet plus vprog-tictactoe). The Wallet has `*_excluding` variants (`build_activity_excluding`, `prepare_settlement_excluding`), but `build_signed_carrier` and `pay_to_address`, which ttflow's scenario uses, have no in-flight exclusion. The ttflow scenario relies on a fixed 2 s step delay:
   - step delay 0: 12 of 14 games failed;
   - at 1k TPS background: 251 of 670 games failed ("already spent ... in the mempool");
   - under the full flood: 1,322 of 1,327 games failed.

   Suggestion: track in-flight outpoints inside the Wallet, or chain on the unconfirmed change output.
6. **3 RPCs per activity tx.** Each move costs a UTXO fetch, a fee estimate and a submit. At 130 moves/s that is about 400 RPC/s from one issuer process, which contributes to the CPU ceiling.
7. **kaspad `--utxoindex` resync blocks startup with no progress output.** Enabling utxoindex on an existing tn10 datadir took 1115 s (18m35s). RPC and P2P were down the whole time, with no progress log. It needed about 13 GB extra disk (disk went 40G -> ~11G free, and would have hit the floor on a smaller box). vprogs needs utxoindex, so a preflight check in the vprogs node or the docs should warn about it.
8. **Carrier builder panics instead of returning an error when funds are fragmented.** `vprogs_l1_wallet/src/build/carrier.rs:62` asserts that one funding UTXO covers the extra outputs: `funding UTXO amount 98955500 too small for extra outputs 100000000`. After about 170 games, each 5-TKAS UTXO had split into sub-1-TKAS pieces. The builder does not combine inputs; it panics the tokio worker, and all 8 ttloop-c workers died at 01:07. Suggestion: select multiple inputs, or return `Err`.
