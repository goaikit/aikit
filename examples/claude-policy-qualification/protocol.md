# Native module admission gate probe

Predeclared 2026-10-25. Inspect the existing `plugin.register` API before adding
new integration surfaces or declaring a provider blocker. Ordinary interactive
Linux Claude 2.1.308 only; Windows checks remain deferred.

Use the independently pinned native image, a fresh node-owned disposable home,
network none, no host mounts and the public dummy key. No model prompt, network
gateway, session message, provider token or managed agent is needed. The owner
starts ordinary Claude with actual loader debug and terminal recording.

Configure a local marketplace in the fresh profile. Put the guard in the
profile's `prependPlugins` list. Its `plugin.register` gate must decide from the
native `tier` and scanned `uses.events`, not a caller-chosen module name. A
benign module and a module declaring `classic.Stop` both record actual startup
only if admitted. The latter represents a competing completion handler.

Fresh cases, with immutable sources and settings for each:

1. `healthy`: the gate refuses the competing module and allows the benign one.
2. `catch`: the gate throws for the competing module; a constant catch refuses it.
3. `catch-fails`: both the gate and its catch throw; observe whether native core
   admits the competing module. Do not infer behavior from documentation alone.

For each, trust the disposable workspace normally, await startup, then exit.
Require native debug to prove actual guard tier, an actual engine-origin
registration observation with the scanned competing event, and the presence or
absence of each module's own startup marker. Retain all failures and sources.
Changing a live module to produce the expected marker is forbidden.

This is native module admission evidence, not a Codelaya completion trial or
proof of mandatory policy, lifecycle attestation or Windows support. The
managed-settings/built-in guard path must be investigated separately before
concluding that native mandatory enforcement is unavailable.
