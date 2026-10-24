import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const directory=process.argv[2];
const port=Number(process.argv[3]??0);
const controlled=process.argv[4]==='busy';
assert(process.argv[4]===undefined||controlled);
assert(Number.isInteger(port)&&port>=0&&port<=65535);
assert(path.isAbsolute(directory));
assert(fs.statSync(directory).isDirectory());
let count=0;
let held;
const controls=[];
const server=http.createServer(async(req,res)=>{
  if(req.headers.authorization){res.writeHead(400);res.end('Fixture must not receive credentials');return;}
  if(controlled&&req.method==='GET'&&req.url==='/fixture/state'){
    res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({requests:count,held:!!held}));return;
  }
  if(controlled&&req.method==='POST'&&req.url==='/fixture/release'){
    if(!held){res.writeHead(409);res.end();return;}
    controls.push({action:'release',at_unix_ms:Date.now(),requests:count});
    fs.writeFileSync(path.join(directory,'gateway-controls.json'),JSON.stringify(controls));
    const release=held;held=undefined;release();res.writeHead(200);res.end();return;
  }
  if(req.method!=='POST'||req.url!=='/v1/responses'){res.writeHead(404);res.end();return;}
  const chunks=[];let size=0;
  for await(const chunk of req){size+=chunk.length;if(size>4*1024*1024){res.writeHead(413);res.end();return;}chunks.push(chunk);}
  if(++count>(controlled?8:4)){
    fs.appendFileSync(path.join(directory,'rejected-requests.jsonl'),JSON.stringify({index:count,reason:'request_limit'})+'\n');
    res.writeHead(429);res.end();return;
  }
  const body=JSON.parse(Buffer.concat(chunks));
  const index=count;
  fs.appendFileSync(path.join(directory,'requests.jsonl'),JSON.stringify({index,session_id:req.headers.session_id??null,body})+'\n');
  const lastUser=body.input?.filter(item=>item.role==='user').at(-1);
  const textOf=item=>(item?.content??[]).map(part=>part.text??'').join('');
  if(controlled&&textOf(lastUser)==='CODELAYA_CODEX_BASELINE: Reply with the fixture answer. Do not call tools.'){
    assert(!held,'only one baseline may be held');
    await new Promise(resolve=>{held=resolve;});
  }
  const text=`CODELAYA_CODEX_FIXTURE_ANSWER_${index}`;
  const item={id:`msg_fixture_${index}`,type:'message',status:'completed',role:'assistant',content:[{type:'output_text',text,annotations:[]}]};
  const response={id:`resp_fixture_${index}`,object:'response',created_at:Math.floor(Date.now()/1000),status:'completed',model:body.model,output:[item],usage:{input_tokens:10,output_tokens:5,total_tokens:15}};
  res.writeHead(200,{'Content-Type':'text/event-stream','Cache-Control':'no-cache'});
  for(const event of [
    {type:'response.created',response:{...response,status:'in_progress',output:[]}},
    {type:'response.output_item.added',output_index:0,item:{...item,status:'in_progress',content:[]}},
    {type:'response.output_text.delta',item_id:item.id,output_index:0,content_index:0,delta:text},
    {type:'response.output_item.done',output_index:0,item},
    {type:'response.completed',response},
  ])res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
  res.end();
});
server.listen(port,'127.0.0.1',()=>{
  const address={pid:process.pid,url:`http://127.0.0.1:${server.address().port}/v1`};
  fs.writeFileSync(path.join(directory,'gateway.json'),JSON.stringify(address),{flag:'wx'});
  console.log(JSON.stringify(address));
});
