// Opt-in native qualification fixture. Never install as an application adapter.
const events = [];
let writes = Promise.resolve();
async function record($, kind, value) {
  events.push({ kind, value });
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write('/home/node/evidence/short-circuit-events.json', bytes));
  await writes;
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    await record($, 'session_start', { sessionId: await $.session.id(), event });
    return next(event);
  });
  on('classic.Stop', async ($, event, _next) => {
    await record($, 'short_circuit_stop', event);
    return {}; // Deliberately do not invoke the next middleware handler.
  });
  on('turn.complete', async ($, event, next) => {
    await record($, 'turn_complete', event);
    return next(event);
  });
}
