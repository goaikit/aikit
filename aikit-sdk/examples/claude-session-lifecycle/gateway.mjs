import http from 'node:http';
import fs from 'node:fs';
let sequence=0;
http.createServer(async(req,res)=>{
  const id=++sequence,chunks=[];let size=0;
  for await(const c of req){size+=c.length;if(size>4194304){res.writeHead(413);res.end();return;}chunks.push(c);}
  if(id>12){res.writeHead(429);res.end();return;}
  const body=chunks.length?JSON.parse(Buffer.concat(chunks)):{};
  fs.appendFileSync('/home/node/evidence/requests.jsonl',JSON.stringify({id,path:req.url,body})+'\n');
  const json=value=>{res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(value));};
  if(req.url.startsWith('/v1/messages/count_tokens'))return json({input_tokens:100});
  if(!req.url.startsWith('/v1/messages'))return json({});
  const text='Offline lifecycle fixture answer';
  const message={id:`msg_lifecycle_${id}`,type:'message',role:'assistant',model:body.model,
    content:[{type:'text',text}],stop_reason:'end_turn',stop_sequence:null,usage:{input_tokens:100,output_tokens:3}};
  if(!body.stream)return json(message);
  res.writeHead(200,{'content-type':'text/event-stream'});
  const event=(type,data)=>res.write(`event: ${type}\ndata: ${JSON.stringify({type,...data})}\n\n`);
  event('message_start',{message:{...message,content:[],stop_reason:null}});
  event('content_block_start',{index:0,content_block:{type:'text',text:''}});
  event('content_block_delta',{index:0,delta:{type:'text_delta',text}});
  event('content_block_stop',{index:0});event('message_delta',{delta:{stop_reason:'end_turn',stop_sequence:null},usage:{output_tokens:3}});
  event('message_stop',{});res.end();
}).listen(8787,'127.0.0.1',()=>console.log('offline lifecycle gateway ready'));
