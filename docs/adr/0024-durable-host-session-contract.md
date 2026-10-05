# Durable sessions belong to their workspace host

The agent server exposes the SDK session contract through HTTP commands and replayable SSE. Each public session belongs to one host and one workspace; a viewer disconnect does not stop execution. SQLite stores receipts and canonical event envelopes, and a process lock prevents concurrent ownership. After a host restart, sessions become interrupted and pending command outcomes become uncertain; actions are never automatically repeated.

Session backends include native ACP and HTTP agents that the existing one-shot CLI Backend enum cannot run. Session discovery therefore uses its own typed catalog and adapter capabilities, with conservative defaults and negotiated ACP resume support. AgentEventPayload remains unchanged inside the envelope, honoring ADR 0005/0016. Native protocol extensions are preserved separately.

The deployment boundary is one trusted user/team per host. Scoped client tokens authorize reads, commands and request responses; containers or VMs isolate mutually untrusted teams. Fleet migration and a central control plane are deferred. Linux is the production deployment target and Windows remains supported for development. Capability parity is not assumed across the nine session backends.
