// Opt-in interruption fixture: kill the OWNED container at stored.json, then
// resume the original native session. Never installed in an owner workspace.
const payload = 'AIKIT_APPEND_INTERRUPTION: preserve exact 雪\nreview';
const operationId = 'synthetic-review-interruption-once';
let attempted = true;
const events = [];
let writes = Promise.resolve();
async function record($, kind, value) {
  events.push({sequence: events.length + 1, kind, value});
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write('/home/node/evidence/interruption-events.json', bytes));
  await writes;
}
async function checkpoint($, action, value) {
  const result = await $.process.run(['node', '/home/node/probe/append-interruption-journal.mjs', action],
    {stdin: JSON.stringify(value), timeoutMs: 5000});
  if (result.exitCode !== 0 || result.isStdoutTruncated !== false || result.isStderrTruncated !== false)
    throw new Error('Diagnostic checkpoint unavailable');
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    const sessionId = await $.session.id();
    await $.command.register({name: 'aikit-interrupt-append', description: 'One synthetic interruption probe'});
    await $.tool.register({name: 'interruption_probe', description: 'Diagnostic marker; do not call', isDeferred: false,
      inputSchema: {type: 'object', properties: {}, additionalProperties: false}});
    await record($, 'session_start', {sessionId, version: await $.session.version()});
    const files = await $.fs.list('/home/node/evidence');
    if (!files.some(file => file.name === 'claim.json')) { attempted = false; return next(event); }
    const claim = JSON.parse(await $.fs.read('/home/node/evidence/claim.json'));
    if (claim.operationId !== operationId || claim.payload !== payload || claim.sessionId !== sessionId)
      throw new Error('Diagnostic resume scope changed');
    attempted = true;
    const messages = await $.session.messages();
    await checkpoint($, 'resumed', {sessionId, operationId, messages});
    await record($, 'resumed_without_submit', {sessionId, operationId});
    return next(event);
  });
  on('command.run', {command: 'aikit-interrupt-append'}, async ($) => {
    if (attempted) return {text: 'Attempt already claimed; never resend uncertain input.'};
    attempted = true;
    const sessionId = await $.session.id();
    await checkpoint($, 'claim', {sessionId, operationId, payload});
    $.clock.after(10, async () => {
      await record($, 'submit_attempt', {sessionId, operationId, payload});
      try {
        const result = await $.prompt.submit({text: payload});
        await record($, 'submit_return', {sessionId, result});
      } catch { await record($, 'submit_uncertain', {sessionId}); }
    });
    return {};
  });
  on('session.append', {door: 'prompt'}, async ($, event, next) => {
    if (event.agentId !== undefined || event.origin?.kind !== 'plugin'
        || event.origin.name !== 'aikit-mod-feasibility') return next(event);
    const sessionId = await $.session.id();
    await checkpoint($, 'prepared', {sessionId, operationId, payload, event});
    const result = await next(event);
    await checkpoint($, 'stored', {sessionId, operationId, result});
    // This $ call holds the append callback after storage but before submit returns.
    // The test controller must kill this owned container as soon as stored.json exists.
    await $.process.run(['node', '/home/node/probe/append-interruption-journal.mjs', 'hold'], {timeoutMs: 60000});
    return result;
  });
  on('turn.start', async ($, event, next) => {
    await record($, 'turn_start', {sessionId: await $.session.id(), event});
    return next(event);
  });
}
