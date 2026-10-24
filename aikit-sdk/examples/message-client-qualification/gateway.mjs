// Scripted loopback endpoint. Captures actual native requests, never proxies them.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
const evidence=process.argv[2];
if(!evidence || !path.isAbsolute(evidence)) throw Error('absolute evidence directory required');
fs.mkdirSync(evidence,{recursive:true});
const key='sk-ant-aikit-local-fixture-not-a-credential',epoch=randomUUID();
let requests=0;
const send=(response,status,body)=>{response.writeHead(status,{'content-type':'application/json'});response.end(JSON.stringify(body));};
http.createServer(async(request,response)=>{
  if(++requests>24) return send(response,400,{error:{type:'invalid_request_error',message:'fixture request ceiling'}});
  let size=0;const chunks=[];
  for await(const chunk of request){size+=chunk.length;if(size>4*1024*1024)return send(response,413,{error:'body ceiling'});chunks.push(chunk);}
  let body;try{body=size?JSON.parse(Buffer.concat(chunks).toString('utf8')):null;}catch{return send(response,400,{error:'invalid JSON'});}
  if(request.method==='POST' && request.headers['x-api-key']!==key) return send(response,401,{error:'fixture key required'});
  fs.appendFileSync(`${evidence}/requests.jsonl`,JSON.stringify({epoch,sequence:requests,method:request.method,path:request.url,body})+'\n');
  if(request.url.startsWith('/v1/messages/count_tokens'))return send(response,200,{input_tokens:100});
  if(request.method!=='POST' || !request.url.startsWith('/v1/messages'))return send(response,200,{fixture:true});
  const feedback=JSON.stringify(body.messages).includes('AIKIT_DURABLE_REVIEW');
  const text=feedback?'Durable feedback received by scripted fixture':'Initial fixture answer';
  const message={id:`msg_fixture_${requests}`,type:'message',role:'assistant',model:body.model,
    content:[{type:'text',text}],stop_reason:'end_turn',stop_sequence:null,usage:{input_tokens:100,output_tokens:5}};
  if(!body.stream)return send(response,200,message);
  response.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
  const event=(type,data)=>response.write(`event: ${type}\ndata: ${JSON.stringify({type,...data})}\n\n`);
  event('message_start',{message:{...message,content:[],stop_reason:null,usage:{input_tokens:100,output_tokens:0}}});
  event('content_block_start',{index:0,content_block:{type:'text',text:''}});
  event('content_block_delta',{index:0,delta:{type:'text_delta',text}});
  event('content_block_stop',{index:0});
  event('message_delta',{delta:{stop_reason:'end_turn',stop_sequence:null},usage:{output_tokens:5}});
  event('message_stop',{});response.end();
}).listen(8787,'127.0.0.1',()=>console.log('fixture ready on 127.0.0.1:8787'));
