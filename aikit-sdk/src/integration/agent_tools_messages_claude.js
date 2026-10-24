// Opt-in native handoff candidate. Recorded checks are not a native lease.
// Returned receipts remain unqualified; no acknowledgement/consumption is inferred.
let messageActive = true, messageStopped = false, messageStarted = false, messagePolling = false;
let messageContext, messageTicket;
let messageInFlight, messageAppendSeen = false, messageProviderVersion;
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
async function messageRpc($, action) {
  const native = await publishContext($);
  if (!sameMessageContext(native)) throw new Error('Native message context changed');
  const stdin = JSON.stringify({schema_version: 1, application_id: config.applicationId,
    installation_id: config.installationId, agent_key: 'claude', native_context: native, action});
  if (messageBytes(stdin).length > 131072) throw new Error('Message envelope exceeds limit');
  const result = await $.process.run(config.messageArgv, {stdin, timeoutMs: config.timeoutMs});
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
async function observeMessageAppend($, event, next) {
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
    const reply = await messageRpc($, {kind: 'prepare_append', operation_id: ticket.request.operation_id,
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
    const reply = await messageRpc($, {kind: 'record_append_stored', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id, native_row_id: intent.native_row_id, stored});
    validateAppendReply(reply, ticket, intent, stored);
    if (stored.stored_payload_sha256 !== intent.framed_payload_sha256) messageStopped = true;
  } catch { messageStopped = true; }
  return result;
}
function registerMessageLifecycle(on) {
  on('session.end', async ($, event, next) => { messageStopped = true; return next(event); });
  on('session.append', {door: 'prompt'}, async ($, event, next) => observeMessageAppend($, event, next));
}
async function pollMessages($) {
  if (messageActive || messageStopped || messagePolling) return;
  messagePolling = true;
  try {
    if (!messageTicket) {
      const reply = await messageRpc($, {kind: 'poll'});
      if (!reply.reservation) return;
      messageTicket = reply.reservation;
      const ticket = messageTicket, target = ticket.target;
      if (!reply.recorded_idle || !sameMessageReference(target, reply.reference)
          || typeof ticket.binding_id !== 'string' || !ticket.binding_id
          || typeof ticket.reservation_id !== 'string' || !ticket.reservation_id
          || typeof ticket.request?.operation_id !== 'string' || !ticket.request.operation_id
          || typeof ticket.request.content !== 'string' || !ticket.request.content
          || messageBytes(ticket.request.content).length > 65536
          || await messageDigest(ticket.request.content) !== ticket.payload_sha256)
        throw new Error('Message reservation unavailable');
    }
    const ticket = messageTicket;
    const check = await messageRpc($, {kind: 'recheck', operation_id: ticket.request.operation_id,
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
    if (messageAttempts.has(ticket.reservation_id)) throw new Error('Message attempt already used');
    // No await between the last local busy check, attempt marker and native call.
    // Native queuing/admission across competing handlers is still a qualification gate.
    messageAttempts.add(ticket.reservation_id);
    messageInFlight = ticket;
    messageAppendSeen = false;
    let outcome;
    try {
      const result = await $.prompt.submit({text: ticket.request.content});
      const raw = JSON.stringify(result);
      if (typeof raw !== 'string' || messageBytes(raw).length > 98304)
        outcome = {kind: 'uncertain', reason: 'native_receipt_unavailable'};
      else outcome = {kind: 'returned', result};
    } catch {
      outcome = {kind: 'uncertain', reason: 'native_submit_threw'};
    } finally { messageInFlight = undefined; }
    // A return without the native append event supplies no candidate row identity.
    if (!messageAppendSeen) messageStopped = true;
    // Storing the raw return does not turn it into qualified delivery proof.
    await messageRpc($, {kind: 'record_handoff', operation_id: ticket.request.operation_id,
      reservation_id: ticket.reservation_id, outcome});
    messageTicket = undefined;
  } catch {
    // A lost reservation or result may already have crossed a process boundary.
    // Stop this module's pump; never retry I/O, reset a ticket or infer a failure.
    messageStopped = true;
  } finally { messagePolling = false; }
}
async function startMessagePump($) {
  if (messageStarted) { messageStopped = true; throw new Error('Message module already started'); }
  messageStarted = true;
  messageContext = await publishContext($);
  const version = await $.session.version();
  if (!['2.1.296', '2.1.308'].includes(version?.version) || typeof config.pluginName !== 'string'
      || !/^[a-z0-9-]{1,35}$/.test(config.pluginName))
    throw new Error('Native append framing is not qualified for this client version');
  messageProviderVersion = version.version;
  messageActive = false;
  $.clock.every(250, async () => pollMessages($));
}
