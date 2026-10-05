// Node 20+ or a browser with fetch streaming. Disconnecting this iterator leaves work running.
export class GatewayError extends Error {
  constructor(status, body) {
    super(`${status}: ${body}`);
    this.name = 'GatewayError'; this.status = status;
    let error; try { error = JSON.parse(body).error; } catch { /* framework response */ }
    this.code = error?.code ?? 'http_error';
    this.retry = error?.retry ?? 'inspect_receipt';
  }
}
export class AgentHost {
  constructor(base, token) { this.base = base.replace(/\/$/, '') + '/api/v1/gateway'; this.token = token; }
  async request(path, body, { signal = AbortSignal.timeout(30_000) } = {}) {
    const response = await fetch(this.base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { Authorization: `Bearer ${this.token}`, 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    });
    if (!response.ok) throw new GatewayError(response.status, await response.text());
    return response.json();
  }
  create(options) { return this.request('/sessions', options); }
  command(session, command) { return this.request(`/sessions/${encodeURIComponent(session)}/commands`, command); }
  async *events(session, after = 0, signal) {
    const response = await fetch(`${this.base}/sessions/${encodeURIComponent(session)}/events?after=${after}`, {
      headers: { Authorization: `Bearer ${this.token}` }, signal,
    });
    if (!response.ok) throw new GatewayError(response.status, await response.text());
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '', data = [], type = '';
    try {
      while (true) {
        const { value, done } = await reader.read(); if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let end;
        while ((end = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, end).replace(/\r$/, ''); buffer = buffer.slice(end + 1);
          if (line.startsWith('event:')) type = line.slice(6).trim();
          else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
          else if (line === '') {
            if (type === 'replay_error') throw new Error(data.join('\n'));
            if (type === 'session_event' && data.length) {
              const event = JSON.parse(data.join('\n'));
              if (event.sequence > after) { after = event.sequence; yield event; }
            }
            data = []; type = '';
          }
        }
      }
    } finally { await reader.cancel(); reader.releaseLock(); }
  }
}
