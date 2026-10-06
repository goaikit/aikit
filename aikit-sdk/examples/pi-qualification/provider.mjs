// Deterministic model responses; Pi still owns its real loop, tools and extensions.
import fs from "node:fs";
import { pathToFileURL } from "node:url";

export default async function (pi) {
  const { fauxProvider, fauxAssistantMessage, fauxToolCall } =
    await import(pathToFileURL(process.env.AIKIT_PI_AI_MODULE).href);
  const scenario = process.env.AIKIT_PI_SCENARIO;
  const record = (row) => fs.appendFileSync(process.env.AIKIT_PI_EVIDENCE, `${JSON.stringify(row)}\n`);
  const faux = fauxProvider({
    provider: "aikit-fixture",
    models: [{ id: "deterministic", contextWindow: 100_000, maxTokens: 1024 }],
  });
  faux.setResponses(Array.from({ length: 8 }, () => (_context, _options, state) => {
    record({ type: "fixture_model_call", call: state.callCount, scenario });
    if (scenario === "failure") {
      return fauxAssistantMessage([], {
        stopReason: "error", errorMessage: "Qualification fixture failure",
      });
    }
    if (scenario === "write" && state.callCount === 1) {
      return fauxAssistantMessage(fauxToolCall("write", {
        path: process.env.AIKIT_PI_WRITE_FILE, content: "native-write-probe\n",
      }, { id: "native-write" }));
    }
    return fauxAssistantMessage("OK");
  }));
  pi.registerProvider(faux.provider);
  for (const name of ["session_start", "input", "tool_call", "tool_result",
    "agent_before_settle", "agent_settled", "session_shutdown"]) {
    pi.on(name, (event, ctx) => {
      record({ type: "native_event", event: name,
        session: ctx.sessionManager.getSessionId(), outcome: event.outcome,
        isError: event.isError, tool: event.toolName,
        event_keys: Object.keys(event).sort(), signal_present: ctx.signal !== undefined });
    });
  }
}
