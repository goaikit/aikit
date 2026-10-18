// Qualification-only connection to an already running native daemon.
import {spawn} from 'node:child_process';
import {Duplex} from 'node:stream';
import WebSocket from 'ws';
export async function connect(executable,home){
 const child=spawn(executable,['app-server','proxy'],{env:{...process.env,CODEX_HOME:home,OPENAI_API_KEY:'',CODEX_API_KEY:''},stdio:['pipe','pipe','pipe']});
 const stream=new Duplex({read(){},write(chunk,encoding,done){child.stdin.write(chunk,encoding,done);}});
 child.stdout.on('data',chunk=>stream.push(chunk));child.stdout.on('end',()=>stream.push(null));
 const socket=new WebSocket('ws://localhost/',{createConnection:()=>stream});
 const pending=new Map(),trace=[];let sequence=0,stderr='';
 child.stderr.on('data',chunk=>{stderr=(stderr+chunk).slice(-8192);});
 const close=()=>{socket.terminate();child.stdin.end();child.kill();};
 const rejectAll=error=>{for(const waiter of pending.values())waiter.reject(error);pending.clear();};
 child.on('exit',code=>rejectAll(Error(`proxy exited ${code}: ${stderr}`)));
 socket.on('error',error=>rejectAll(error));
 socket.on('message',data=>{
   const reply=JSON.parse(data.toString());const waiter=pending.get(reply.id);
   if(waiter){pending.delete(reply.id);waiter.resolve(reply);}
 });
 async function rpc(method,params){
   const id=++sequence;
   const reply=await new Promise((resolve,reject)=>{
     const timer=setTimeout(()=>{pending.delete(id);reject(Error(`proxy deadline: ${method}`));},10000);
     pending.set(id,{resolve:value=>{clearTimeout(timer);resolve(value);},reject:error=>{clearTimeout(timer);reject(error);}});
     socket.send(JSON.stringify({id,method,params}));
   });
   trace.push({method,params,reply,observed_at_unix_ms:Date.now()});
   return reply;
 }
 try{
   await new Promise((resolve,reject)=>{
     const timer=setTimeout(()=>reject(Error('proxy connection deadline')),10000);
     socket.once('open',()=>{clearTimeout(timer);resolve();});socket.once('error',error=>{clearTimeout(timer);reject(error);});
   });
   const initialize=await rpc('initialize',{clientInfo:{name:'aikit_queue_qualification',title:'Isolated queue qualification',version:'0.1.0'},capabilities:{experimentalApi:true,requestAttestation:false}});
   if(initialize.error)throw Error(JSON.stringify(initialize.error));
   socket.send(JSON.stringify({method:'initialized'}));
   return {rpc,trace,close};
 }catch(error){close();throw error;}
}
