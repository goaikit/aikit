import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const base='/home/node/evidence/';
const lines=file=>fs.readFileSync(file,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
const rows=lines(base+'handler.jsonl'), starts=rows.filter(row=>row.frame.hook_event_name==='SessionStart');
assert.equal(starts.length,3);
const contexts=starts.map(row=>row.context.native_context);
assert.equal(new Set(contexts.map(c=>c.invocation_id)).size,3);
assert.equal(contexts[0].session_id,contexts[2].session_id);
assert.notEqual(contexts[0].session_id,contexts[1].session_id);
for(const context of contexts) {
  const scoped=rows.filter(row=>row.context?.native_context.invocation_id===context.invocation_id);
  assert(scoped.some(row=>row.kind==='messages' && row.frame.action.kind==='poll'));
  const begun=scoped.filter(row=>row.frame.hook_event_name==='AIKitNativeTurnStarted');
  const stopped=scoped.filter(row=>row.frame.hook_event_name==='Stop');
  const finished=scoped.filter(row=>row.frame.hook_event_name==='AIKitNativeTurnFinished');
  assert.equal(begun.length,1);assert.equal(stopped.length,1);assert.equal(finished.length,1);
  assert.equal(begun[0].frame.native_turn.id,finished[0].frame.native_turn.id);
  assert.equal(stopped[0].context.native_turn_id,begun[0].frame.native_turn.id);
  assert.deepEqual(finished[0].frame.native_turn.outcome,{reason:'answer',is_aborted:false});
  for(const row of scoped)assert.equal(row.frame.session_id??row.frame.native_context.session_id,context.session_id);
}
const requests=lines(base+'requests.jsonl');assert(requests.length<=12);
const main=requests.filter(row=>row.body.tools?.some(tool=>tool.name==='mcp__aikit-lifecycle-module__review'));
assert.equal(main.length,3);
assert(JSON.stringify(main[0]).includes('LIFECYCLE_INITIAL'));
assert(JSON.stringify(main[1]).includes('LIFECYCLE_AFTER_CLEAR'));
assert(JSON.stringify(main[2]).includes('LIFECYCLE_AFTER_RESUME'));
assert(!JSON.stringify(main[2]).includes('LIFECYCLE_AFTER_CLEAR'));
const lifecycle=JSON.parse(fs.readFileSync(base+'lifecycle.json'));
assert.equal(lifecycle.filter(row=>row.name==='session.start' && row.phase==='before').length,1);
for(const source of ['clear','resume'])assert.equal(lifecycle.filter(row=>row.name==='classic.SessionStart' && row.source===source && row.phase==='before').length,1);
const history='/home/node/.claude/projects/-home-node-workspace';
for(const file of fs.readdirSync(history).filter(file=>file.endsWith('.jsonl')))
  for(const row of lines(history+'/'+file))
    assert(!/aikit-lifecycle-module:.*(?:skipped|did not load|failed)/s.test(typeof row.content==='string'?row.content:''));
const sha=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const verdict={passed:true,scope:'generated-module lifecycle only; diagnostic journal and scripted model',
  native_sha256:sha('/usr/local/bin/claude'),module_sha256:sha('/home/node/provider/hooks/register.js'),
  handler_sha256:sha('/home/node/handler.mjs'),requests_sha256:sha(base+'requests.jsonl'),contexts,
  requests:requests.length,main_requests:main.length,native_completed_turns:3,
  windows_qualified:false,production_consumption_qualified:false};
fs.writeFileSync(base+'verdict.json',JSON.stringify(verdict,null,2),{flag:'wx'});
console.log(JSON.stringify(verdict));
