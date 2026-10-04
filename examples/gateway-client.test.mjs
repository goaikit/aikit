import test from 'node:test';
import assert from 'node:assert/strict';
import { AgentHost } from './gateway-client.mjs';

test('client preserves command IDs and authorization', async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async (url, options) => {
    assert.equal(url, 'https://host/api/v1/gateway/sessions/session-1/commands');
    assert.equal(options.headers.Authorization, 'Bearer token');
    assert.deepEqual(JSON.parse(options.body), { command_id: 'retry-same-id', type: 'interrupt' });
    return Response.json({ status: 'accepted' });
  };
  try { assert.equal((await new AgentHost('https://host/', 'token').command('session-1', { command_id: 'retry-same-id', type: 'interrupt' })).status, 'accepted'); }
  finally { globalThis.fetch = previous; }
});

test('SSE handles fragmented UTF-8, replay duplicates and cursor', async () => {
  const previous = globalThis.fetch;
  const bytes = new TextEncoder().encode('event: session_event\r\ndata: {"sequence":1}\r\n\r\nevent: session_event\ndata: {"sequence":2,"text":"🌍"}\n\n');
  globalThis.fetch = async url => {
    assert.ok(url.endsWith('/events?after=1'));
    return new Response(new ReadableStream({ start(controller) { for (const byte of bytes) controller.enqueue(Uint8Array.of(byte)); controller.close(); } }));
  };
  try { const events = []; for await (const event of new AgentHost('https://host', 'token').events('s', 1)) events.push(event); assert.deepEqual(events, [{ sequence: 2, text: '🌍' }]); }
  finally { globalThis.fetch = previous; }
});

test('expired cursor requires explicit resynchronization', async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async () => new Response('cursor_expired', { status: 410 });
  try { await assert.rejects(async () => { for await (const event of new AgentHost('https://host', 'token').events('s')) void event; }, /410/); }
  finally { globalThis.fetch = previous; }
});
