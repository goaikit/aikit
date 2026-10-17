// Opt-in native handoff candidate. Recorded checks are not a native lease.
// Returned receipts remain unqualified; no acknowledgement/consumption is inferred.
let messageActive = true, messageStopped = false, messageStarted = false, messagePolling = false;
let messageContext, messageTicket;
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
    let outcome;
    try {
      const result = await $.prompt.submit({text: ticket.request.content});
      const raw = JSON.stringify(result);
      if (typeof raw !== 'string' || messageBytes(raw).length > 98304)
        outcome = {kind: 'uncertain', reason: 'native_receipt_unavailable'};
      else outcome = {kind: 'returned', result};
    } catch {
      outcome = {kind: 'uncertain', reason: 'native_submit_threw'};
    }
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
  messageActive = false;
  $.clock.every(250, async () => pollMessages($));
}
