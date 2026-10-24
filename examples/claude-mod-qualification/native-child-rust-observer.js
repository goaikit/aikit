// Fresh native fixture: compare the reusable Rust helper with independent facts.
const expected = '356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58';
export function register(on) {
  on('session.start', async ($, event, next) => {
    const python = await $.process.run(['python3', '/qualification/native-child-probe.py'], { timeoutMs: 10000 });
    if (python.exitCode !== 0 || python.isStdoutTruncated !== false || python.isStderrTruncated !== false)
      throw new Error('Native fact comparison failed');
    const address = await $.env.get('CLAUDE_CODE_MESSAGING_SOCKET');
    const inspect = async hash => {
      const result = await $.process.run(['/qualification/native_caller_inspect', address, hash], { timeoutMs: 10000 });
      if (result.isStdoutTruncated !== false || result.isStderrTruncated !== false)
        throw new Error('Native Rust inspection truncated');
      return { exit_code: result.exitCode, facts: result.exitCode === 0 ? JSON.parse(result.stdout) : null };
    };
    const accepted = await inspect(expected);
    const wrong_hash = await inspect('0'.repeat(64));
    await $.fs.write('/home/node/evidence/native-child-rust.json', JSON.stringify({
      session_id: await $.session.id(), python: JSON.parse(python.stdout), accepted, wrong_hash,
    }));
    return next(event);
  });
}
