import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';

// Verifies retained native observations; does not grant a production capability.
export function verify(root) {
  const ids=[];const counts=[];
  for(const mode of ['baseline','missing','timeout']) {
    const directory=path.join(root,mode+'-root');
    const read=name=>fs.readFileSync(path.join(directory,'evidence',name),'utf8').replace(/^\uFEFF/,'');
    const json=name=>JSON.parse(read(name));
    const rows=name=>read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
    const optional=name=>fs.existsSync(path.join(directory,'evidence',name))?rows(name):[];
    const container=JSON.parse(fs.readFileSync(path.join(directory,'container.json'),'utf8').replace(/^\uFEFF/,''))[0];
    assert.equal(container.Image,'sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924');
    assert.equal(container.HostConfig.NetworkMode,'none');assert.deepEqual(container.Mounts,[]);
    assert.equal(container.Config.User,'node');
    assert.equal(json('case.json').mode,mode);
    assert.equal(json('case.json').production_activated,false);
    assert.equal(json('case.json').owner_settings_changed,false);
    assert.equal(read('native-version.txt').trim(),'2.1.308 (Claude Code)');
    assert.equal(read('native-sha256.txt').trim(),'356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58  /usr/local/bin/claude');
    assert.deepEqual(json('managed-ownership.json'),{uid:0,mode:0o644});
    const script={baseline:'stop-chain-command.mjs',missing:'intentionally-missing-policy.mjs',timeout:'stop-timeout-command.mjs'}[mode];
    const settings={hooks:{Stop:[{hooks:[{type:'command',command:'node /home/node/probe/'+script,timeout:1,onFailure:'block'}]}]}};
    assert.deepEqual(json('expected-settings.json'),settings);
    assert.deepEqual(json('actual-managed-settings.json'),settings);
    const events=json('events.json');
    const starts=events.filter(e=>e.kind==='session_start');assert.equal(starts.length,1);
    const {sessionId,event}=starts[0].value;
    assert.equal(event.isInteractive,true);assert.equal(event.surface,'terminal');
    assert.match(sessionId,/^[a-f0-9-]{36}$/);assert(!ids.includes(sessionId));ids.push(sessionId);
    assert.equal(events.filter(e=>e.kind==='short_circuit_stop').length,0);
    const completed=events.filter(e=>e.kind==='turn_complete');assert.equal(completed.length,1);
    const complete=completed[0].value;
    assert.equal(complete.event.reason,'answer');assert.equal(complete.event.isAborted,false);
    assert.equal(complete.event.answer,'Fixture answer');assert(complete.event.usage.output_tokens>0);
    assert.deepEqual(complete.origin,{plugin:'engine',tier:'core'});
    const core=complete.trace.filter(t=>t.plugin==='engine'&&t.tier==='core'&&t.event==='turn.complete');
    assert.equal(core.length,1);assert.equal(core[0].outcome,'returned');
    assert.deepEqual(core[0].received,complete.event);assert.deepEqual(core[0].returned,complete.result);
    assert.equal(complete.result.text,'Fixture answer');
    const debug=read('native-debug.log');
    assert(debug.includes('cc-plugin-sec-default@builtin seated outermost: this machine has managed settings'));
    assert(debug.includes('classic.Stop bypassed by cc-plugin-sec-default (tier user); beneath runs'));
    const transcript=rows('native-transcript.jsonl');
    assert(transcript.some(t=>t.sessionId===sessionId&&t.subtype==='turn_duration'));
    const requests=rows('requests.jsonl');assert(requests.length<=32);
    const api=requests.filter(r=>r.method==='POST'&&r.path.startsWith('/v1/messages')&&!r.path.includes('count_tokens'));
    const sources=[...debug.matchAll(/\[API REQUEST\] \/v1\/messages source=(\w+)/g)].map(m=>m[1]);
    assert.equal(api.length,sources.length);
    assert(sources.every(s=>['repl_main_thread','generate_session_title','hello'].includes(s)));
    const main=api.filter(r=>r.body?.tools?.length>0&&JSON.stringify(r.body.messages).includes('AIKIT_STOP_FAILURE_'+mode.toUpperCase()));
    assert.equal(main.length,sources.filter(s=>s==='repl_main_thread').length);counts.push(main.length);
    assert.equal(new Set(requests.map(r=>r.epoch)).size,1);
    const calls=optional('command-stops.jsonl');const timeout=optional('timeout-command.jsonl');
    if(mode==='baseline') {
      assert.equal(main.length,2);assert.equal(timeout.length,0);
      assert.deepEqual(calls.map(c=>[c.ordinal,c.result]),[[1,{decision:'block',reason:'AIKIT_STOP_CHAIN_FIRST_BLOCK'}],[2,{}]]);
      assert(calls.every(c=>c.input.session_id===sessionId&&c.input.hook_event_name==='Stop'));
      assert(JSON.stringify(main[1].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(transcript.some(t=>t.attachment?.type==='hook_blocking_error'));
      assert(transcript.some(t=>t.attachment?.type==='hook_success'&&t.attachment.exitCode===0));
    } else {
      assert.equal(main.length,1);assert.equal(calls.length,0);
      assert(!transcript.some(t=>t.attachment?.type==='hook_blocking_error'));
      assert(debug.includes('not blocking (onFailure: "block" is ignored on Stop)'));
      if(mode==='missing') {
        assert.equal(timeout.length,0);assert(debug.includes('MODULE_NOT_FOUND'));
        assert(debug.includes('status code 1'));assert(debug.includes('Stop command hook failed; not blocking'));
      } else {
        assert(debug.includes('timed out after 1000ms'));assert(debug.includes('Stop command hook timed out; not blocking'));
        assert.equal(timeout.length,1);assert.equal(timeout[0].phase,'started');
        assert.equal(timeout[0].input.session_id,sessionId);assert.equal(timeout[0].input.hook_event_name,'Stop');
        assert(transcript.some(t=>t.attachment?.type==='hook_cancelled'));
      }
    }
  }
  return {sessions:ids,main_requests:counts,native_stop_failure_allows_completion:true,
    scope:'Claude 2.1.308 Linux x86-64; root managed command hooks; no Codelaya or Windows readiness claim'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]),null,2));
