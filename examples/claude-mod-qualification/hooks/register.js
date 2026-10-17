// Observe only. Validation is not native lifecycle or delivery qualification.
export function register(on) {
  on('turn.start', async ($, event, next) => next(event));
  on('turn.complete', async ($, event, next) => next(event));
  on('classic.Stop', async ($, event, next) => next(event));
  on('session.receive', async ($, event, next) => next(event));
}
