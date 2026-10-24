// Bounded synthetic loopback endpoint; never proxies or uses a real credential.
import fs from 'node:fs';
import http from 'node:http';
const directory='/home/node/evidence';let sequence=0;
http.createServer(async(req,res)=>{
  const n=++sequence,json=(status,value)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(value));};
  if(n>12)return json(429,{error:'predeclared request ceiling'});
  let size=0;const chunks=[];
  for await(const chunk of req){size+=chunk.length;if(size>4*1024*1024)return json(413,{error:'body ceiling'});chunks.push(chunk);}
  let body;try{body=size?JSON.parse(Buffer.concat(chunks)):null;}catch{return json(400,{error:'JSON required'});}
  if(req.method==='POST'&&req.headers['x-api-key']!=='sk-ant-aikit-local-fixture-not-a-credential')return json(401,{error:'public fixture key required'});
  fs.appendFileSync(directory+'/requests.jsonl',JSON.stringify({sequence:n,path:req.url,body})+'\n');
  if(req.url.startsWith('/v1/messages/count_tokens'))return json(200,{input_tokens:100});
  if(req.method!=='POST'||!req.url.startsWith('/v1/messages'))return json(404,{error:'unsupported route'});

  const answer='Native input comparison response.';
  const message={id:`msg_dispatch_${n}`,type:'message',role:'assistant',model:body.model,
    content:[{type:'text',text:answer}],stop_reason:'end_turn',stop_sequence:null,usage:{input_tokens:100,output_tokens:3}};
  if(!body.stream)return json(200,message);
  res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
  const event=(type,data)=>res.write(`event: ${type}\ndata: ${JSON.stringify({type,...data})}\n\n`);
  event('message_start',{message:{...message,content:[],stop_reason:null,usage:{input_tokens:100,output_tokens:0}}});
  event('content_block_start',{index:0,content_block:{type:'text',text:''}});
  event('content_block_delta',{index:0,delta:{type:'text_delta',text:answer}});
  event('content_block_stop',{index:0});
  event('message_delta',{delta:{stop_reason:'end_turn',stop_sequence:null},usage:{output_tokens:3}});
  event('message_stop',{});res.end();
}).listen(8787,'127.0.0.1',()=>console.log('bounded dispatch fixture ready'));
