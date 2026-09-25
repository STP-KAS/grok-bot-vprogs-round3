//! grok-desk-runner: our own load runner for our own vprog guest `grok-deskfloor` (round 3).
//!
//! Modes (GD_MODE):
//!   addr - print the issuer / runner / fund addresses (never keys).
//!   fund - fan out GD_UTXOS_PER UTXOs of GD_UTXO_SOMPI to every issuer from the fund key.
//!   run  - start the vprogs runner engine (exec mode, fresh lane, our ELF), then issue moves from all
//!          issuers concurrently at GD_RATE moves/s total (live-tunable via GD_RATEFILE) for GD_SECS,
//!          then wait GD_DRAIN_SECS for execution and exit.
//! Keys: GD_WALLETS = JSON {name: {key, address}} (runner key GD_RUNNER_W, fund key GD_FUND_W) and
//! GD_ISSUERS = JSON {"keys": [hex, ...]}. Keys are never printed.
//! One log line per submitted move: `MOVE tag=.. kind=.. issuer=.. txid=.. build_ms=.. submit_ms=..`;
//! failures: `MOVEFAIL tag=.. kind=.. issuer=.. stage=.. err=..`. The engine's vprogs_zk_vm trace
//! `executed: ... data=<hex>` carries the move tag in the last 8 bytes (see the guest).

use std::{
    collections::HashSet,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use kaspa_consensus_core::{
    config::params::Params,
    constants::TX_VERSION_TOCCATA,
    network::{NetworkId, NetworkType},
    subnets::SubnetworkId,
};
use kaspa_wrpc_client::prelude::KaspaRpcClient;
use secp256k1::{Keypair, SecretKey};
use vprogs_core_test_utils::ResourceIdExt;
use vprogs_core_types::{AccessMetadata, ResourceId};
use vprogs_l1_wallet::{Wallet, encode_activity_payload};
use vprogs_runner::{Elfs, RunnerConfig, connect_wrpc, start_runner};
use vprogs_zk_backend_risc0_test_suite::{batch_aggregator_elf, batch_processor_elf};

const TN10: NetworkId = NetworkId::with_suffix(NetworkType::Testnet, 10);

fn env(k: &str, d: &str) -> String {
    std::env::var(k).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| d.to_string())
}
fn env_u64(k: &str, d: u64) -> u64 {
    env(k, &d.to_string()).parse().unwrap_or_else(|_| panic!("{k} must be u64"))
}
fn wallet_key(name: &str) -> SecretKey {
    let path = env("GD_WALLETS", "/home/box/secure/tn10-break-wallets.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read GD_WALLETS")).expect("json");
    SecretKey::from_str(v[name]["key"].as_str().unwrap_or_else(|| panic!("no key {name}")))
        .expect("secret key")
}
fn issuer_keys() -> Vec<SecretKey> {
    let path = env("GD_ISSUERS", "/home/box/secure/round3-issuers.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read GD_ISSUERS")).expect("json");
    let n = env_u64("GD_ISSUER_COUNT", 1000) as usize;
    v["keys"].as_array().expect("keys").iter().take(n).map(|k| SecretKey::from_str(k.as_str().unwrap()).expect("key")).collect()
}
fn kp(sk: &SecretKey) -> Keypair {
    Keypair::from_secret_key(secp256k1::SECP256K1, sk)
}
fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}
fn acct(a: u64) -> ResourceId {
    ResourceId::for_test(1 + a as usize)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    kaspa_core::log::try_init_logger(&env(
        "GD_LOG",
        "info,grok_desk_runner=info,vprogs_node_framework=info,vprogs_zk_vm=trace,risc0_zkvm=warn",
    ));
    let url = env("GD_WRPC_URL", "ws://127.0.0.1:17210");
    let params = Params::from(TN10);
    let client = connect_wrpc(&url, TN10).await;
    match env("GD_MODE", "run").as_str() {
        "addr" => {
            for (i, sk) in issuer_keys().iter().enumerate() {
                println!("issuer {i} {}", Wallet::new(&client, &params, kp(sk)).address());
            }
            for n in [env("GD_RUNNER_W", "W4"), env("GD_FUND_W", "W3")] {
                println!("{n} {}", Wallet::new(&client, &params, kp(&wallet_key(&n))).address());
            }
        }
        "fund" => fund(&client, &params).await,
        "run" => run(client, params).await,
        m => panic!("unknown GD_MODE {m}"),
    }
}

