import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

const unavailable = "AIKit integration checks unavailable; restore the local handler before retrying.";
const selected = new Set(config.events);

// No process, timer or other session resource is started by the factory.
export default function (pi) {
  let invocation;
  let lastBoundary;
  let continued = false;
  const warn = (ctx, reason) => { try { ctx.ui.notify(reason, "warning"); } catch { /* UI failure cannot cancel a denial. */ } };
  const request = (name, ctx, data = {}) => ({
    ...data,
    aikit_hook_version: 2,
    invocation_id: invocation,
    hook_event_name: name,
    session_id: ctx.sessionManager.getSessionId(),
    cwd: ctx.cwd,
  });

  async function invoke(input, decisionPoint) {
    try {
      const raw = JSON.stringify(input);
      if (Buffer.byteLength(raw) > 1024 * 1024) throw new Error("input limit");
      const result = await new Promise((resolve, reject) => {
        const child = spawn(config.executable, config.arguments, {
          shell: false, windowsHide: true, stdio: ["pipe", "pipe", "pipe"],
        });
        const chunks = [];
        let length = 0;
        let done = false;
        const finish = (error, value) => {
          if (done) return;
          done = true;
          clearTimeout(timer);
          if (error) { child.kill(); reject(error); } else resolve(value);
        };
        const timer = setTimeout(() => finish(new Error("deadline")), config.timeout_ms);
        child.on("error", (error) => finish(error));
        child.stdin.on("error", (error) => finish(error));
        child.stderr.resume(); // Never expose arbitrary child diagnostics/prompts.
        child.stdout.on("data", (chunk) => {
          length += chunk.length;
          if (length > 64 * 1024) return finish(new Error("output limit"));
          chunks.push(chunk);
        });
        child.on("close", (code) => finish(code === 0 ? null : new Error("handler failed"), Buffer.concat(chunks).toString("utf8")));
        child.stdin.end(raw);
      });
      const response = JSON.parse(result);
      if (!input.invocation_id || input.invocation_id !== invocation) throw new Error("invocation changed");
      if (!response || typeof response !== "object" || Array.isArray(response)) throw new Error("response shape");
      if (!decisionPoint) return response;
      if (response.decision === "allow") return response;
      if (response.decision === "block" && typeof response.reason === "string" && response.reason.trim() && Buffer.byteLength(response.reason) <= 8192) return response;
      throw new Error("missing decision");
    } catch {
      if (decisionPoint) return { decision: "block", reason: unavailable };
      return { error: true };
    }
  }

  async function observe(input, ctx) {
    if ((await invoke(input, false)).error) warn(ctx, unavailable);
  }

  pi.on("session_start", async (event, ctx) => {
    invocation = randomUUID();
    lastBoundary = undefined;
    continued = false;
    if (selected.has("session_started")) await observe(request(event.type, ctx), ctx);
  });
  if (selected.has("input_submitted")) pi.on("input", async (event, ctx) => {
    // Input may be queued during a run. Do not reset settlement evidence here.
    const result = await invoke(request(event.type, ctx), true);
    if (result.decision === "block") { warn(ctx, result.reason); return { action: "handled" }; }
    return { action: "continue" };
  });
  if (selected.has("before_tool")) pi.on("tool_call", async (event, ctx) => {
    const result = await invoke(request(event.type, ctx, {
      tool_use_id: event.toolCallId, tool_name: event.toolName, tool_input: event.input,
    }), true);
    if (result.decision === "block") return { block: true, reason: result.reason };
    // Undefined preserves native permissions and other extensions' behavior.
  });
  if (selected.has("after_tool") || selected.has("tool_failed")) pi.on("tool_result", async (event, ctx) => {
    if (!selected.has(event.isError ? "tool_failed" : "after_tool")) return;
    await observe(request(event.type, ctx, {
      tool_use_id: event.toolCallId, tool_name: event.toolName,
      tool_response: event.content, is_error: event.isError,
    }), ctx);
    // Observation never rewrites the native result.
  });
  if (selected.has("completion_proposed") || selected.has("completion_failed")) {
    pi.on("agent_before_settle", async (event, ctx) => {
      lastBoundary = { session: ctx.sessionManager.getSessionId(), outcome: event.outcome };
      if (event.outcome !== "completed" || !selected.has("completion_proposed")) return;
      let result;
      try {
        const messages = event.context.contextMessages;
        const lastAssistant = [...messages].reverse().find((m) => m.role === "assistant");
        const answer = lastAssistant?.content?.filter((part) => part.type === "text").map((part) => part.text).join("\n");
        result = await invoke(request(event.type, ctx, {
          outcome: event.outcome, last_assistant_message: answer, stop_hook_active: continued,
        }), true);
      } catch { result = { decision: "block", reason: unavailable }; }
      if (result.decision === "block") {
        continued = true;
        return { entries: [...(Array.isArray(event.entries) ? event.entries : []), { type: "custom_message", customType: "aikit.integration.block", content: result.reason, display: true }], continue: true };
      }
      // Do not set continue:false: it would cancel another extension's request.
    });
    pi.on("agent_settled", async (event, ctx) => {
      const boundary = lastBoundary;
      lastBoundary = undefined;
      continued = false;
      if (selected.has("completion_failed") && boundary?.session === ctx.sessionManager.getSessionId() && ["error", "aborted"].includes(boundary.outcome)) {
        await observe(request(event.type, ctx, { outcome: boundary.outcome }), ctx);
      }
      // Settlement without correlated outcome never fabricates success/failure.
    });
  }
  if (selected.has("session_ended")) pi.on("session_shutdown", async (event, ctx) => {
    await observe(request(event.type, ctx), ctx);
    invocation = undefined;
    lastBoundary = undefined;
    continued = false;
  });
}
