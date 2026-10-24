// Generated provider bridge. Native context is not application authorization.
const config = /* AIKIT_CONFIG */null;
let sessionEpoch;
let sessionGeneration = 0, sessionStarting = false;
let pendingSessionStart, pendingSessionObservation;
let contextPublication = Promise.resolve();
let nativeTurnId;
let turnStartObservation;
/* AIKIT_MESSAGE_HELPERS */
function assertSession(epoch) {
  if (!epoch || epoch !== sessionEpoch || !epoch.active) throw new Error('Native session context changed');
}
function publishContext($, epoch = sessionEpoch) {
  // Serialize environment writes: an abandoned session must never publish after
  // its replacement, even when an earlier env.set is still settling.
  const publish = async () => {
    assertSession(epoch);
    const session_id = await $.session.id(), cwd = await $.session.cwd();
    assertSession(epoch);
    if (session_id !== epoch.sessionId || cwd !== epoch.cwd) throw new Error('Native session context changed');
    const native = {session_id, invocation_id: epoch.invocationId, cwd};
    await $.env.set('AIKIT_INVOCATION_ENV_KEY', JSON.stringify({schema_version: 1,
      installation_id: config.installationId, native_context: native, ...(nativeTurnId ? {native_turn_id: nativeTurnId} : {})}));
    assertSession(epoch);
    return native;
  };
  const pending = contextPublication.then(publish, publish);
  contextPublication = pending.catch(() => {});
  return pending;
}
async function startSession($, expected) {
  if (sessionStarting || sessionEpoch?.active) throw new Error('Native session already started');
  sessionStarting = true;
  const generation = sessionGeneration;
  let epoch;
  try {
    const sessionId = await $.session.id(), cwd = await $.session.cwd();
    if (generation !== sessionGeneration) throw new Error('Native session startup was abandoned');
    if (expected && (expected.session_id !== sessionId || expected.cwd !== cwd))
      throw new Error('Native session startup context changed');
    epoch = {sessionId, cwd, invocationId: crypto.randomUUID(), active: true};
    sessionEpoch = epoch;
    nativeTurnId = undefined;
    turnStartObservation = undefined;
    const native = await publishContext($, epoch);
    if (config.sessionArgv) {
      const started = await $.process.run(config.sessionArgv, {stdin: JSON.stringify({
        hook_event_name: 'SessionStart', session_id: native.session_id, cwd: native.cwd,
      }), timeoutMs: config.timeoutMs});
      assertSession(epoch);
      if (started.exitCode !== 0 || started.isStdoutTruncated !== false || started.isStderrTruncated !== false)
        throw new Error('Recorded session startup unavailable');
    }
    /* AIKIT_MESSAGE_START */
  } catch (error) {
    if (epoch) {epoch.active = false; epoch.stop?.();}
    throw error;
  } finally {sessionStarting = false;}
}
async function resumePendingSession($) {
  const pending = pendingSessionStart, generation = sessionGeneration;
  if (!pending) return;
  if (!pendingSessionObservation) {
    const sessionId = await $.session.id(), cwd = await $.session.cwd();
    if (pending !== pendingSessionStart || generation !== sessionGeneration) return;
    // Native 2.1.308 resumes after classic.SessionStart has returned. Do not
    // publish the requested ID while the API still names the departing session.
    if (sessionId !== pending.session_id || cwd !== pending.cwd) return;
    if (!pendingSessionObservation) pendingSessionObservation = startSession($, pending);
  }
  await pendingSessionObservation;
  if (pending === pendingSessionStart && generation === sessionGeneration) {
    pendingSessionStart = undefined;
    $.ui.invalidate('prompt.context');
  }
}
async function observeTurn($, event, finished, epoch = sessionEpoch) {
  if (event.agentId !== undefined || !config.sessionArgv) return;
  assertSession(epoch);
  if (typeof event.turnId !== 'string' || !event.turnId.trim() || event.turnId.length > 256)
    throw new Error('Native Turn identity unavailable');
  if (!finished) nativeTurnId = event.turnId;
  if (event.turnId !== nativeTurnId) throw new Error('Native Turn identity changed');
  const native = await publishContext($, epoch);
  if (event.turnId !== nativeTurnId) throw new Error('Native Turn identity changed');
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
  assertSession(epoch);
  if (result.exitCode !== 0 || result.isStdoutTruncated !== false || result.isStderrTruncated !== false)
    throw new Error('Native Turn observation unavailable');
}
const unavailable = 'Agent operation unavailable. Retain an uncertain operation ID; do not infer success or blindly resend.';
async function invoke($, event) {
  try {
    if (event.agentId !== undefined) return { deny: 'Subagent authoring is not qualified.' };
    const epoch = sessionEpoch, turn = nativeTurnId, startup = turnStartObservation;
    assertSession(epoch);
    if (config.sessionArgv) {
      if (!turnStartObservation) return { deny: 'Native Turn startup observation unavailable.' };
      await turnStartObservation;
    }
    assertSession(epoch);
    if (turn !== nativeTurnId || startup !== turnStartObservation) throw new Error('Native Turn identity changed');
    const request = event.request;
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || new TextEncoder().encode(JSON.stringify(request)).length > 65536)
      return { deny: 'Agent request must be a JSON object of at most 64 KiB.' };
    const call = { schema_version: 1, application_id: config.applicationId, installation_id: config.installationId,
      agent_key: 'claude', tool_name: config.toolName,
      native_context: await publishContext($, epoch), request };
    if (turn !== nativeTurnId || startup !== turnStartObservation) throw new Error('Native Turn identity changed');
    const stdin = JSON.stringify(call);
    if (new TextEncoder().encode(stdin).length > 131072) return { deny: 'Agent invocation exceeds its bounded envelope.' };
    const result = await $.process.run(config.argv, { stdin, timeoutMs: config.timeoutMs });
    assertSession(epoch);
    if (turn !== nativeTurnId || startup !== turnStartObservation) throw new Error('Native Turn identity changed');
    if (result.isStdoutTruncated !== false || result.isStderrTruncated !== false
        || new TextEncoder().encode(result.stdout).length > 1048576) return { deny: unavailable };
    JSON.parse(result.stdout);
    return result.exitCode === 0 ? { result: result.stdout } : { result: result.stdout, isError: true };
  } catch { return { deny: unavailable }; }
}
export function register(on) {
  /* AIKIT_COMPLETION_BRIDGE */
  /* AIKIT_MESSAGE_LIFECYCLE */
  on('session.start', async ($, event, next) => {
    await startSession($);
    await $.tool.register({ name: config.toolName, description: config.description, isDeferred: false,
      inputSchema: { type: 'object', properties: { request: config.schema }, required: ['request'], additionalProperties: false } });
    $.ui.invalidate('prompt.context');
    return next(event);
  });
  on('session.end', async ($, event, next) => {
    sessionGeneration++;
    pendingSessionStart = undefined;
    pendingSessionObservation = undefined;
    if (sessionEpoch) {sessionEpoch.active = false; sessionEpoch.stop?.();}
    nativeTurnId = undefined;
    turnStartObservation = undefined;
    return next(event);
  });
  on('classic.SessionStart', async ($, event, next) => {
    // session.start is mod-load only; clear/resume retain this module instance.
    if (['clear', 'resume'].includes(event.source)) {
      if (sessionEpoch?.active || pendingSessionStart) throw new Error('Native session already started');
      if (typeof event.session_id !== 'string' || !event.session_id || typeof event.cwd !== 'string' || !event.cwd)
        throw new Error('Native session startup context changed');
      pendingSessionStart = {session_id: event.session_id, cwd: event.cwd};
      pendingSessionObservation = undefined;
      await resumePendingSession($);
    }
    return next(event);
  });
  on('turn.start', async ($, event, next) => {
    if (event.agentId === undefined) {
      await resumePendingSession($);
      const epoch = sessionEpoch;
      /* AIKIT_MESSAGE_BUSY */
      turnStartObservation = observeTurn($, event, false, epoch);
      await turnStartObservation;
      assertSession(epoch);
    }
    return next(event);
  });
  on('turn.complete', async ($, event, next) => {
    const epoch = sessionEpoch;
    // A proposal/Allow is never an outcome. Observe only the completed native
    // chain, retain abort/refusal/error, and never retry a failed observation.
    const observed = event.agentId === undefined && config.sessionArgv;
    if (observed && (next?.origin?.plugin !== 'engine' || next.origin.tier !== 'core'))
      throw new Error('Native completion requires an engine dispatch');
    const result = await next(event);
    if (observed) {
      const core = Array.isArray(next.trace) ? next.trace.filter(entry =>
        entry?.plugin === 'engine' && entry.tier === 'core' && entry.event === 'turn.complete') : [];
      if (core.length !== 1 || !['returned', 'passed'].includes(core[0].outcome)
          || !core[0].received || ['turnId', 'agentId', 'reason', 'isAborted', 'answer']
            .some(key => core[0].received[key] !== event[key]))
        throw new Error('Native completion engine trace is missing or changed');
    }
    await observeTurn($, event, true, epoch);
    /* AIKIT_MESSAGE_IDLE */
    return result;
  });
  on('prompt.context', async ($, event, next) => {
    const context = await next(event);
    if (!config.instructions) return context;
    if (context.blocks.some(block => block.name === config.contextName)) throw new Error('Conflicting application instruction block');
    return { ...context, blocks: [...context.blocks, { name: config.contextName, text: config.instructions }] };
  });
  on('tool.call', { tool: 'AIKIT_NATIVE_TOOL_NAME' }, async ($, event, next) => {
    // Dispatch metadata belongs to the native host, not model request JSON.
    // This guard is not authentication of the eventual stdio child process.
    if (next?.origin?.plugin !== 'engine' || next.origin.tier !== 'core')
      return { deny: 'Agent operation requires a native engine tool dispatch.' };
    return invoke($, event);
  })
    .catch(() => ({ deny: unavailable }));
}
