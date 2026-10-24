import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {Duplex} from 'node:stream';
import WebSocket from 'ws';
const [executable,home,directory,stage='live']=process.argv.slice(2);
assert(['live','after-exit','after-restart'].includes(stage));
for(const value of [executable,home,directory])assert(path.isAbsolute(value));
assert.equal(fs.readFileSync(path.join(home,'config.toml'),'utf8').includes('Codelaya loopback fixture'),true);
const owner=JSON.parse(fs.readFileSync(path.join(directory,'owner-review.json')));
const child=spawn(executable,['app-server','proxy'],{env:{...process.env,CODEX_HOME:home,OPENAI_API_KEY:'',CODEX_API_KEY:''},stdio:['pipe','pipe','pipe']});
const stream=new Duplex({read(){},write(chunk,encoding,done){child.stdin.write(chunk,encoding,done);}});
child.stdout.on('data',chunk=>stream.push(chunk));
child.stdout.on('end',()=>stream.push(null));
const socket=new WebSocket('ws://localhost/',{createConnection:()=>stream});
const pending=new Map();let sequence=0;
let stderr='';child.stderr.on('data',chunk=>stderr+=chunk);
const timer=setTimeout(()=>{child.kill();throw Error('fixture proxy deadline exceeded');},15000);
socket.on('message',data=>{
  const reply=JSON.parse(data.toString());const waiter=pending.get(reply.id);
  if(waiter){pending.delete(reply.id);reply.error?waiter.reject(Error(JSON.stringify(reply.error))):waiter.resolve(reply.result);}
});
child.on('exit',code=>{for(const waiter of pending.values())waiter.reject(Error(`proxy exited ${code}: ${stderr}`));});
function rpc(method,params){return new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});}
try{
 await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
 const initialize=await rpc('initialize',{clientInfo:{name:'aikit_queue_qualification',title:'Isolated queue qualification',version:'0.1.0'},capabilities:{experimentalApi:true,requestAttestation:false}});
 socket.send(JSON.stringify({method:'initialized'}));
 const history=await rpc('thread/read',{threadId:owner.thread_id,includeTurns:true});
 assert.equal(history.thread.id,owner.thread_id);
 assert.equal(path.resolve(history.thread.cwd),path.join(directory,'workspace'));
 const queue=await rpc('thread/queue/list',{threadId:owner.thread_id,limit:100});
 fs.writeFileSync(path.join(directory,`native-inspection${stage==='live'?'':`-${stage}`}.json`),JSON.stringify({initialize,history,queue},null,2),{flag:'wx'});
 console.log(JSON.stringify({inspected:true,thread_id:history.thread.id,turns:history.thread.turns?.length,queued:queue.data.length}));
}finally{clearTimeout(timer);socket.terminate();child.stdin.end();child.kill();}
