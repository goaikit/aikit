// Explicitly opt-in native qualification fixture, never a production adapter.
const events = [];
async function record($, kind, value) {
  events.push({ kind, value });
  await $.fs.write('/home/node/evidence/native-events.json', JSON.stringify(events));
}
async function echo($, text) {
  const input = JSON.stringify({ text, sessionId: await $.session.id() });
  const result = await $.process.run(['node', '-e', 'process.stdin.pipe(process.stdout)'],
    { stdin: input, timeoutMs: 1000 });
  if (result.exitCode !== 0 || result.isStdoutTruncated || result.stdout !== input)
    throw new Error('native stdin echo did not match');
  await record($, 'process_stdin', { input, stdout: result.stdout, exitCode: result.exitCode });
  return result.stdout;
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    await $.command.register({ name: 'aikit-native-probe', description: 'Run the isolated native stdin qualification fixture' });
    await $.tool.register({ name: 'echo', description: 'Isolated qualification echo; no repository changes',
      isDeferred: false, inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'], additionalProperties: false } });
    await record($, 'session_start', { sessionId: await $.session.id(), event });
    return next(event);
  });
  on('command.run', { command: 'aikit-native-probe' }, async ($) =>
    ({ text: await echo($, 'Native command stdin 雪\n') }));
  on('tool.call', { tool: 'mcp__aikit-mod-feasibility__echo' }, async ($, event) =>
    ({ result: await echo($, event.text) }));
  on('turn.start', async ($, event, next) => {
    await record($, 'turn_start', event);
    return next(event);
  });
  on('turn.complete', async ($, event, next) => {
    await record($, 'turn_complete', event);
    return next(event);
  });
}
