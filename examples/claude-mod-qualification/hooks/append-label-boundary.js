// Opt-in native framing experiment only. ASCII manifest names of lengths
// 21, 22, 23, 24, 25 and 35; run one command at a time against the offline fixture.
const events = [];
let writes = Promise.resolve(), attempted = false;
async function record($, kind, value) {
  events.push({kind, value});
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write(`/home/node/label-evidence/${$.plugin.name}.json`, bytes));
  await writes;
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    await $.command.register({name: `label-${$.plugin.name.length}`, description: 'One synthetic native label boundary check'});
    return next(event);
  });
  on('command.run', async ($, event, next) => {
    if (event.command !== `label-${$.plugin.name.length}`) return next(event);
    if (attempted) return {text: 'Already attempted'};
    attempted = true;
    $.clock.after(10, async () => {
      const payload = `AIKIT_LABEL_BOUNDARY:${$.plugin.name}`;
      const result = await $.prompt.submit({text: payload});
      await record($, 'submit_return', {sessionId: await $.session.id(), payload, result});
    });
    return {};
  });
  on('session.append', {door: 'prompt'}, async ($, event, next) => {
    const result = await next(event);
    if (event.origin?.kind === 'plugin' && event.origin.name === $.plugin.name)
      await record($, 'append_return', {sessionId: await $.session.id(), event, result});
    return result;
  });
}
