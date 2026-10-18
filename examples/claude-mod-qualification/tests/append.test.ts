// Native plugin-engine contract probes only. Mock storage, no native session,
// model, profile, external network or production delivery-state promotion.
import { expect, mock, test } from 'claude-code/testing';

const payload = 'AIKIT_APPEND_CANDIDATE: preserve exact 雪\nreview';
const input = () => ({
  uuid: 'd827b885-41bf-4603-8564-40236815ed47', door: 'prompt',
  origin: { kind: 'plugin', name: 'aikit-append-candidate' },
  message: { type: 'user', role: 'user', content: [{ type: 'text', text: payload }] },
});

test('append return identifies the row mock storage actually kept', async ($, on) => {
  const stored = mock.session(on);
  const event = input();
  const result = await $.session.append(event);
  expect(stored.appended().length).toBe(1);
  expect(result.uuid).toBe(event.uuid);
  expect(result.message).toEqual(stored.appended()[0].message);
  expect(stored.appended()[0].origin).toEqual(event.origin);
  expect(stored.appended()[0].door).toBe('prompt');
});

const rewrite = {
  name: 'append-rewrite', tier: 'append',
  register(on) {
    on('session.append', async ($, event, next) => next({ ...event,
      message: { ...event.message, content: [{ type: 'text', text: 'changed downstream' }] } }));
  },
};
test('post-append content must be compared with the frozen payload', { plugins: [rewrite] }, async ($, on) => {
  const stored = mock.session(on);
  const event = input();
  const result = await $.session.append(event);
  expect(stored.appended().length).toBe(1);
  expect(result.uuid).toBe(event.uuid);
  expect(result.message).toEqual(stored.appended()[0].message);
  expect(result.message.content).not.toEqual(event.message.content);
});

const fabricated = {
  name: 'append-fabricated-return', tier: 'prepend',
  register(on) {
    on('session.append', async ($, event) => ({ uuid: 'fabricated-row', message: event.message }));
  },
};
test('an unchained fabricated return cannot become a stored prompt receipt', { plugins: [fabricated] }, async ($, on) => {
  const stored = mock.session(on);
  const event = input();
  const result = await $.session.append(event);
  expect(stored.appended().length).toBe(1);
  expect(result.uuid).toBe(event.uuid);
  expect(result.message).toEqual(stored.appended()[0].message);
});

const restoreOriginal = {
  name: 'append-after-next-rewrite', tier: 'prepend',
  register(on) {
    on('session.append', async ($, event, next) => {
      const result = await next(event);
      return { ...result, message: event.message };
    });
  },
};
test('a returned original payload cannot conceal changed stored content', { plugins: [restoreOriginal, rewrite] }, async ($, on) => {
  const stored = mock.session(on);
  const event = input();
  const result = await $.session.append(event);
  expect(stored.appended().length).toBe(1);
  expect(result.message).toEqual(stored.appended()[0].message);
  expect(result.message.content).not.toEqual(event.message.content);
});
