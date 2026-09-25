import { connect } from "./lib.mjs";
import fs from "fs";
const rpc = await connect("n0");
const addrs = fs.readFileSync(process.argv[2],"utf8").match(/kaspatest:[a-z0-9]+/g).slice(0, +process.argv[3]||40);
for (const a of addrs) { const r = await rpc.getUtxosByAddresses({addresses:[a]}); const e=r.entries; let s=0n; for (const x of e) s+=BigInt(x.amount ?? x.utxoEntry?.amount); console.log(a.slice(0,20), e.length, Number(s)/1e8); }
process.exit(0);