/// Fan out UTXOs to every issuer. After each payout, wait until the spent inputs disappear from the
/// fund wallet's UTXO set (change confirmed) so the next payout never double-spends.
async fn fund(client: &KaspaRpcClient, params: &Params) {
    let fund = Wallet::new(client, params, kp(&wallet_key(&env("GD_FUND_W", "W3"))));
    let (value, per) = (env_u64("GD_UTXO_SOMPI", 500_000_000), env_u64("GD_UTXOS_PER", 20) as usize);
    for (i, sk) in issuer_keys().iter().enumerate() {
        let addr = Wallet::new(client, params, kp(sk)).address().clone();
        let have = client_utxo_count(&Wallet::new(client, params, kp(sk))).await;
        if have >= per {
            log::info!("FUND issuer {i} already has {have} utxos, skip");
            continue;
        }
        for attempt in 0..20 {
            let tx = fund.pay_to_address(&addr, value, per).await;
            let spent: HashSet<_> = tx.inputs.iter().map(|x| x.previous_outpoint).collect();
            match fund.submit_transaction(&tx).await {
                Ok(id) => {
                    log::info!("FUND issuer {i} {addr} {per}x{value} tx {id}");
                    for _ in 0..120 {
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                        if let Ok(u) = fund.fetch_spendable_utxos().await {
                            if !u.iter().any(|(o, _)| spent.contains(o)) && !u.is_empty() {
                                break;
                            }
                        }
                    }
                    break;
                }
                Err(e) => {
                    log::warn!("FUND issuer {i} attempt {attempt} failed: {e}");
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                }
            }
        }
    }
    log::info!("FUND done");
}

async fn client_utxo_count(w: &Wallet<'_, KaspaRpcClient>) -> usize {
    w.fetch_spendable_utxos().await.map(|u| u.len()).unwrap_or(0)
}

struct Stats {
    issued: AtomicU64,
    fail_build: AtomicU64,
    fail_nofund: AtomicU64,
    fail_submit: AtomicU64,
}

