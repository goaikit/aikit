// Deterministic local fixture. No proxying, external requests or real credentials.
// Run only in a disposable home; request bodies can contain system/tool context.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';

const evidence = process.argv[2];
const fixtureKey = 'sk-ant-aikit-local-fixture-not-a-credential';
if (!evidence || !path.isAbsolute(evidence)) throw new Error('absolute evidence directory required');
fs.mkdirSync(evidence, { recursive: true });
let requests = 0;
const epoch = randomUUID();
const record = (entry) => fs.appendFileSync(path.join(evidence, 'requests.jsonl'), JSON.stringify({ epoch, ...entry }) + '\n');
const json = (response, status, body) => {
  response.writeHead(status, { 'content-type': 'application/json' });
  response.end(JSON.stringify(body));
};

http.createServer(async (request, response) => {
  if (++requests > 32) return json(response, 429, { type: 'error', error: { type: 'rate_limit_error', message: 'fixture request ceiling' } });
  const chunks = [];
  let bytes = 0;
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > 4 * 1024 * 1024) return json(response, 413, { error: 'fixture body ceiling' });
    chunks.push(chunk);
  }
  let body;
  try { body = bytes ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : null; }
  catch { return json(response, 400, { error: 'invalid fixture JSON' }); }
  if (request.method === 'POST' && request.headers['x-api-key'] !== fixtureKey)
    return json(response, 401, { type: 'error', error: { type: 'authentication_error', message: 'only the public fixture key is accepted' } });
  record({ sequence: requests, method: request.method, path: request.url, body });
  if (request.url.startsWith('/v1/messages/count_tokens')) return json(response, 200, { input_tokens: 100 });
  if (request.url.startsWith('/v1/messages') && request.method === 'POST') {
    const probe = body.tools?.some(tool => tool.name === 'mcp__aikit-mod-feasibility__echo')
      && JSON.stringify(body.messages).includes('AIKIT_NATIVE_TOOL_PROBE');
    const answeredTool = JSON.stringify(body.messages).includes('fixture-native-echo');
    const toolCall = probe && !answeredTool;
    const content = toolCall ? [{ type: 'tool_use', id: 'fixture-native-echo',
      name: 'mcp__aikit-mod-feasibility__echo', input: { text: 'Native tool stdin 雪\n' } }] : [{ type: 'text', text: 'Fixture answer' }];
    const message = { id: `msg_fixture_${requests}`, type: 'message', role: 'assistant', model: body.model,
      content, stop_reason: toolCall ? 'tool_use' : 'end_turn', stop_sequence: null,
      usage: { input_tokens: 100, output_tokens: 3 } };
    if (!body.stream) return json(response, 200, message);
    response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
    const event = (type, data) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`);
    event('message_start', { message: { ...message, content: [], stop_reason: null, usage: { input_tokens: 100, output_tokens: 0 } } });
    event('content_block_start', { index: 0, content_block: toolCall ? { ...content[0], input: {} } : { type: 'text', text: '' } });
    event('content_block_delta', { index: 0, delta: toolCall ? { type: 'input_json_delta', partial_json: JSON.stringify(content[0].input) } : { type: 'text_delta', text: 'Fixture answer' } });
    event('content_block_stop', { index: 0 });
    event('message_delta', { delta: { stop_reason: message.stop_reason, stop_sequence: null }, usage: { output_tokens: 3 } });
    event('message_stop', {});
    return response.end();
  }
  if (request.method === 'GET' && request.url === '/') return json(response, 200, { fixture: true });
  return json(response, 404, { type: 'error', error: { type: 'not_found_error', message: 'unsupported fixture route' } });
}).listen(8787, '127.0.0.1', () => console.log('fixture listening on 127.0.0.1:8787'));
