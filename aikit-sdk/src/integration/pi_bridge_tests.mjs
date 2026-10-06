import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
const [source, cwd] = process.argv.slice(2);
const root = path.dirname(source);
process.env.AIKIT_PI_TEST_LOG = path.join(root, "observations.jsonl");
process.env.AIKIT_PI_TEST_MODE = path.join(root, "response-mode.txt");
const mode = (value) => fs.writeFileSync(process.env.AIKIT_PI_TEST_MODE, value);
const handlers = new Map();
const extension = (await import(pathToFileURL(source))).default;
extension({ on(name, callback) { assert(!handlers.has(name)); handlers.set(name, callback); } });
assert(!fs.existsSync(process.env.AIKIT_PI_TEST_LOG)); // Factory performs no work.
let session = "session-é";
let throwUi = false;
const warnings = [];
const ctx = { cwd, sessionManager: { getSessionId: () => session }, ui: { notify(message) { if (throwUi) throw Error("UI gone"); warnings.push(message); } } };
const emit = (type, fields = {}) => handlers.get(type)({ type, ...fields }, ctx);
const rows = () => fs.readFileSync(process.env.AIKIT_PI_TEST_LOG, "utf8").trim().split("\n").map(JSON.parse);
const boundary = { outcome: "completed", entries: [{ type: "custom", customType: "other", data: 1 }], context: { contextMessages: [{ role: "assistant", content: [{ type: "text", text: "answer é" }] }] } };
mode("allow");
await emit("session_start");
assert.deepEqual(await emit("input"), { action: "continue" }, JSON.stringify({warnings, rows: fs.existsSync(process.env.AIKIT_PI_TEST_LOG) ? rows() : []}));
assert.equal(await emit("tool_call", { toolCallId: "call", toolName: "write", input: { path: "file", content: "content é" } }), undefined);
assert.equal(await emit("tool_result", { toolCallId: "call", toolName: "write", content: [{ type: "text", text: "result" }], isError: true }), undefined);
assert.equal(await emit("agent_before_settle", boundary), undefined);
const beforeSuccess = rows().length;
await emit("agent_settled");
assert.equal(rows().length, beforeSuccess); // No invented accepted completion.
mode("deny");
throwUi = true;
assert.deepEqual(await emit("input"), { action: "handled" });
throwUi = false;
assert.deepEqual(await emit("tool_call", { toolCallId: "call2", toolName: "bash", input: {} }), { block: true, reason: "review é pending" });
for (let n = 0; n < 3; n++) {
  const result = await emit("agent_before_settle", boundary);
  assert.equal(result.continue, true);
  assert.deepEqual(result.entries[0], boundary.entries[0]);
  assert.equal(result.entries[1].content, "review é pending");
  assert.equal(rows().at(-1).stop_hook_active, n > 0);
}
mode("allow");
await emit("agent_before_settle", { ...boundary, outcome: "error" });
await emit("agent_settled");
assert.equal(rows().at(-1).outcome, "error");
assert.equal(rows().at(-1).hook_event_name, "agent_settled");
const afterFailure = rows().length;
await emit("agent_settled");
assert.equal(rows().length, afterFailure); // Correlation consumed once.
await emit("agent_before_settle", { ...boundary, outcome: "aborted" });
session = "another-session";
await emit("agent_settled");
assert.equal(rows().length, afterFailure); // No foreign failure attribution.
for (const failure of ["exit", "bad", "empty", "flood", "hang"]) {
  mode(failure);
  assert.equal((await emit("tool_call", { toolCallId: failure, toolName: "bash", input: {} })).block, true);
  assert.equal((await emit("agent_before_settle", boundary)).continue, true);
}
mode("allow");
await emit("session_shutdown");
assert.equal(rows().at(-1).hook_event_name, "session_shutdown");
assert.equal(rows()[0].session_id, "session-é");
assert(rows().some((row) => row.tool_input?.content === "content é"));
console.log("Pi bridge contract passed");