async fn run(client: KaspaRpcClient, params: Params) {
    let program = std::fs::read(env("GD_ELF", "/workspace/tmp/grok-deskfloor.elf")).expect("read GD_ELF");
    let (batch, aggregator) = (batch_processor_elf(), batch_aggregator_elf());
    let elfs = Elfs { program: &program, batch: &batch, aggregator: &aggregator };
    let cfg = RunnerConfig {
        wrpc_url: env("GD_WRPC_URL", "ws://127.0.0.1:17210"),
        private_key: wallet_key(&env("GD_RUNNER_W", "W4")),
        network_id: TN10,
        program_elf: None,
        batch_elf: None,
        aggregator_elf: None,
        data_dir: env("GD_DATA_DIR", "/workspace/tmp/gd-data").into(),
        lane_id: None,
        covenant_id: None,
        bootstrap_txid: None,
        start_from: None,
        seed_depth: env_u64("GD_SEED_DEPTH", 500),
        prove: false,
        start_mode: None,
    };
    let t0 = Instant::now();
    let handles = start_runner(&cfg, &client, &params, elfs, |_| [0u8; 32])
        .await
        .unwrap_or_else(|e| panic!("runner start failed: {e}"));
    log::info!(
        "RUN started lane={} covenant={} startup_ms={}",
        handles.lane_id,
        handles.covenant_id,
        t0.elapsed().as_millis()
    );
    let _node = handles.node;
    let lane_subnet = handles.lane_subnet;

    let keys = issuer_keys();
    let n = keys.len() as u64;
    let accounts = env_u64("GD_ACCOUNTS", n.max(2));
    let secs = env_u64("GD_SECS", 600);
    let rate = Arc::new(AtomicU64::new(env_u64("GD_RATE", 20)));
    let stats = Arc::new(Stats {
        issued: AtomicU64::new(0),
        fail_build: AtomicU64::new(0),
        fail_nofund: AtomicU64::new(0),
        fail_submit: AtomicU64::new(0),
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    // live rate file
    {
        let rate = rate.clone();
        let f = env("GD_RATEFILE", "/tmp/gd-rate");
        tokio::spawn(async move {
            loop {
                if let Ok(s) = std::fs::read_to_string(&f) {
                    if let Ok(v) = s.trim().parse::<u64>() {
                        rate.store(v, Ordering::Relaxed);
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    {
        let stats = stats.clone();
        let rate = rate.clone();
        tokio::spawn(async move {
            let mut last = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let i = stats.issued.load(Ordering::Relaxed);
                log::info!(
                    "STATS issued={i} per_s={:.1} fail_build={} fail_nofund={} fail_submit={} rate_target={}",
                    (i - last) as f64 / 10.0,
                    stats.fail_build.load(Ordering::Relaxed),
                    stats.fail_nofund.load(Ordering::Relaxed),
                    stats.fail_submit.load(Ordering::Relaxed),
                    rate.load(Ordering::Relaxed)
                );
                last = i;
            }
        });
    }
    let mut tasks = Vec::new();
    for (i, sk) in keys.into_iter().enumerate() {
        let (client, params, stats, rate) = (client.clone(), params.clone(), stats.clone(), rate.clone());
        tasks.push(tokio::spawn(async move {
            issuer(i as u64, n, accounts, sk, client, params, lane_subnet, stats, rate, deadline).await
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    let drain = env_u64("GD_DRAIN_SECS", 90);
    log::info!("RUN issuing done after {}s; draining {drain}s", t0.elapsed().as_secs());
    tokio::time::sleep(Duration::from_secs(drain)).await;
    log::info!(
        "RUN END issued={} fail_build={} fail_nofund={} fail_submit={}",
        stats.issued.load(Ordering::Relaxed),
        stats.fail_build.load(Ordering::Relaxed),
        stats.fail_nofund.load(Ordering::Relaxed),
        stats.fail_submit.load(Ordering::Relaxed)
    );
    std::process::exit(0);
}

#[allow(clippy::too_many_arguments)]
async fn issuer(
    i: u64,
    n: u64,
    accounts: u64,
    sk: SecretKey,
    client: KaspaRpcClient,
    params: Params,
    lane_subnet: SubnetworkId,
    stats: Arc<Stats>,
    rate: Arc<AtomicU64>,
    deadline: Instant,
) {
    let wallet = Wallet::new(&client, &params, kp(&sk));
    let mut in_flight = HashSet::new();
    let home = i % accounts;
    let mut seq = 0u64;
    // stagger start
    tokio::time::sleep(Duration::from_millis(fastrand::u64(0..1000))).await;
    while Instant::now() < deadline {
        let r = rate.load(Ordering::Relaxed);
        if r == 0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        let period = Duration::from_micros(1_000_000 * n / r);
        let tick = Instant::now();
        seq += 1;
        let tag = (i << 48) | seq;
        let roll = fastrand::u64(0..100);
        let (kind, op, amount, other) = if roll < 45 {
            ("credit", 1u8, fastrand::u64(1..1000), None)
        } else if roll < 75 {
            ("debit", 2, fastrand::u64(1..300), None)
        } else if roll < 80 {
            ("floor", 0, fastrand::u64(0..200), None)
        } else if roll < 95 {
            let mut o = fastrand::u64(0..accounts);
            if o == home {
                o = (o + 1) % accounts;
            }
            ("move", 3, fastrand::u64(1..200), Some(o))
        } else {
            ("bad", 2, u64::MAX / 2, None)
        };
        let (meta, op) = match other {
            None => (vec![AccessMetadata::write(acct(home))], op),
            Some(o) if o > home => (vec![AccessMetadata::write(acct(home)), AccessMetadata::write(acct(o))], 3),
            Some(o) => (vec![AccessMetadata::write(acct(o)), AccessMetadata::write(acct(home))], 4),
        };
        let mut ix = vec![op];
        ix.extend_from_slice(&amount.to_le_bytes());
        ix.extend_from_slice(&tag.to_le_bytes());
        let payload = encode_activity_payload(&meta, &ix);
        let tb = Instant::now();
        let tx = match build_move(&wallet, &client, &params, &sk, payload, lane_subnet, &mut in_flight).await {
            Ok(Some(tx)) => tx,
            Ok(None) => {
                stats.fail_nofund.fetch_add(1, Ordering::Relaxed);
                log::warn!("MOVEFAIL tag={tag} kind={kind} issuer={i} stage=nofund in_flight={}", in_flight.len());
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(e) => {
                stats.fail_build.fetch_add(1, Ordering::Relaxed);
                log::warn!("MOVEFAIL tag={tag} kind={kind} issuer={i} stage=build err={e}");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        let build_ms = tb.elapsed().as_millis();
        let ts = Instant::now();
        let res = wallet.submit_transaction(&tx).await;
        in_flight.extend(tx.inputs.iter().map(|x| x.previous_outpoint));
        match res {
            Ok(id) => {
                stats.issued.fetch_add(1, Ordering::Relaxed);
                log::info!(
                    "MOVE tag={tag} kind={kind} issuer={i} txid={id} build_ms={build_ms} submit_ms={} t_ms={}",
                    ts.elapsed().as_millis(),
                    now_ms()
                );
            }
            Err(e) => {
                stats.fail_submit.fetch_add(1, Ordering::Relaxed);
                log::warn!("MOVEFAIL tag={tag} kind={kind} issuer={i} stage=submit err={e}");
            }
        }
        if let Some(rest) = period.checked_sub(tick.elapsed()) {
            tokio::time::sleep(rest).await;
        }
    }
}

/// Fee mode for moves (GD_FEE): `normal` = upstream `Wallet::build_activity_excluding` (the node's
/// sub-minute `normal_buckets[0]` feerate), `priority` = the estimator's priority bucket, `x<M>` =
/// normal bucket times M, or a plain number = fixed sompi/gram. Found in round 3: under a 10x-fee
/// flood the upstream `normal` choice (573 sompi/g) sat behind the flood (1000 sompi/g), every
/// issuer UTXO stayed in flight and moves stalled (fail_nofund).
async fn build_move(
    wallet: &Wallet<'_, KaspaRpcClient>,
    client: &KaspaRpcClient,
    params: &Params,
    sk: &SecretKey,
    payload: Vec<u8>,
    lane_subnet: SubnetworkId,
    in_flight: &mut HashSet<kaspa_consensus_core::tx::TransactionOutpoint>,
) -> Result<Option<kaspa_consensus_core::tx::Transaction>, kaspa_rpc_core::RpcError> {
    use kaspa_rpc_core::api::rpc::RpcApi;
    let mode = env("GD_FEE", "normal");
    if mode == "normal" {
        return wallet.build_activity_excluding(payload, lane_subnet, TX_VERSION_TOCCATA, in_flight).await;
    }
    let utxos = wallet.fetch_spendable_utxos().await?;
    let present: HashSet<_> = utxos.iter().map(|(o, _)| *o).collect();
    in_flight.retain(|o| present.contains(o));
    let candidates: Vec<_> = utxos.into_iter().filter(|(o, _)| !in_flight.contains(o)).collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let feerate = if let Ok(v) = mode.parse::<f64>() {
        v
    } else {
        let est = client.get_fee_estimate().await?;
        let normal = est.normal_buckets.first().map_or(est.priority_bucket.feerate, |b| b.feerate);
        if mode == "priority" {
            est.priority_bucket.feerate
        } else if let Some(m) = mode.strip_prefix('x') {
            normal * m.parse::<f64>().unwrap_or(1.0)
        } else {
            normal
        }
    };
    let address = wallet.address().clone();
    Ok(vprogs_l1_wallet::build::activity_transaction(vprogs_l1_wallet::build::ActivityTx {
        payload,
        candidates,
        keypair: kp(sk),
        address: &address,
        subnetwork_id: lane_subnet,
        tx_version: TX_VERSION_TOCCATA,
        fee_policy: vprogs_l1_wallet::build::FeePolicy::TargetFeerate(feerate),
        params,
    })
    .inspect_err(|e| log::warn!("activity funding failed: {e}"))
    .ok())
}
