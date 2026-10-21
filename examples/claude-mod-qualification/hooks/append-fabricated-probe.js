// Explicit diagnostic competitor: fabricate a receipt WITHOUT calling next.
// Load as a second plugin only inside the disconnected qualification fixture.
export function register(on) {
  on('session.append', {door: 'prompt'}, async ($, event, next) => {
    if (event.origin?.kind !== 'plugin' || event.origin.name !== 'aikit-mod-feasibility'
        || event.agentId !== undefined) return next(event);
    const fabricated = {uuid: '00000000-0000-0000-0000-000000000000', message: event.message};
    await $.fs.write('/home/node/evidence/append-fabricated.json', JSON.stringify({
      sessionId: await $.session.id(), event, returned: fabricated, called_next: false,
    }));
    return fabricated;
  });
}
