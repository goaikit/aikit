// Qualification-only competing extension. Never installed in the owner profile.
import fs from "node:fs";
import { execFileSync } from "node:child_process";

export default function (pi) {
  pi.on("agent_before_settle", (event, ctx) => {
    const scenario = process.env.AIKIT_PI_SCENARIO;
    if (!["abort-after-allow", "override-block"].includes(scenario)) return;
    const journal = JSON.parse(execFileSync(process.env.AIKIT_PI_SDK_EXECUTABLE,
      ["events", process.env.AIKIT_PI_SDK_STATE, process.env.AIKIT_PI_INSTALLATION],
      { encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024, windowsHide: true }));
    const decisions = journal.records.filter((row) =>
      row.request.session_id === ctx.sessionManager.getSessionId() &&
      row.request.event === "completion_proposed" && row.decision);
    const last = decisions.at(-1);
    const expected = scenario === "abort-after-allow" ? "allow" : "block";
    if (!last || last.decision.decision !== expected) {
      throw new Error("Qualification probe must run after the expected SDK decision");
    }
    fs.appendFileSync(process.env.AIKIT_PI_EVIDENCE, `${JSON.stringify({
      type: "boundary_probe", scenario, sdk_decision: last.decision.decision,
      request_id: last.request.id, incoming_continue: event.continue,
      outcome_before_action: event.outcome, signal_present: ctx.signal !== undefined,
    })}\n`);
    if (scenario === "abort-after-allow") {
      ctx.abort();
      return;
    }
    // Demonstrate that a later extension can cancel the requested continuation.
    return { entries: [], continue: false };
  });
}
