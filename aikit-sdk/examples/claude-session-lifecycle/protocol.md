# Native lifecycle final trial

Before trial: compose the current AIKit base/completion/message templates unchanged
with fixed diagnostic stdio handlers. Fresh disposable native Claude 2.1.308 home,
network none, node user, no host mounts, public dummy key, loopback-only gateway.
Retain trial 1 (resume identity mismatch and bounded request ceiling) as failed.
Trial 2 validates intermediate code with an additional lifecycle timer. Final
code reuses the opted-in message timer and must be qualified in a fresh home.

Owner terminal actions: initial synthetic prompt; /clear; second synthetic prompt;
/resume the initial session ID; third synthetic prompt; /exit. No resume repair in
place and no model prompt retry. Gateway ceiling: twelve requests, 4 MiB each.

Pass: three distinct invocation IDs in one module lifetime; native IDs initial,
cleared, initial again; one startup per invocation; polling in each context;
one correlated main start/Stop/accepted native completion per synthetic prompt;
no module failure notices. This is generated-module lifecycle evidence with a
scripted model and diagnostic journal, not actual Codelaya journal/consumption,
provider authority, or Windows qualification.
