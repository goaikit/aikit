// Read-only verification of one immutable native case. Never grants readiness.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const root=process.argv[2]??'/home/node/evidence';
const read=name=>fs.readFileSync(root+'/'+name,'utf8');
const json=name=>JSON.parse(read(name));
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const setup=json('setup.json'), outcomes=json('native-outcomes.json');
assert.equal(hash('/usr/local/bin/claude'),setup.native_sha256);
assert.deepEqual(JSON.parse(fs.readFileSync('/home/node/.claude/settings.json')),setup.settings);
assert.deepEqual(JSON.parse(fs.readFileSync('/etc/claude-code/managed-settings.json')),setup.managed);
assert.deepEqual(json('native-policy.json'),setup.managed);
const stat=fs.statSync('/etc/claude-code/managed-settings.json');
assert.equal(stat.uid,0);assert.equal(stat.mode&0o222,0o200);
assert.throws(()=>fs.accessSync('/etc/claude-code/managed-settings.json',fs.constants.W_OK));
for(const [name,digest] of Object.entries(setup.source_sha256))
  assert.equal(hash('/home/node/workspace/'+name+'/hooks/register.js'),digest);
const debug=read('native-debug.log');
assert.match(debug,/cc-plugin-sec-default@builtin seated outermost: this machine has managed settings/);
assert.match(debug,/hooks module cc-plugin-sec-default@builtin loaded \(native, environment \d+, tier prepend\)/);
assert.ok(debug.includes(`tier user); events: classic.${setup.event}`));
assert.ok(!fs.existsSync(root+'/user-stop-intercept.json'),'competing user hook must not run');
assert.match(read('native-terminal.log'),/COMMAND_EXIT_CODE="0"/);
const rows=fs.existsSync(root+'/requests.jsonl')?read('requests.jsonl').trim().split('\n').filter(Boolean).map(JSON.parse):[];
assert.ok(rows.length<=8);
const main=rows.filter(r=>r.path.startsWith('/v1/messages')&&Array.isArray(r.body.tools)&&r.body.tools.length>0);
const start=outcomes.filter(o=>o.kind==='session.start');assert.equal(start.length,1);
for(const o of outcomes){assert.deepEqual(o.origin,{plugin:'engine',tier:'core'});assert.equal(o.session_id,start[0].session_id);}
const starts=outcomes.filter(o=>o.kind==='turn.start'),ends=outcomes.filter(o=>o.kind==='turn.complete');
if(setup.mode==='input-block'){
  assert.equal(rows.length,0);assert.equal(starts.length,0);assert.equal(ends.length,0);
  assert.match(read('native-terminal.log'),/blocking because onFailure is/);
}else{
  assert.equal(starts.length,1);assert.equal(ends.length,1);
  const end=ends[0];assert.equal(end.event.turnId,starts[0].event.turnId);
  assert.ok(end.trace.some(t=>t.plugin==='engine'&&t.tier==='core'&&t.event==='turn.complete'&&t.outcome==='returned'));
  const blocked=['block','fallback'].includes(setup.mode);
  assert.equal(end.event.reason,blocked?'aborted':'answer');assert.equal(end.event.isAborted,blocked);
  assert.equal(main.length,blocked||setup.event==='PreToolUse'?2:1);
  if(blocked){assert.equal(json('blocked-request.json').sequence,main[1].sequence);
    assert.equal(json('held-request-closed.json').sequence,main[1].sequence);}
  else assert.ok(!fs.existsSync(root+'/blocked-request.json'));
  if(setup.event==='PreToolUse'){
    const results=main[1].body.messages.flatMap(m=>Array.isArray(m.content)?m.content:[])
      .filter(b=>b.type==='tool_result'&&b.tool_use_id==='probe_read');assert.equal(results.length,1);
    const result=results[0];
    if(setup.mode==='tool-block'){
      assert.equal(result.is_error,true);assert.match(result.content,/blocking because onFailure is "block"/);
      assert.ok(!result.content.includes('disposable managed Stop probe'));
    }else{assert.ok(!result.is_error);assert.match(result.content,/disposable managed Stop probe/);}
  }
}
if(setup.fault==='timeout'){
  assert.equal(read('managed-command-started.txt'),'started\n');assert.match(debug,/timed out after 1000ms/);
}
if(setup.fault==='missing')assert.match(debug+read('native-terminal.log'),/not found/);
console.log(JSON.stringify({mode:setup.mode,passed:true,requests:rows.length,main_requests:main.length,
  session_id:start[0].session_id,windows_qualified:false,mandatory_policy_qualified:false}));
