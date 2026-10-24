// Diagnostic only. The child emits sanitized facts, never credentials.
export function register(on) {
  on('session.start',async($,event,next)=>{
    const result=await $.process.run(['python3','/home/node/probe/inbox-auth-probe.py'],{timeoutMs:5000});
    if(result.exitCode!==0||result.isStdoutTruncated!==false||result.isStderrTruncated!==false)
      throw Error('Disposable inbox probe failed');
    const facts=JSON.parse(result.stdout);
    await $.fs.write('/home/node/evidence/inbox-auth.json',JSON.stringify({sessionId:await $.session.id(),event,facts}));
    return next(event);
  });
}
