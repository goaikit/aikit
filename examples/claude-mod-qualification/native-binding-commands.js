export function register(on) {
  on('session.start', async ($, event, next) => {
    for (const name of ['caller-original', 'caller-current'])
      await $.command.register({ name, description: 'Disposable native caller admission check' });
    return next(event);
  });
  for (const name of ['caller-original', 'caller-current']) on('command.run', { command: name }, async ($) => {
    const result = await $.process.run(['/qualification/native_caller_binding', name === 'caller-original' ? 'check-original' : 'check-current',
      '/home/node/workspace', '/home/node/sdk-state'], { timeoutMs: 10000 });
    const rows = await $.fs.exists('/home/node/evidence/command-checks.json')
      ? JSON.parse(await $.fs.read('/home/node/evidence/command-checks.json')) : [];
    rows.push({ command: name, exit_code: result.exitCode, truncated: result.isStdoutTruncated !== false || result.isStderrTruncated !== false,
      result: result.exitCode === 0 ? JSON.parse(result.stdout) : null });
    await $.fs.write('/home/node/evidence/command-checks.json', JSON.stringify(rows));
    return {};
  });
}
