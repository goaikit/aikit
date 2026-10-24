import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
export function verify(root) {
  const read=(dir,name)=>fs.readFileSync(path.join(root,dir,name),'utf8').replace(/^\uFEFF/,'');
  const json=(dir,name)=>JSON.parse(read(dir,name));
  for(const dir of ['inbox-auth-20261024','row-read-20261024']) {
    const c=json(dir,'container.json')[0];assert.equal(c.HostConfig.NetworkMode,'none');assert.deepEqual(c.Mounts,[]);
    assert.equal(c.Image,'sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924');
    assert.equal(read(dir,'native-version.txt').trim(),'2.1.308 (Claude Code)');
    assert.equal(json(dir,'model-dispatch-summary.json').main_requests,0);
    assert(read(dir,'native-loaded.log').includes('events: session.start'));
  }
  const auth=json('inbox-auth-20261024','inbox-auth.json');assert.equal(auth.event.isInteractive,true);assert.equal(auth.event.surface,'terminal');
  const f=auth.facts;assert.equal(f.probe,'observed');assert.equal(f.socket_present,true);assert.equal(f.token_present,true);assert.equal(f.socket_uid,1000);
  assert.deepEqual(f.connections.map(c=>c.mode),['valid','invalid','none']);
  const peer=f.connections[0];assert(Number.isInteger(peer.peer_pid)&&peer.peer_pid>0);assert.match(peer.peer_start_ticks,/^\d+$/);
  for(const c of f.connections){assert.equal(c.peer_pid,peer.peer_pid);assert.equal(c.peer_start_ticks,peer.peer_start_ticks);
    assert.equal(c.peer_uid,1000);assert.equal(c.peer_gid,1000);
    assert.equal(c.peer_executable_sha256,'356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58');
    assert.equal(c.observation,'open-until-observer-timeout');assert.equal(c.response_type,undefined);}
  const live=json('inbox-auth-20261024','process-match.json');assert.equal(live.peer_pid,peer.peer_pid);
  assert.equal(live.peer_start_ticks,peer.peer_start_ticks);assert.equal(live.executable_sha256,peer.peer_executable_sha256);
  const ended=json('inbox-auth-20261024','peer-after-exit.json');assert.equal(ended.peer_pid,peer.peer_pid);assert.equal(ended.state,'gone');
  const note=json('row-read-20261024','row-read.json');assert.notEqual(note.sessionId,auth.sessionId);
  assert.equal(note.text,'AIKIT_ROW_LOOKUP_PROBE: literal Unicode 雪');assert.equal(note.stored.message.isMeta,true);
  assert.deepEqual(note.rows,[]);assert.deepEqual(note.api,[{role:'user',content:[{type:'text',text:note.text}]}]);
  const durable=json('row-read-20261024','stored-row.json');assert.equal(durable.transcript_exists,true);assert.equal(durable.rows.length,1);
  assert.equal(durable.rows[0].uuid,note.stored.uuid);assert.equal(durable.rows[0].sessionId,note.sessionId);
  assert.deepEqual(durable.rows[0].message.content,[{type:'text',text:note.text}]);
  const history='history-reuse-20261024',expected=json(history,'expected-review.json');
  assert.equal(json(history,'history-review.json').messages.Ok.length,0);
  assert.equal(json(history,'history-note-published.json').messages.Ok.length,0);
  const fixed=json(history,'history-review-published.json');assert.equal(fixed.messages.Ok.length,32);
  assert.equal(fixed.info.Ok.session_id,expected.native_session_id);assert.equal(fixed.info.Ok.cwd,'/home/node/workspace');
  assert.equal(expected.receipt_state,'unknown');
  const matches=fixed.messages.Ok.filter(m=>m.uuid===expected.native_row_id);assert.equal(matches.length,1);
  const row=matches[0];assert.equal(row.session_id,expected.native_session_id);assert.equal(row.role,'user');assert.equal(row.parent_tool_use_id,null);
  assert(row.content.text.includes(expected.payload));
  assert.equal(createHash('sha256').update(row.content.text).digest('hex'),expected.framed_payload_sha256);
  assert.equal(read(history,'history-before.sha'),read(history,'history-published-after.sha'));
  assert.equal(json(history,'published-manifest.json').claude_sdk_revision,'57ad0ec71c9e1de19cc750ba6ab7591adfd13e9f');
  return {native_sessions:[auth.sessionId,note.sessionId],model_requests:0,
    linux_auth_acknowledgement:false,kernel_peer_identity_observed:true,
    history_messages_before:0,history_messages_after:32,stored_review_uuid:row.uuid,receipt_promoted:false};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify(verify(process.argv[2]),null,2));
