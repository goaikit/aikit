// Opt-in native handoff candidate. Recorded checks are not a native lease.
// Returned receipts remain unqualified; no acknowledgement/consumption is inferred.
let messageBridge, messageTimerStarted = false;
function createMessageBridge($, epoch) {
const invocationId = epoch.invocationId;
let messageActive = true, messageStopped = false, messageStarted = false, messagePolling = false;
let messageContext, messageTicket;
let messageInFlight, messageAppendSeen = false, messageProviderVersion;
let messageInputCandidate;
let messageApiRead;
const messageAttempts = new Set();
const messageBytes = value => new TextEncoder().encode(value);
async function messageDigest(value) {
  return [...new Uint8Array(await crypto.subtle.digest('SHA-256', messageBytes(value)))]
    .map(byte => byte.toString(16).padStart(2, '0')).join('');
}
function sameMessageContext(native) {
  return native.session_id === messageContext.session_id && native.cwd === messageContext.cwd
    && native.invocation_id === messageContext.invocation_id;
}
function sameMessageReference(left, right) {
  return ['installation_id', 'installation_revision', 'native_session_id', 'invocation_id', 'start_cursor']
    .every(key => left?.[key] === right?.[key]);
}
function sameMessageReservation(left, right) {
  return left?.binding_id === right.binding_id && left?.reservation_id === right.reservation_id
    && left?.payload_sha256 === right.payload_sha256
    && left?.request?.operation_id === right.request.operation_id
    && left?.request?.content === right.request.content
    && sameMessageReference(left?.target, right.target);
}
function validateMessageReply(reply) {
  const ref = reply?.reference;
  if (reply?.schema_version !== 1 || typeof reply.recorded_idle !== 'boolean'
      || typeof reply.recorded_ready !== 'boolean'
      || ref?.installation_id !== config.installationId
      || typeof ref?.installation_revision !== 'string' || !ref.installation_revision
      || !Number.isSafeInteger(ref?.start_cursor) || ref.start_cursor < 1
      || ref?.native_session_id !== messageContext.session_id || ref?.invocation_id !== invocationId)
    throw new Error('Recorded message context unavailable');
}
async function messageRpc(action) {
  const native = await publishContext($, epoch);
  if (!sameMessageContext(native)) throw new Error('Native message context changed');
  const stdin = JSON.stringify({schema_version: 1, application_id: config.applicationId,
    installation_id: config.installationId, agent_key: 'claude', native_context: native, action});
  if (messageBytes(stdin).length > config.messageEnvelopeLimit) throw new Error('Message envelope exceeds limit');
  const result = await $.process.run(config.messageArgv, {stdin, timeoutMs: config.timeoutMs});
  assertSession(epoch);
  if (result.exitCode !== 0 || result.isStdoutTruncated !== false || result.isStderrTruncated !== false
      || typeof result.stdout !== 'string' || messageBytes(result.stdout).length > 1048576)
    throw new Error('Message journal unavailable');
  const reply = JSON.parse(result.stdout);
  validateMessageReply(reply);
  return reply;
}
function appendText(message) {
  if (message?.type !== 'user' || message.role !== 'user' || !Array.isArray(message.content)
      || message.content.length !== 1 || message.content[0].type !== 'text'
      || typeof message.content[0].text !== 'string') throw new Error('Native append content unavailable');
  return message.content[0].text;
}
function validateAppendReply(reply, ticket, intent, stored) {
  const checkpoint = reply.append_checkpoint, reservation = checkpoint?.reservation;
  if (checkpoint?.schema_version !== 1 || !sameMessageReference(reply.reference, ticket.target)
      || reservation?.binding_id !== ticket.binding_id || !sameMessageReference(reservation?.target, ticket.target)
      || reservation?.reservation_id !== ticket.reservation_id || reservation?.payload_sha256 !== ticket.payload_sha256
      || reservation?.request?.operation_id !== ticket.request.operation_id || reservation?.request?.content !== ticket.request.content
      || checkpoint.intent?.native_row_id !== intent.native_row_id || checkpoint.intent?.native_source !== intent.native_source
      || checkpoint.intent?.framed_payload_sha256 !== intent.framed_payload_sha256
      || !Number.isSafeInteger(checkpoint.prepared_at_unix_ms) || checkpoint.prepared_at_unix_ms <= 0)
    throw new Error('Native append checkpoint scope unavailable');
  if (stored) {
    if (checkpoint.stored?.stored_payload_sha256 !== stored.stored_payload_sha256
        || checkpoint.stored?.evidence_reference !== stored.evidence_reference
        || !Number.isSafeInteger(checkpoint.stored_at_unix_ms) || checkpoint.stored_at_unix_ms <= 0)
      throw new Error('Native append storage checkpoint unavailable');
  } else if (checkpoint.stored !== null || checkpoint.stored_at_unix_ms !== null)
    throw new Error('Native append intent already has a storage observation');
}
async function observeMessageAppend(event, next) {
  const ticket = messageInFlight;
  if (!ticket || event.agentId !== undefined || event.origin?.kind !== 'plugin'
      || event.origin.name !== config.pluginName) return next(event);
  if (messageAppendSeen) { messageStopped = true; return next(event); }
  messageAppendSeen = true;
  let intent, prepared = false;
  try {
    if (messageStopped || event.door !== 'prompt'
        || typeof event.uuid !== 'string' || !/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(event.uuid))
      throw new Error('Native append identity unavailable');
    // Qualified on native 2.1.296/2.1.308 for ASCII SDK names at lengths 21–25 and 35:
    // labels over 24 characters use the first 23 plus U+2026. Provenance stays full.
    const label = config.pluginName.length > 24 ? config.pluginName.slice(0, 23) + '…' : config.pluginName;
    const expected = `The ${label} plugin sent a message:\n${ticket.request.content}`
      + "\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.";
    if (appendText(event.message) !== expected) throw new Error('Unknown or rewritten native framing');
    intent = {native_row_id: event.uuid, native_source: `claude-code/${messageProviderVersion}/session.append`,
      framed_payload_sha256: await messageDigest(expected)};
    const reply = await messageRpc({kind: 'prepare_append', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id, intent});
    validateAppendReply(reply, ticket, intent);
    prepared = true;
  } catch { messageStopped = true; }
  // Delegate storage exactly once. A capture failure does not establish that the
  // engine blocked storage. Never throw after successful next merely to retry it.
  const result = await next(event);
  if (prepared) try {
    if (result?.uuid !== intent.native_row_id) throw new Error('Native append result identity changed');
    // A downstream return is not evidence that the engine stored this row.
    // Check the actual settled core execution, including both input and output;
    // keep the original intent so real stored rewrites remain visible as such.
    const core = Array.isArray(next.trace) ? next.trace.filter(entry =>
      entry?.plugin === 'engine' && entry.tier === 'core' && entry.event === 'session.append') : [];
    if (core.length !== 1 || !['returned', 'passed'].includes(core[0].outcome))
      throw new Error('Native append engine trace unavailable');
    const received = core[0].received, storedRow = core[0].returned;
    if (received?.uuid !== intent.native_row_id || received.door !== 'prompt'
        || received.agentId !== undefined || received.origin?.kind !== 'plugin'
        || received.origin.name !== config.pluginName || storedRow?.uuid !== intent.native_row_id
        || storedRow.deny !== undefined || result.deny !== undefined
        || appendText(received.message) !== appendText(result.message)
        || appendText(storedRow.message) !== appendText(result.message))
      throw new Error('Native append engine row changed');
    const stored = {stored_payload_sha256: await messageDigest(appendText(result.message)),
      evidence_reference: `claude-code:session:${messageContext.session_id}:row:${intent.native_row_id}`};
    const reply = await messageRpc({kind: 'record_append_stored', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id, native_row_id: intent.native_row_id, stored});
    validateAppendReply(reply, ticket, intent, stored);
    if (stored.stored_payload_sha256 !== intent.framed_payload_sha256) messageStopped = true;
    else if (!messageStopped && messageProviderVersion === '2.1.308') {
      if (messageInputCandidate) throw new Error('Unsettled input candidate already exists');
      messageInputCandidate = {ticket, appendIntent: intent, stored, frame: appendText(result.message)};
    }
  } catch { messageStopped = true; }
  return result;
}
  // API reads are hookable too. Bind this one read to the genuine core result,
  // then compare again at the API caller to catch rewrites on either side.
  // This is local trace correlation, not authenticated caller/consumption proof.
  async function observeApiMessages(event, next) {
    const read = messageApiRead;
    if (!read || event.as !== 'api' || event.agentId !== undefined || next?.origin?.plugin !== config.pluginName)
      return next(event);
    const result = await next(event);
    try {
      const core = Array.isArray(next.trace) ? next.trace.filter(entry =>
        entry?.plugin === 'engine' && entry.tier === 'core' && entry.event === 'session.messages') : [];
      if (core.length !== 1 || !['returned', 'passed'].includes(core[0].outcome)
          || messageCanonical(core[0].received) !== messageCanonical({as: 'api'})
          || !Array.isArray(result?.value) || !Array.isArray(core[0].returned?.value))
        throw new Error('Native API view core trace unavailable');
      for (const value of [result.value, core[0].returned.value])
        if (messageBytes(JSON.stringify(value)).length > 4194304)
          throw new Error('Native API view exceeds candidate limit');
      const canonical = messageCanonical(result.value);
      if (canonical !== messageCanonical(core[0].returned.value))
        throw new Error('Native API view changed after core');
      if (messageApiRead !== read) throw new Error('Native API read scope changed');
      read.canonical = canonical;
    } catch { messageStopped = true; }
    return result;
  }
  async function* observeInputStep(event, next) {
    const candidate = messageInputCandidate;
    if (!candidate || event.agentId !== undefined) return yield* next(event);
    // Take one step only; missing/uncertain evidence is retained, never retried
    // on another step and never permission to resubmit the owner review.
    messageInputCandidate = undefined;
    let intent, prepared = false;
    try {
      if (messageStopped || next?.origin?.plugin !== 'engine' || next.origin.tier !== 'core'
          || event.turnId !== nativeTurnId || typeof event.turnId !== 'string' || !event.turnId
          || event.turnId.length > 256 || !Number.isSafeInteger(event.index) || event.index < 0)
        throw new Error('Native input step scope unavailable');
      const read = {};
      messageApiRead = read;
      let snapshot;
      try { snapshot = await $.session.messages({as: 'api'}); }
      finally { if (messageApiRead === read) messageApiRead = undefined; else read.canonical = undefined; }
      const serialized = JSON.stringify(snapshot);
      if (!Array.isArray(snapshot) || typeof serialized !== 'string' || messageBytes(serialized).length > 4194304)
        throw new Error('Native API snapshot unavailable or oversized');
      if (read.canonical === undefined || read.canonical !== messageCanonical(snapshot))
        throw new Error('Native API snapshot lacks a matching core read');
      const occurrences = snapshot.flatMap(message => typeof message.content === 'string' ? [message.content]
        : Array.isArray(message.content) ? message.content.filter(block => block?.type === 'text').map(block => block.text) : [])
        .filter(text => text === candidate.frame).length;
      if (occurrences !== 1) throw new Error('Exact native feedback frame missing or duplicated');
      intent = {native_turn_id: event.turnId, step_index: event.index,
        api_snapshot_sha256: await messageDigest(serialized),
        framed_payload_sha256: candidate.appendIntent.framed_payload_sha256,
        step_event_sha256: await messageDigest(messageCanonical(event))};
      const reply = await messageRpc({kind: 'prepare_input', operation_id: candidate.ticket.request.operation_id,
        reservation_id: candidate.ticket.reservation_id, intent});
      validateInputReply(reply, candidate, intent);
      prepared = true;
    } catch { messageStopped = true; }
    const result = yield* next(event);
    if (prepared) try {
      const core = Array.isArray(next.trace) ? next.trace.filter(entry =>
        entry?.plugin === 'engine' && entry.tier === 'core' && entry.event === 'turn.step') : [];
      if (core.length !== 1 || !['returned', 'passed'].includes(core[0].outcome)
          || await messageDigest(messageCanonical(core[0].received)) !== intent.step_event_sha256)
        throw new Error('Matching native input core dispatch unavailable');
      const trace = messageCanonical(core[0]);
      if (messageBytes(trace).length > 262144) throw new Error('Native core trace exceeds candidate limit');
      const dispatched = {core_received_sha256: intent.step_event_sha256,
        core_trace_sha256: await messageDigest(trace),
        evidence_reference: `claude-code:session:${messageContext.session_id}:turn:${intent.native_turn_id}:step:${intent.step_index}`};
      const reply = await messageRpc({kind: 'record_input_dispatched', operation_id: candidate.ticket.request.operation_id,
        reservation_id: candidate.ticket.reservation_id, intent, dispatched});
      validateInputReply(reply, candidate, intent, dispatched);
    } catch { messageStopped = true; }
    return result;
  }
function messageCanonical(value) {
  const sort = value => Array.isArray(value) ? value.map(sort) : value && typeof value === 'object'
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, sort(value[key])])) : value;
  return JSON.stringify(sort(value));
}
function validateInputReply(reply, candidate, intent, dispatched) {
  const checkpoint = reply.append_checkpoint, input = checkpoint?.input;
  if (checkpoint?.schema_version !== 2 || !sameMessageReference(reply.reference, candidate.ticket.target)
      || !sameMessageReservation(checkpoint.reservation, candidate.ticket)
      || messageCanonical(checkpoint.intent) !== messageCanonical(candidate.appendIntent)
      || messageCanonical(checkpoint.stored) !== messageCanonical(candidate.stored)
      || messageCanonical(input?.intent) !== messageCanonical(intent)
      || !Number.isSafeInteger(input?.prepared_at_unix_ms) || input.prepared_at_unix_ms <= 0)
    throw new Error('Native input checkpoint scope unavailable');
  if (dispatched) {
    if (messageCanonical(input.dispatched) !== messageCanonical(dispatched)
        || !Number.isSafeInteger(input.dispatched_at_unix_ms) || input.dispatched_at_unix_ms <= 0)
      throw new Error('Native input dispatch checkpoint unavailable');
  } else if (input.dispatched !== null || input.dispatched_at_unix_ms !== null)
    throw new Error('Native input intent already has a dispatch observation');
}
async function pollMessages() {
  if (messageActive || messageStopped || messagePolling) return;
  messagePolling = true;
  try {
    if (!messageTicket) {
      const reply = await messageRpc({kind: 'poll'});
      if (!reply.reservation) return;
      messageTicket = reply.reservation;
      const ticket = messageTicket, target = ticket.target;
      if (!reply.recorded_idle || !sameMessageReference(target, reply.reference)
          || typeof ticket.binding_id !== 'string' || !ticket.binding_id
          || typeof ticket.reservation_id !== 'string' || !ticket.reservation_id
          || typeof ticket.request?.operation_id !== 'string' || !ticket.request.operation_id
          || typeof ticket.request.content !== 'string' || !ticket.request.content
          || messageBytes(ticket.request.content).length > config.messagePayloadLimit
          || await messageDigest(ticket.request.content) !== ticket.payload_sha256)
        throw new Error('Message reservation unavailable');
    }
    const ticket = messageTicket;
    const check = await messageRpc({kind: 'recheck', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id});
    if (['unknown', 'failed', 'acknowledged'].includes(check.recorded_state)) {
      // Retire this exact ticket, never reset/resend it. The SDK's immutable
      // operation prevents it being reserved again; later reviews may still run.
      messageAttempts.add(ticket.reservation_id);
      messageTicket = undefined;
      return;
    }
    if (check.recorded_state !== undefined && check.recorded_state !== 'dispatching')
      throw new Error('Message attempt state changed unexpectedly');
    if (!check.recorded_idle || !check.recorded_ready || messageActive || messageStopped) return;
    if (!sameMessageReference(check.reference, ticket.target))
      throw new Error('Message target changed');
    if (!sameMessageReservation(check.reservation, ticket))
      throw new Error('Message recheck did not confirm the exact reservation');
    if (messageAttempts.has(ticket.reservation_id)) throw new Error('Message attempt already used');
    // No await between the last local busy check, attempt marker and native call.
    // Native queuing/admission across competing handlers is still a qualification gate.
    messageAttempts.add(ticket.reservation_id);
    messageInFlight = ticket;
    messageAppendSeen = false;
    let outcome;
    try {
      const result = await $.prompt.submit({text: ticket.request.content});
      // A native claimed refusal is not permission to associate this review
      // with a later owner's Turn. Retain the raw return as Unknown; no resend.
      if (typeof result?.drop === 'string') messageInputCandidate = undefined;
      const raw = JSON.stringify(result);
      if (typeof raw !== 'string' || messageBytes(raw).length > config.messageResultLimit)
        outcome = {kind: 'uncertain', reason: 'native_receipt_unavailable'};
      else outcome = {kind: 'returned', result};
    } catch {
      outcome = {kind: 'uncertain', reason: 'native_submit_threw'};
    } finally { messageInFlight = undefined; }
    // A return without the native append event supplies no candidate row identity.
    if (!messageAppendSeen) messageStopped = true;
    // Storing the raw return does not turn it into qualified delivery proof.
    await messageRpc({kind: 'record_handoff', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id, outcome});
    messageTicket = undefined;
  } catch {
    // A lost reservation or result may already have crossed a process boundary.
    // Stop this module's pump; never retry I/O, reset a ticket or infer a failure.
    messageStopped = true;
  } finally { messagePolling = false; }
}
async function start() {
  if (messageStarted) { messageStopped = true; throw new Error('Message module already started'); }
  messageStarted = true;
  messageContext = await publishContext($, epoch);
  const version = await $.session.version();
  assertSession(epoch);
  if (!['2.1.296', '2.1.308'].includes(version?.version) || typeof config.pluginName !== 'string'
      || !/^[a-z0-9-]{1,35}$/.test(config.pluginName))
    throw new Error('Native append framing is not qualified for this client version');
  if (config.messagePayloadLimit !== 65536 || config.messageResultLimit !== 524288
      || config.messageEnvelopeLimit !== 1048576)
    throw new Error('Message transport bounds are not qualified');
  messageProviderVersion = version.version;
  messageActive = false;
}
epoch.stop = () => {messageStopped = true;};
return {start, poll: pollMessages, append: observeMessageAppend, messages: observeApiMessages, step: observeInputStep,
  busy: () => {messageActive = true;}, idle: () => {messageActive = false;}};
}
async function startMessagePump($) {
  const bridge = createMessageBridge($, sessionEpoch);
  messageBridge = bridge;
  await bridge.start();
  if (!messageTimerStarted) {
    messageTimerStarted = true;
    $.clock.every(250, async () => {
      // Reuse the opted-in timer; authoring-only modules start no polling loop.
      // A failed startup stays failed, never automatically retried.
      try {await resumePendingSession($);} catch {return;}
      return messageBridge?.poll();
    });
  }
}
function registerMessageLifecycle(on) {
  on('session.append', {door: 'prompt'}, async ($, event, next) =>
    messageBridge ? messageBridge.append(event, next) : next(event));
  on('session.messages', async ($, event, next) =>
    messageBridge ? messageBridge.messages(event, next) : next(event));
  on('turn.step', async function* ($, event, next) {
    if (messageBridge) return yield* messageBridge.step(event, next);
    return yield* next(event);
  });
}
