// Disposable native diagnostic. Never install as a production adapter.
const events = [];
let writes = Promise.resolve();
let stops = 0;
let mode;
async function record($, kind, value) {
  events.push({ kind, at: Date.now(), value });
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write('/home/node/evidence/stop-catch-events.json', bytes));
  await writes;
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    mode = (await $.fs.read('/home/node/evidence/case.txt')).trim();
    if (!['throw', 'timeout', 'catch-failure'].includes(mode)) throw new Error('invalid case');
    await record($, 'session_start', { mode, sessionId: await $.session.id(), event });
    return next(event);
  });
  on('classic.Stop', async ($, event, next) => {
    stops += 1;
    await record($, 'stop', { mode, stops, event });
    if (stops > 1) return next(event);
    if (mode === 'timeout') await new Promise(() => {});
    throw new Error('AIKIT_STOP_CATCH_INJECTED_FAILURE');
  }).catch(async ($, event, next) => {
    await record($, 'catch', { mode, stops, error: next.error, called: next.called, event });
    if (mode === 'catch-failure') throw new Error('AIKIT_STOP_CATCH_FALLBACK_FAILURE');
    const result = { block: 'AIKIT_STOP_CATCH_BLOCK' };
    await record($, 'catch_block', { mode, result });
    return result;
  });
  on('turn.complete', async ($, event, next) => {
    await record($, 'turn_complete', event);
    return next(event);
  });
}
