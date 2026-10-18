// Explicit opt-in native API qualification. Fixed synthetic content, one attempt.
// No production receipt, consumption claim, external network or model API call.
const payload = 'AIKIT_APPEND_CANDIDATE: preserve exact 雪\nreview';
const events = [];
let attempted = false;
let writes = Promise.resolve();
async function record($, kind, value) {
  events.push({sequence: events.length + 1, kind, value});
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write('/home/node/evidence/append-events.json', bytes));
  await writes;
}
async function submit($) {
  await record($, 'submit_attempt', {sessionId: await $.session.id(), payload});
  try {
    const result = await $.prompt.submit({text: payload});
    await record($, 'submit_return', {sessionId: await $.session.id(), result});
  } catch {
    await record($, 'submit_uncertain', {sessionId: await $.session.id()});
  }
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    await $.command.register({name: 'aikit-append-feedback', description: 'Submit one synthetic native append qualification prompt'});
    await $.tool.register({name: 'append_probe', description: 'Diagnostic marker; do not call this tool', isDeferred: false,
      inputSchema: {type: 'object', properties: {}, additionalProperties: false}});
    await record($, 'session_start', {sessionId: await $.session.id(), version: await $.session.version()});
    return next(event);
  });
  on('command.run', {command: 'aikit-append-feedback'}, async ($) => {
    if (attempted) return {text: 'Already attempted; no automatic retry.'};
    attempted = true;
    $.clock.after(10, async () => submit($));
    return {};
  });
  on('tool.call', {tool: 'mcp__aikit-mod-feasibility__append_probe'}, async () => ({deny: 'Diagnostic marker only.'}));
  on('session.append', async ($, event, next) => {
    const result = await next(event);
    if (event.agentId === undefined && event.door === 'prompt' && event.origin?.kind === 'plugin'
        && event.origin.name === 'aikit-mod-feasibility')
      await record($, 'append_return', {sessionId: await $.session.id(), event, result});
    return result;
  });
  on('turn.start', async ($, event, next) => {
    if (event.agentId === undefined) await record($, 'turn_start', {sessionId: await $.session.id(), event});
    return next(event);
  });
  on('turn.complete', async ($, event, next) => {
    const result = await next(event);
    if (event.agentId === undefined) await record($, 'turn_complete', {sessionId: await $.session.id(), event});
    return result;
  });
  on('session.end', async ($, event, next) => {
    await record($, 'session_end', {sessionId: await $.session.id(), event});
    return next(event);
  });
}
