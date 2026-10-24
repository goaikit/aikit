// Local synthetic responses only. No real credentials or external proxy.
import fs from 'node:fs';
import http from 'node:http';
let sequence=0;
const root='/home/node/evidence/';
const setup=JSON.parse(fs.readFileSync(root+'setup.json'));
let main=0;
http.createServer(async(req,res)=>{
  if(req.method==='GET'&&req.url==='/__aikit_probe_ready'){
    res.writeHead(200,{'content-type':'application/json'});res.end('{"ready":true}');return;
  }
  const id=++sequence;if(id>8){res.writeHead(429);res.end();return;}
  const chunks=[];let size=0;
  for await(const chunk of req){size+=chunk.length;if(size>4194304){res.writeHead(413);res.end();return;}chunks.push(chunk);}
  const body=chunks.length?JSON.parse(Buffer.concat(chunks)):{};
  fs.appendFileSync(root+'requests.jsonl',JSON.stringify({sequence:id,path:req.url,body})+'\n');
  const json=value=>{res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(value));};
  if(req.url.startsWith('/v1/messages/count_tokens'))return json({input_tokens:100});
  if(!req.url.startsWith('/v1/messages'))return json({});
  const isMain=Array.isArray(body.tools)&&body.tools.length>0;
  if(isMain)main++;
  const texts=body.messages?.flatMap(message=>typeof message.content==='string'?[message.content]
    :Array.isArray(message.content)?message.content.filter(block=>block.type==='text').map(block=>block.text):[])??[];
  if(Array.isArray(body.tools)&&body.tools.length>0
      &&texts.some(value=>value.includes('AIKIT_MANAGED_STOP_BLOCK: validation unavailable'))) {
    fs.writeFileSync(root+'blocked-request.json',JSON.stringify({sequence:id,body}),{flag:'wx'});
    const timer=setTimeout(()=>{if(!res.destroyed){res.writeHead(500);res.end('owner did not cancel held probe');}},120000);
    res.once('close',()=>{clearTimeout(timer);fs.writeFileSync(root+'held-request-closed.json',JSON.stringify({sequence:id}));});
    return;
  }
  const text='Synthetic answer before managed validation';
  const read=isMain&&main===1&&setup.event==='PreToolUse';
  const content=read?{type:'tool_use',id:'probe_read',name:'Read',input:{file_path:'/home/node/workspace/.aikit-native-policy-probe'}}:{type:'text',text};
  const stop_reason=read?'tool_use':'end_turn';
  const message={id:`msg_policy_${id}`,type:'message',role:'assistant',model:body.model,content:[content],
    stop_reason,stop_sequence:null,usage:{input_tokens:100,output_tokens:3}};
  if(!body.stream)return json(message);
  res.writeHead(200,{'content-type':'text/event-stream'});
  const event=(type,data)=>res.write(`event: ${type}\ndata: ${JSON.stringify({type,...data})}\n\n`);
  event('message_start',{message:{...message,content:[],stop_reason:null}});
  event('content_block_start',{index:0,content_block:read?{...content,input:{}}:{type:'text',text:''}});
  event('content_block_delta',{index:0,delta:read?{type:'input_json_delta',partial_json:JSON.stringify(content.input)}:{type:'text_delta',text}});
  event('content_block_stop',{index:0});event('message_delta',{delta:{stop_reason,stop_sequence:null},usage:{output_tokens:3}});
  event('message_stop',{});res.end();
}).listen(8787,'127.0.0.1',()=>console.log('managed Stop probe loopback ready'));
