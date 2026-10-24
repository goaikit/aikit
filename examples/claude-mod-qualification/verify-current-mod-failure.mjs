import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const hash=text=>createHash('sha256').update(text).digest('hex');
export function verify(root) {
  const ids=[];const counts=[];
  for(const mode of ['pending','busy']) {
    const directory=path.join(root,mode);
    const file=name=>fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,'');
    const read=name=>file('evidence/'+name);
    const json=name=>JSON.parse(read(name));
    const rows=name=>read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
    const container=JSON.parse(file('container.json'))[0];
    assert.equal(container.Image,'sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924');
    assert.equal(container.HostConfig.NetworkMode,'none');assert.deepEqual(container.Mounts,[]);
    assert.equal(container.Config.User,'node');
    assert.equal(read('native-version.txt').trim(),'2.1.308 (Claude Code)');
    assert.equal(read('native-sha256.txt').trim(),'356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58  /usr/local/bin/claude');
    const config=json('case.json');assert.equal(config.mode,mode);
    assert.equal(config.production_activated,false);assert.equal(config.owner_settings_changed,false);
    const source=file('completion-source.js').replaceAll('\r\n','\n');
    assert.equal(hash(source),config.sdk_source_sha256);
    const fallback=source.match(/\}\)\.catch\((\(\) => \(\{ block: '[^']+' \}\))\);\s*$/)[1];
    assert.equal(fallback,config.fallback);
    assert.equal(fallback,"() => ({ block: 'Completion validation unavailable: restore the local handler and retry.' })");
    const reason='Completion validation unavailable: restore the local handler and retry.';
    const guard=file('guard/hooks/register.js');assert.equal(hash(guard),config.guard_sha256);
    assert(guard.includes('}).catch('+fallback+');'));
    assert(guard.includes(mode==='pending'?'await new Promise(()=>{});':'while(true) {}'));
    const events=json('events.json'),starts=events.filter(e=>e.kind==='session_start');assert.equal(starts.length,1);
    const {sessionId,event}=starts[0].value;assert.equal(event.surface,'terminal');assert.equal(event.isInteractive,true);
    assert.match(sessionId,/^[a-f0-9-]{36}$/);assert(!ids.includes(sessionId));ids.push(sessionId);
    const calls=json('guard-calls.json');assert.equal(calls.length,2);
    assert(calls.every(c=>c.event.session_id===sessionId&&c.event.hook_event_name==='Stop'&&c.budget.ms===10000));
    assert.equal(calls[0].event.stop_hook_active,false);assert.equal(calls[1].event.stop_hook_active,true);
    assert(calls[1].at>calls[0].at);assert(calls[1].at-calls[0].at<35000);
    assert.equal(calls[0].event.prompt_id,calls[1].event.prompt_id);
    const completions=events.filter(e=>e.kind==='turn_complete');assert.equal(completions.length,1);
    assert(completions[0].at>=calls[1].at);const complete=completions[0].value;
    assert.equal(complete.event.reason,'answer');assert.equal(complete.event.isAborted,false);
    assert.equal(complete.event.answer,'Fixture answer');assert(complete.event.usage.output_tokens>0);
    assert.deepEqual(complete.origin,{plugin:'engine',tier:'core'});
    const core=complete.trace.filter(t=>t.plugin==='engine'&&t.tier==='core'&&t.event==='turn.complete');
    assert.equal(core.length,1);assert.equal(core[0].outcome,'returned');
    assert.deepEqual(core[0].received,complete.event);assert.deepEqual(core[0].returned,complete.result);
    assert.equal(complete.result.text,'Fixture answer');
    const debug=read('native-debug.log');
    assert(debug.includes('hooks module aikit-mod-guard@inline loaded (worker, environment 1, tier user); events: classic.Stop'));
    assert(debug.includes('hooks module aikit-mod-observer@inline loaded (worker, environment 2, tier user); events: session.start,turn.complete'));
    assert(debug.includes('hook failed closed: aikit-mod-guard:'));
    assert(debug.includes('(classic.Stop; its .catch answered)'));
    if(mode==='pending') {
      assert(calls[1].at-calls[0].at>=10000);
      assert(debug.includes('still running 5000ms after its budget ran out; ignores its signal (classic.Stop)'));
      assert(!debug.includes('.catch asked in the replacement'));
    } else {
      assert(debug.includes('no answer to a heartbeat within 5000ms: the hooks worker is wedged'));
      assert(debug.includes('.catch asked in the replacement after the hooks worker ended during classic.Stop'));
      assert(debug.includes('the hooks worker was replaced during classic.Stop; aikit-mod-guard\'s .catch answered in place of its hook'));
      // Both plugin environments share the worker. This is the replacement's
      // session.start, not an independent pre-loss witness or a second session.
      assert(starts[0].at>calls[0].at);
    }
    const requests=rows('requests.jsonl');assert(requests.length<=32);
    const api=requests.filter(r=>r.method==='POST'&&r.path.startsWith('/v1/messages')&&!r.path.includes('count_tokens'));
    const sources=[...debug.matchAll(/\[API REQUEST\] \/v1\/messages source=(\w+)/g)].map(m=>m[1]);
    assert.equal(api.length,sources.length);assert(sources.every(s=>['repl_main_thread','generate_session_title','hello'].includes(s)));
    const main=api.filter(r=>r.body?.tools?.length>0&&JSON.stringify(r.body.messages).includes('AIKIT_MOD_FAILURE_'+mode.toUpperCase()));
    assert.equal(main.length,2);counts.push(main.length);assert.equal(sources.filter(s=>s==='repl_main_thread').length,2);
    assert(!JSON.stringify(main[0].body.messages).includes(reason));assert(JSON.stringify(main[1].body.messages).includes(reason));
    const transcript=rows('native-transcript.jsonl');
    assert(transcript.some(t=>t.sessionId===sessionId&&t.subtype==='turn_duration'));
    assert(transcript.some(t=>t.sessionId===sessionId&&t.type==='user'&&JSON.stringify(t.message).includes('Stop hook feedback')&&JSON.stringify(t.message).includes(reason)));
  }
  return {sessions:ids,main_requests:counts,constant_fallback_recovers:['native budget','wedged worker replacement'],
    scope:'Claude 2.1.308 Linux native primitives; exact SDK fallback, not full generated bridge, protected ordering or Windows readiness'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify(verify(process.argv[2]),null,2));
