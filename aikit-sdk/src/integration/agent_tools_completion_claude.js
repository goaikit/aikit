// Explicit unqualified candidate: native ordering and identity remain separate.
on('classic.Stop', async ($, event, next) => {
  const turn = nativeTurnId;
  const startup = turnStartObservation;
  const epoch = sessionEpoch;
  const unavailable = () => next.signal?.aborted || !epoch?.active || epoch !== sessionEpoch
    || turn !== nativeTurnId || startup !== turnStartObservation;
  if (event.agent_id !== undefined || !turnStartObservation || !nativeTurnId)
    return { block: 'Completion validation unavailable: native Turn context is missing.' };
  await turnStartObservation;
  if (unavailable()) return { block: 'Completion validation unavailable: native Turn changed or was cancelled.' };
  const native = await publishContext($, epoch);
  if (unavailable()) return { block: 'Completion validation unavailable: native Turn changed or was cancelled.' };
  if (event.session_id !== native.session_id || event.cwd !== native.cwd)
    return { block: 'Completion validation unavailable: native session context changed.' };
  const frame = JSON.stringify(event);
  if (new TextEncoder().encode(frame).length > 131072)
    return { block: 'Completion validation unavailable: native input exceeds its limit.' };
  const response = await $.process.run(config.sessionArgv, { stdin: frame, timeoutMs: config.timeoutMs });
  if (unavailable()) return { block: 'Completion validation unavailable: native Turn changed or was cancelled.' };
  if (response.exitCode !== 0 || response.isStdoutTruncated !== false
      || response.isStderrTruncated !== false || typeof response.stdout !== 'string'
      || new TextEncoder().encode(response.stdout).length > 65536)
    return { block: 'Completion validation unavailable: local handler failed.' };
  const decision = JSON.parse(response.stdout);
  if (!decision || typeof decision !== 'object' || Array.isArray(decision))
    return { block: 'Completion validation unavailable: invalid handler response.' };
  const keys = Object.keys(decision);
  if (keys.length === 0) return next(event);
  if (keys.length === 2 && decision.decision === 'block'
      && typeof decision.reason === 'string' && decision.reason.trim()
      && new TextEncoder().encode(decision.reason).length <= 32768)
    return { block: decision.reason };
  return { block: 'Completion validation unavailable: unsupported handler response.' };
}).catch(() => ({ block: 'Completion validation unavailable: restore the local handler and retry.' }));
