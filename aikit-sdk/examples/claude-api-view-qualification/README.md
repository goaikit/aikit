# Native API-view rewrite diagnostic

This command-only probe accompanies [the SDK fix](../../claude-api-view-provenance.md).
It is deliberately not a production plugin or a generated application workflow.

In a fresh disposable Linux node home, use the checksum-pinned native 2.1.308
image from the report, `--network none`, no host mounts, and the public dummy key
`sk-ant-aikit-local-fixture-not-a-credential`. No external model call is needed.

1. Make `/home/node/workspace/.codelaya-qualification-only` and a node-owned
   plugin `/home/node/probe`. Set its manifest name to `aikit-types-probe`, version
   `0.0.1`, and `hooks/hooks.json` to `{"modules":["./register.js"]}`. Copy
   `observer.js` to `hooks/register.js`.
2. Start ordinary interactive `claude --plugin-dir /home/node/probe` in that
   disposable workspace. Accept normal workspace trust, then run
   `/aikit-view-probe`. Preserve `/home/node/view-return.json` and
   `/home/node/view-witness.json` as control artifacts. Exit normally.
3. Create another plugin `/home/node/mutator`, manifest name `api-view-mutator`,
   with the same hooks declaration and `mutator.js` as its register file. Add it
   to a task-local directory marketplace `api-view-probe` rooted at `/home/node`,
   source `./mutator`. Enable it in that disposable profile with
   `appendPlugins:["api-view-mutator@api-view-probe"]`.
4. Restart the same native command with debug logging and run the diagnostic
   command once. Preserve its return, witness, mutator record and native loading
   trace. Require actual user/append/core placement, not just declared settings.
5. Run `node verify.mjs CONTROL_RETURN CONTROL_WITNESS RETURN WITNESS MUTATOR`.
   Preserve exact Claude version/executable hash, generated declarations hash,
   image identity, settings, source files and any failed attempts. Exit normally.

The retained native trial has empty core arrays. The mutator adds a fabricated
Unicode user message only to the API result. The verifier requires that exact
native counterexample, without treating a model request or fabricated event as
having occurred. It does not prove the SDK fix's complete application path,
native receipts, effective-policy enforcement or Windows acceptance.
