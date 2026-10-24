export function register(on) {
  on('session.start', async ($, event, next) => {
    const result = await $.process.run(['python3', '/qualification/native-child-probe.py'], { timeoutMs: 5000 });
    if (result.exitCode !== 0 || result.isStdoutTruncated !== false || result.isStderrTruncated !== false)
      throw new Error('Bounded native child probe failed');
    const facts = JSON.parse(result.stdout);
    await $.fs.write('/home/node/evidence/native-child.json', JSON.stringify({ session_id: await $.session.id(), facts }));
    return next(event);
  });
}
