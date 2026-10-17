// Generated provider bridge. Native context is not application authorization.
const config = /* AIKIT_CONFIG */null;
const invocationId = crypto.randomUUID();
const unavailable = 'Agent operation unavailable. Retain an uncertain operation ID; do not infer success or blindly resend.';
async function invoke($, event) {
  try {
    if (event.agentId !== undefined) return { deny: 'Subagent authoring is not qualified.' };
    const request = event.request;
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || new TextEncoder().encode(JSON.stringify(request)).length > 65536)
      return { deny: 'Agent request must be a JSON object of at most 64 KiB.' };
    const call = { schema_version: 1, application_id: config.applicationId, installation_id: config.installationId,
      agent_key: 'claude', tool_name: config.toolName,
      native_context: { session_id: await $.session.id(), invocation_id: invocationId, cwd: await $.session.cwd() }, request };
    const stdin = JSON.stringify(call);
    if (new TextEncoder().encode(stdin).length > 131072) return { deny: 'Agent invocation exceeds its bounded envelope.' };
    const result = await $.process.run(config.argv, { stdin, timeoutMs: config.timeoutMs });
    if (result.isStdoutTruncated !== false || result.isStderrTruncated !== false
        || new TextEncoder().encode(result.stdout).length > 1048576) return { deny: unavailable };
    JSON.parse(result.stdout);
    return result.exitCode === 0 ? { result: result.stdout } : { result: result.stdout, isError: true };
  } catch { return { deny: unavailable }; }
}
export function register(on) {
  on('session.start', async ($, event, next) => {
    const native = { session_id: await $.session.id(), invocation_id: invocationId, cwd: await $.session.cwd() };
    await $.env.set('AIKIT_INVOCATION_ENV_KEY', JSON.stringify({ schema_version: 1,
      installation_id: config.installationId, native_context: native }));
    if (config.sessionArgv) {
      const started = await $.process.run(config.sessionArgv, { stdin: JSON.stringify({
        hook_event_name: 'SessionStart', session_id: native.session_id, cwd: native.cwd,
      }), timeoutMs: config.timeoutMs });
      if (started.exitCode !== 0 || started.isStdoutTruncated !== false || started.isStderrTruncated !== false)
        throw new Error('Recorded session startup unavailable');
    }
    await $.tool.register({ name: config.toolName, description: config.description, isDeferred: false,
      inputSchema: { type: 'object', properties: { request: config.schema }, required: ['request'], additionalProperties: false } });
    $.ui.invalidate('prompt.context');
    return next(event);
  });
  on('prompt.context', async ($, event, next) => {
    const context = await next(event);
    if (!config.instructions) return context;
    if (context.blocks.some(block => block.name === config.contextName)) throw new Error('Conflicting application instruction block');
    return { ...context, blocks: [...context.blocks, { name: config.contextName, text: config.instructions }] };
  });
  on('tool.call', { tool: 'AIKIT_NATIVE_TOOL_NAME' }, async ($, event) => invoke($, event))
    .catch(() => ({ deny: unavailable }));
}
