// Opt-in actual server + native fixture tests. No credentials or model calls.
// AIKIT_HTTP_TEST_BIN=target/debug/aikit node --test examples/gateway-server.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import net from 'node:net';
import { AgentHost } from './gateway-client.mjs';

const binary = process.env.AIKIT_HTTP_TEST_BIN;
const owner = 'test-owner-credential-not-for-production';
const readerToken = 'test-read-only-credential-0000000000';
const executorToken = 'test-execute-only-credential-0000000';
const responderToken = 'test-respond-only-credential-0000000';
const delay = ms => new Promise(r => setTimeout(r, ms));
async function eventually(fn) {
  const until = Date.now() + 15_000; let last;
  while (Date.now() < until) {
    try { const value = await fn(); if (value) return value; } catch (e) { last = e; }
    await delay(30);
  }
  throw last ?? new Error('condition timed out');
}
async function port() {
  const s = net.createServer(); s.listen(0, '127.0.0.1'); await once(s, 'listening');
  const p = s.address().port; await new Promise(r => s.close(r)); return p;
}
async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, 'exit'); child.kill('SIGKILL'); await exited;
}

test('actual HTTP client, scoped auth, reconnect and crash recovery', { skip: !binary, timeout: 120_000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'aikit-gateway-http-'));
  const peer = join(dir, process.platform === 'win32' ? 'peer.exe' : 'peer');
  const compiled = spawnSync('rustc', ['--edition=2021', 'tests/fixtures/acp_peer.rs', '-o', peer], { encoding: 'utf8', windowsHide: true });
  assert.equal(compiled.status, 0, compiled.stderr);
  const tokens = join(dir, 'tokens.json');
  await writeFile(tokens, JSON.stringify([{token: readerToken, scopes:['read']}, {token: responderToken, scopes:['respond']}, {token: executorToken, scopes:['execute']}]));
  const audit = join(dir, 'native.log'); let child; let client;
  async function start(stall = false) {
    const p = await port(); const base = `http://127.0.0.1:${p}`;
    const env = {...process.env, AIKIT_GATEWAY_DATA:join(dir,'data'), AIKIT_GATEWAY_WORKSPACES:JSON.stringify([dir]), AIKIT_GATEWAY_TOKENS_FILE:tokens, AIKIT_GATEWAY_ORIGINS:JSON.stringify(['https://client.example']), AIKIT_CURSOR_BIN:peer, AIKIT_CURSOR_ARGS:'[]', AIKIT_FIXTURE_AUDIT:audit};
    delete env.AIKIT_FIXTURE_STALL_INIT; if (stall) env.AIKIT_FIXTURE_STALL_INIT='1';
    child = spawn(resolve(binary), ['serve','--host','127.0.0.1','--port',String(p),'--api-key',owner], { env, windowsHide:true, stdio:['ignore','pipe','pipe'] });
    let output=''; child.stdout.on('data', b => {output += b;}); child.stderr.on('data', b => {output += b;});
    client = new AgentHost(base, owner);
    await eventually(async () => {if (child.exitCode !== null) throw new Error(output); return await client.request('');});
    return base;
  }
  const req = key => ({command_id:key, backend:'cursor',cwd:dir,prompt:'fixture',permission_policy:'ask'});
  try {
    const base = await start();
    assert.equal((await fetch(base+'/api/v1/gateway')).status,401);
    assert.equal((await fetch(base+'/api/v1/gateway',{headers:{Authorization:`Bearer ${owner}`,Origin:'https://bad.example'}})).status,401);
    const preflight = await fetch(base+'/api/v1/gateway/sessions',{method:'OPTIONS',headers:{Origin:'https://client.example','Access-Control-Request-Method':'POST'}});
    assert.equal(preflight.status,204); assert.equal(preflight.headers.get('access-control-allow-origin'),'https://client.example');
    const reader = new AgentHost(base,readerToken); await reader.request('/metrics');
    await assert.rejects(reader.create(req('forbidden')),/401/);
    const created = await client.create(req('create'));
    assert.equal((await client.create(req('create'))).session_id,created.session_id);
    await assert.rejects(client.create({...req('create'),prompt:'different'}),e => e.status===409 && e.code==='idempotency_conflict' && e.retry==='never');
    const id = created.session_id;
    let cursor=0;
    for await (const event of client.events(id,0,AbortSignal.timeout(10_000))) {cursor=event.sequence;break;}
    const request = await eventually(async () => (await client.request(`/sessions/${id}/requests`))[0]);
    assert.equal((await client.request(`/sessions/${id}`)).status,'running');
    const executor = new AgentHost(base,executorToken);
    await assert.rejects(executor.command(id,{command_id:'bypass',type:'respond',request_id:request.request_id,response:{type:'allow'}}),/401/);
    const responder = new AgentHost(base,responderToken);
    await responder.request(`/sessions/${id}/requests/${request.request_id}/response`,{command_id:'answer',response:{type:'allow'}});
    await eventually(async () => (await client.request(`/sessions/${id}`)).status==='idle');
    let terminal=false;
    for await (const event of client.events(id,cursor,AbortSignal.timeout(10_000))) {
      assert.ok(event.sequence>cursor);cursor=event.sequence;
      if (event.type==='agent' && event.payload?.payload?.terminal) {terminal=true;break;}
    }
    assert.ok(terminal);
    const late=await responder.request(`/sessions/${id}/requests/${request.request_id}/response`,{command_id:'late',response:{type:'allow'}});
    assert.equal(late.status,'failed');
    assert.equal(late.failure.code,'request_expired_or_resolved');
    assert.equal(late.failure.retry,'never');
    const metrics = await reader.request('/metrics');
    assert.equal(metrics.active_sessions,1);
    assert.equal(metrics.pending_requests,0);
    // Crash after dispatch while a second turn is waiting for approval.
    await client.command(id,{command_id:'turn-2',type:'send_turn',text:'second'});
    await eventually(async () => (await client.request(`/sessions/${id}/requests`)).length===1);
    const before = await readFile(audit,'utf8'); await stop(child); await start();
    assert.equal((await client.request(`/sessions/${id}`)).status,'interrupted');
    assert.deepEqual(await client.request(`/sessions/${id}/requests`),[]);
    assert.equal((await client.create(req('create'))).session_id,id);
    assert.equal(await readFile(audit,'utf8'),before,'restart/retry must not dispatch native work');
    // Crash after durable acceptance but before the adapter finishes startup.
    await stop(child); await start(true);
    const opening=await client.create(req('opening'));
    await eventually(async () => (await readFile(audit,'utf8')).length>before.length);
    const queued=await client.command(opening.session_id,{command_id:'queued',type:'send_turn',text:'never replay'});
    assert.equal(queued.status,'accepted');
    await stop(child); const checkpoint=await readFile(audit,'utf8'); await start();
    const uncertain=await client.request('/commands/opening');
    assert.equal(uncertain.status,'outcome_unknown');
    assert.equal(uncertain.failure.retry,'inspect_receipt');
    assert.equal((await client.request(`/sessions/${opening.session_id}/commands/queued`)).status,'outcome_unknown');
    assert.equal((await client.create(req('opening'))).session_id,opening.session_id);
    assert.equal(await readFile(audit,'utf8'),checkpoint);
  } finally {
    if (child) await stop(child);
    await rm(dir,{recursive:true,force:true,maxRetries:10,retryDelay:100});
  }
});
