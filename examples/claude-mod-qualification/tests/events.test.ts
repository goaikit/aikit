// Runs in Claude's offline plugin engine. No model, native turn loop or transport.
import { expect, test } from 'claude-code/testing';

test('12 isolated Stop events preserve a downstream denial', async ($, on) => {
  on('classic.Stop', () => ({ decision: { behavior: 'deny', message: 'fixture validation still fails' } }));
  for (let index = 0; index < 12; index++) {
    const result = await $.classic.Stop({ stop_hook_active: true, last_assistant_message: 'fixture answer' });
    expect(result.decision.behavior).toBe('deny');
  }
});

test('aborted completion retains correlation and exact final text', async ($, on) => {
  const seen: unknown[] = [];
  on('turn.complete', ($, event) => { seen.push(event); return { text: '' }; });
  await $.turn.complete({ turnId: 'fixture-turn', answer: 'Exact answer 雪\n', durationMs: 7, isAborted: true, usage: null });
  expect(seen).toEqual([{ turnId: 'fixture-turn', answer: 'Exact answer 雪\n', durationMs: 7, isAborted: true, usage: null }]);
});

test('receive can be suppressed downstream and is not consumption evidence', async ($, on) => {
  on('session.receive', () => ({ consumed: 'fixture filter removed the message' }));
  const result = await $.session.receive({ origin: { kind: 'peer-send-message' }, text: 'Review operation fixture' });
  expect(result.consumed).toBe('fixture filter removed the message');
});

const wrapper = {
  name: 'fixture-wrapper', tier: 'prepend',
  register(on) {
    on('classic.Stop', async ($, event, next) => {
      const result = await next(event);
      if (result.decision?.behavior !== 'deny') throw new Error('fixture expected a valid downstream denial');
      return {};
    });
  },
};
const blocker = {
  name: 'fixture-blocker', tier: 'append',
  register(on) {
    on('classic.Stop', async () => ({ decision: { behavior: 'deny', message: 'fixture mandatory policy' } }));
  },
};
// The initial hypothesis that denial survives this wrapper was falsified on
// 2.1.296. Preserve the counterexample; this is a readiness gap, not an Allow.
test('a wrapper can erase a downstream denial: native precedence remains unqualified', { plugins: [wrapper, blocker] }, async ($, on) => {
  on('classic.Stop', () => ({}));
  const result = await $.classic.Stop({ stop_hook_active: false, last_assistant_message: 'fixture answer' });
  expect(result.decision).toBeUndefined();
});
