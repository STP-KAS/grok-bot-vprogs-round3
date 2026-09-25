#![no_std]
#![no_main]

//! grok-deskfloor: our own vprog guest (round 3). See Cargo.toml for the account layout and ops.

use vprogs_zk_abi::{transaction_processor::process_transaction, Error};
use vprogs_zk_backend_risc0_api::{Host, Journal, Sha256};

risc0_zkvm::guest::entry!(main);

const LEN: usize = 32;
const E_BAD_IX: u32 = 1;
const E_FLOOR: u32 = 2;
const E_MOVE_FLOOR: u32 = 3;
const E_OVERFLOW: u32 = 4;
const E_FLOOR_ABOVE_BALANCE: u32 = 5;

fn rd(d: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(d[i * 8..i * 8 + 8].try_into().unwrap())
}
fn wr(d: &mut [u8], i: usize, v: u64) {
    d[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
}

fn main() {
    process_transaction::<Sha256>(
        &mut Host,
        &mut Journal,
        |tx, _merge_idx, _context_hash, resources, _exits, _deposit| {
            let ix = tx.payload.ix_data;
            if ix.len() < 17 || resources.is_empty() {
                return Err(Error::Guest(E_BAD_IX));
            }
            let op = ix[0];
            let amount = u64::from_le_bytes(ix[1..9].try_into().unwrap());
            let tag = u64::from_le_bytes(ix[9..17].try_into().unwrap());
            for r in resources.iter_mut() {
                if r.is_new() || r.data().len() < LEN {
                    r.resize(LEN);
                }
            }
            let (bal, floor, ops) = {
                let d = resources[0].data();
                (rd(d, 0), rd(d, 1), rd(d, 2))
            };
            match op {
                0 => {
                    if amount > bal {
                        return Err(Error::Guest(E_FLOOR_ABOVE_BALANCE));
                    }
                    wr(resources[0].data_mut(), 1, amount);
                }
                1 => {
                    let nb = bal.checked_add(amount).ok_or(Error::Guest(E_OVERFLOW))?;
                    wr(resources[0].data_mut(), 0, nb);
                }
                2 => {
                    let nb = bal.checked_sub(amount).ok_or(Error::Guest(E_FLOOR))?;
                    if nb < floor {
                        return Err(Error::Guest(E_FLOOR));
                    }
                    wr(resources[0].data_mut(), 0, nb);
                }
                3 | 4 => {
                    // 3 = move resources[0] -> resources[1], 4 = move resources[1] -> resources[0]
                    // (the access list is sorted by resource id, so the runner picks the direction).
                    if resources.len() < 2 {
                        return Err(Error::Guest(E_BAD_IX));
                    }
                    let (s, t) = if op == 3 { (0, 1) } else { (1, 0) };
                    let (sb, sf, so) = {
                        let d = resources[s].data();
                        (rd(d, 0), rd(d, 1), rd(d, 2))
                    };
                    let nb = sb.checked_sub(amount).ok_or(Error::Guest(E_MOVE_FLOOR))?;
                    if nb < sf {
                        return Err(Error::Guest(E_MOVE_FLOOR));
                    }
                    let (tb, to) = {
                        let d = resources[t].data();
                        (rd(d, 0), rd(d, 2))
                    };
                    let nt = tb.checked_add(amount).ok_or(Error::Guest(E_OVERFLOW))?;
                    let ds = resources[s].data_mut();
                    wr(ds, 0, nb);
                    wr(ds, 2, so + 1);
                    wr(ds, 3, tag);
                    let dt = resources[t].data_mut();
                    wr(dt, 0, nt);
                    wr(dt, 2, to + 1);
                    wr(dt, 3, tag);
                    return Ok(());
                }
                _ => return Err(Error::Guest(E_BAD_IX)),
            }
            let d0 = resources[0].data_mut();
            wr(d0, 2, ops + 1);
            wr(d0, 3, tag);
            Ok(())
        },
    );
}
