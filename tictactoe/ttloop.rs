//! ttloop (grok-bot round 3, 2026-09-26): local tic-tac-toe load runner. Plays complete games on an
//! initialized lane from several operator keys in parallel (one game at a time per key), then polls
//! the local DA (`ttd`) until each game reaches a final state (First/Second/Draw).
//! Env: the usual TT_* / TTFLOW_* (see docs/demo/reference.md) except TTFLOW_PRIVATE_KEY, plus
//! TTLOOP_KEYS (JSON {"keys":[hex..]}), TTLOOP_KEY_OFFSET, TTLOOP_WORKERS, TTLOOP_SECS,
//! TTLOOP_FINISH_TIMEOUT_S, TT_DA_ADDR (host:port, default 127.0.0.1:9880). Keys are never printed.
use std::{sync::Arc, time::{Duration, Instant}};

use kaspa_consensus_core::config::params::Params;
use kaspa_wrpc_client::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vprog_tictactoe_driver::{config::Config, scenario};

fn env(k: &str, d: &str) -> String {
    std::env::var(k).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| d.to_string())
}

async fn http_get(addr: &str, path: &str) -> Option<String> {
    let mut s = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(addr)).await.ok()?.ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes()).await.ok()?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await.ok()?.ok()?;
    let txt = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = txt.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") { return None; }
    Some(body.to_string())
}

fn state_of(body: &str) -> Option<String> {
    let i = body.find("\"state_name\":\"")? + 14;
    Some(body[i..].split('"').next()?.to_string())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    kaspa_core::log::try_init_logger("info,ttloop=info,vprog_tictactoe_driver=warn,vprogs_l1_wallet=warn,vprogs_zk_backend_risc0_app_kit=warn");
    let keys: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(env("TTLOOP_KEYS", "/home/box/secure/round3-issuers.json")).unwrap()).unwrap();
    let keys: Vec<String> = keys["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_string()).collect();
    let off: usize = env("TTLOOP_KEY_OFFSET", "16").parse().unwrap();
    let workers: usize = env("TTLOOP_WORKERS", "4").parse().unwrap();
    let secs: u64 = env("TTLOOP_SECS", "3600").parse().unwrap();
    let fin_to: u64 = env("TTLOOP_FINISH_TIMEOUT_S", "300").parse().unwrap();
    let da = env("TT_DA_ADDR", "127.0.0.1:9880");
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut tasks = Vec::new();
    for w in 0..workers {
        let key = keys[off + w].clone();
        let da = da.clone();
        tasks.push(tokio::spawn(async move {
            let cfg = Config::from_lookup(|k| if k == "TTFLOW_PRIVATE_KEY" { Some(key.clone()) } else { std::env::var(k).ok().filter(|s| !s.is_empty()) });
            let params = Params::from(cfg.network_id);
            let client = KaspaRpcClient::new_with_args(WrpcEncoding::Borsh, Some(&cfg.wrpc_url), None, Some(cfg.network_id), None).expect("client");
            client.connect(Some(ConnectOptions { block_async_connect: true, connect_timeout: Some(Duration::from_millis(10_000)), ..Default::default() })).await.expect("connect");
            let client = Arc::new(client);
            let mut n = 0u64;
            while Instant::now() < deadline {
                n += 1;
                let t0 = Instant::now();
                match scenario::play_one(&client, &params, &cfg).await {
                    Ok((gid, steps)) => {
                        let id = faster_hex::hex_string(gid.as_slice());
                        let st: Vec<String> = steps.iter().map(|(k, v)| format!("{k}={v}")).collect();
                        let submitted_ms = t0.elapsed().as_millis();
                        log::info!("GAME_SUBMITTED worker={w} n={n} id={id} total_submit_ms={submitted_ms} {}", st.join(" "));
                        let tf = Instant::now();
                        let mut last = String::from("none");
                        let mut done = false;
                        while tf.elapsed() < Duration::from_secs(fin_to) {
                            if let Some(b) = http_get(&da, &format!("/api/games/{id}")).await {
                                if let Some(s) = state_of(&b) { last = s; }
                                if matches!(last.as_str(), "First" | "Second" | "Draw") { done = true; break; }
                            }
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                        }
                        if done {
                            log::info!("GAME_FINISHED worker={w} n={n} id={id} state={last} finish_wait_ms={} game_ms={}", tf.elapsed().as_millis(), t0.elapsed().as_millis());
                        } else {
                            log::warn!("GAME_TIMEOUT worker={w} n={n} id={id} last_state={last} waited_s={fin_to}");
                        }
                    }
                    Err(e) => {
                        log::warn!("GAME_FAIL worker={w} n={n} after_ms={} err={e}", t0.elapsed().as_millis());
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }));
    }
    for t in tasks { let _ = t.await; }
    log::info!("TTLOOP END");
}
