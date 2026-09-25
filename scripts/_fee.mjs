import { connect } from "./lib.mjs";
const rpc = await connect("n0");
const r = await rpc.getFeeEstimate({});
const e = r.estimate;
console.log(JSON.stringify({prio: e.priorityBucket, normal: e.normalBuckets?.slice(0,3), low: e.lowBuckets?.slice(0,2)}, (k,v)=>typeof v==="bigint"?Number(v):v));
process.exit(0);
