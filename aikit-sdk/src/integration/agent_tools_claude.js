// Generated provider bridge. Native context is not application authorization.
const config = /* AIKIT_CONFIG */null;
const invocationId = crypto.randomUUID();
let nativeTurnId;
let turnStartObservation;
/* AIKIT_MESSAGE_HELPERS */
async function publishContext($) {
  const native = { session_id: await $.session.id(), invocation_id: invocationId, cwd: await $.session.cwd() };
  await $.env.set('AIKIT_INVOCATION_ENV_KEY', JSON.stringify({ schema_version: 1,
    installation_id: config.installationId, native_context: native, ...(nativeTurnId ? { native_turn_id: nativeTurnId } : {}) }));
  return native;
}
async function observeTurn($, event, finished) {
  if (event.agentId !== undefined || !config.sessionArgv) return;
  if (typeof event.turnId !== 'string' || !event.turnId.trim() || event.turnId.length > 256)
    throw new Error('Native Turn identity unavailable');
  if (!finished) nativeTurnId = event.turnId;
  if (event.turnId !== nativeTurnId) throw new Error('Native Turn identity changed');
  const native = await publishContext($);
  const frame = { aikit_native_turn_version: 1,
    hook_event_name: finished ? 'AIKitNativeTurnFinished' : 'AIKitNativeTurnStarted',
    session_id: native.session_id, cwd: native.cwd, native_turn: { id: event.turnId } };
  if (finished) {
    if (!['answer', 'aborted', 'refusal', 'error'].includes(event.reason)
        || typeof event.isAborted !== 'boolean' || typeof event.answer !== 'string')
      throw new Error('Native Turn outcome unavailable');
    frame.native_turn.outcome = { reason: event.reason, is_aborted: event.isAborted };
    frame.last_assistant_message = event.answer;
  }
  const result = await $.process.run(config.sessionArgv, { stdin: JSON.stringify(frame), timeoutMs: config.timeoutMs });
  if (result.exitCode !== 0 || result.isStdoutTruncated !== false || result.isStderrTruncated !== false)
    throw new Error('Native Turn observation unavailable');
}
const unavailable = 'Agent operation unavailable. Retain an uncertain operation ID; do not infer success or blindly resend.';
async function invoke($, event) {
  try {
    if (event.agentId !== undefined) return { deny: 'Subagent authoring is not qualified.' };
    if (config.sessionArgv) {
      if (!turnStartObservation) return { deny: 'Native Turn startup observation unavailable.' };
      await turnStartObservation;
    }
    const request = event.request;
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || new TextEncoder().encode(JSON.stringify(request)).length > 65536)
      return { deny: 'Agent request must be a JSON object of at most 64 KiB.' };
    const call = { schema_version: 1, application_id: config.applicationId, installation_id: config.installationId,
      agent_key: 'claude', tool_name: config.toolName,
      native_context: { session_id: await $.session.id(), invocation_id: invocationId, cwd: await $.session.cwd() }, request };
    const stdin = JSON.stringify(call);
    if (new TextEncoder().encode(stdin).length > 131072) return { deny: 'Agent invocation exceeds its bounded envelope.' };
    const result = await $.process.run(config.argv, { stdin, timeoutMs: config.timeoutMs });
    if (result.isStdoutTruncated !== false || result.isStderrTruncated !== false
        || new TextEncoder().encode(result.stdout).length > 1048576) return { deny: unavailable };
    JSON.parse(result.stdout);
    return result.exitCode === 0 ? { result: result.stdout } : { result: result.stdout, isError: true };
  } catch { return { deny: unavailable }; }
}
export function register(on) {
  /* AIKIT_MESSAGE_LIFECYCLE */
  on('session.start', async ($, event, next) => {
    const native = await publishContext($);
    if (config.sessionArgv) {
      const started = await $.process.run(config.sessionArgv, { stdin: JSON.stringify({
        hook_event_name: 'SessionStart', session_id: native.session_id, cwd: native.cwd,
      }), timeoutMs: config.timeoutMs });
      if (started.exitCode !== 0 || started.isStdoutTruncated !== false || started.isStderrTruncated !== false)
        throw new Error('Recorded session startup unavailable');
    }
    await $.tool.register({ name: config.toolName, description: config.description, isDeferred: false,
      inputSchema: { type: 'object', properties: { request: config.schema }, required: ['request'], additionalProperties: false } });
    $.ui.invalidate('prompt.context');
    /* AIKIT_MESSAGE_START */
    return next(event);
  });
  on('turn.start', async ($, event, next) => {
    if (event.agentId === undefined) {
      /* AIKIT_MESSAGE_BUSY */
      turnStartObservation = observeTurn($, event, false);
      await turnStartObservation;
    }
    return next(event);
  });
  on('turn.complete', async ($, event, next) => {
    // A proposal/Allow is never an outcome. Observe only the completed native
    // chain, retain abort/refusal/error, and never retry a failed observation.
    const result = await next(event);
    await observeTurn($, event, true);
    /* AIKIT_MESSAGE_IDLE */
    return result;
  });
  on('prompt.context', async ($, event, next) => {
    const context = await next(event);
    if (!config.instructions) return context;
    if (context.blocks.some(block => block.name === config.contextName)) throw new Error('Conflicting application instruction block');
    return { ...context, blocks: [...context.blocks, { name: config.contextName, text: config.instructions }] };
  });
  on('tool.call', { tool: 'AIKIT_NATIVE_TOOL_NAME' }, async ($, event) => invoke($, event))
    .catch(() => ({ deny: unavailable }));
}
