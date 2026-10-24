# Initial owned integration bundle

`IntegrationService::installation_identity(&spec)` returns the canonical opaque
installation ID without creating a plan or modifying provider configuration.
Use it in `AgentToolSpec.installation_id`, then call
`plan_initial_with_agent_tool_id(operation_id, spec, policy, tool, message_handler)`.

The caller must durably retain its operation ID and exact recipe before planning.
The returned preview includes owned configuration, completion setting and module
file changes. It applies nothing. Use the existing `apply_install`, recovery and
`applied_installation_guard` APIs. Keep the guard until the consumer commits its
own matching installation metadata. An exact retry returns the original preview
after a lost planning reply, partial apply or completed apply; a preview is not
proof that the installation is still current. Revalidate through the guard.

A new initial operation refuses an existing installation. Existing update APIs
still require one. Initial and update requests have distinct operation digests;
old update digests remain unchanged so persisted updates can resume. Unsupported
callable adapters, mismatched identities and attached bindings refuse planning.
Attachment, owned files and configuration are rechecked during apply/recovery.
Only SDK-owned settings/files are removed; unrelated entries remain intact.

The optional message handler uses the existing experimental bridge and requires
input observations. This API does not qualify that bridge, module loading,
effective completion policy, native caller identity or any provider capability.
Configured is configuration evidence only. Consumers must still enforce their
mandatory readiness requirements before production activation. No agent is
started, stopped or granted application authority.

## Validation scope

Linux Docker, Rust 1.88.0, integration feature with default features disabled.
`install_tests.rs` covers combined initial creation, no provider writes before
apply, lost replies, interrupted module writes and recovery, exact retry after
apply, changed request/scope/intent conflicts, initial-versus-update preconditions,
unsupported providers, foreign identities, attached binding refusal at plan/apply
and recovery, configuration drift, optional feedback and completion settings, and
owned removal preserving unrelated hooks/settings. Existing update and schema
upgrade tests remain in the integration suite.

This is library installation validation. It is not native Claude execution,
production Codelaya activation or Windows acceptance.
