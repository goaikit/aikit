import assert from "node:assert/strict";
import fs from "node:fs";
assert.deepEqual(process.argv.slice(2), ["", "quote\" slash\\ ' $ ; é"]);
let text = "";
for await (const chunk of process.stdin) text += chunk;
const request = JSON.parse(text);
assert.equal(request.aikit_hook_version, 2);
const mode = fs.readFileSync(process.env.AIKIT_PI_TEST_MODE, "utf8");
fs.appendFileSync(process.env.AIKIT_PI_TEST_LOG, `${JSON.stringify(request)}\n`);
if (mode === "paused") {
  while (!fs.existsSync(`${process.env.AIKIT_PI_TEST_MODE}.release`)) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}
if (mode === "hang") await new Promise((resolve) => setTimeout(resolve, 10_000));
if (mode === "exit") process.exit(2);
if (mode === "bad") { process.stdout.write("invalid JSON"); process.exit(0); }
if (mode === "flood") { process.stdout.write("x".repeat(100_000)); process.exit(0); }
if (mode === "empty") { process.stdout.write("{}"); process.exit(0); }
process.stdout.write(JSON.stringify(mode === "allow" ? { decision: "allow" } : { decision: "block", reason: "review é pending" }));
