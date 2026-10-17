// Offline native engine cases for the SDK-generated fixture plugin.
import { expect, test } from 'claude-code/testing';
const tool = 'mcp__aikit-native-bridge-fixtur-15198397__review';
const payload = { request: { text: 'fixture 雪' }, tool };
const output = (stdout, exitCode = 0, truncated = false) => ({ value: {
  exitCode, stdout, stderr: '', isStdoutTruncated: truncated, isStderrTruncated: false,
} });

test('native context stays separate from model payload and uses literal argv/stdin', async ($, on) => {
  on('session.id', () => ({ value: 'native-session' }));
  on('session.cwd', () => ({ value: '/fixture' }));
  on('process.run', ($, event) => {
    expect(event.argv).toEqual(['/generated/agent_tool_plugin', 'inspect']);
    return output(event.init.stdin);
  });
  const result = await $.tool.call(payload);
  const call = JSON.parse(result.result);
  expect(call.native_context.session_id).toBe('native-session');
  expect(call.native_context.cwd).toBe('/fixture');
  expect(call.request).toEqual({ text: 'fixture 雪' });
});

test('structured application failure remains a tool error', async ($, on) => {
  on('session.id', () => ({ value: 'native-session' }));
  on('session.cwd', () => ({ value: '/fixture' }));
  on('process.run', () => output('{"code":"stale_revision"}', 2));
  const result = await $.tool.call(payload);
  expect(result.isError).toBe(true);
  expect(JSON.parse(result.result).code).toBe('stale_revision');
});

test('truncated output refuses without a second process attempt', async ($, on) => {
  let calls = 0;
  on('session.id', () => ({ value: 'native-session' }));
  on('session.cwd', () => ({ value: '/fixture' }));
  on('process.run', () => { calls++; return output('{}', 0, true); });
  const result = await $.tool.call(payload);
  expect(result.deny).toContain('Retain an uncertain operation ID');
  expect(calls).toBe(1);
});

test('process exception is redacted and never retried', async ($, on) => {
  let calls = 0;
  on('session.id', () => ({ value: 'native-session' }));
  on('session.cwd', () => ({ value: '/fixture' }));
  on('process.run', () => { calls++; throw new Error('private-fixture-token'); });
  const result = await $.tool.call(payload);
  expect(result.deny).toContain('Retain an uncertain operation ID');
  expect(result.deny).not.toContain('private-fixture-token');
  expect(calls).toBe(1);
});

test('application instructions append without replacing existing blocks or template-like data', async ($, on) => {
  on('prompt.context', () => ({ blocks: [{ name: 'other', text: 'keep exact' }] }));
  const result = await $.prompt.context({ blocks: [] });
  expect(result.blocks[0]).toEqual({ name: 'other', text: 'keep exact' });
  expect(result.blocks[1].text).toContain("Literal 'AIKIT_NATIVE_TOOL_NAME' stays data");
});

test('subagent authoring refuses before executing a process', async ($, on) => {
  let calls = 0;
  on('process.run', () => { calls++; return output('{}'); });
  const result = await $.tool.call({ ...payload, agentId: 'child' });
  expect(result.deny).toContain('Subagent authoring is not qualified');
  expect(calls).toBe(0);
});
