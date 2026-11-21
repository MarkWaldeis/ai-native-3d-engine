// Byte transport and presentation metadata only; clocks/prediction live in NetGame.
export function netURL(pageURL, authored = null) {
  const page = new URL(pageURL), query = page.searchParams;
  if (!query.has('net') && !authored) return null;
  const configured = query.has('net') ? query.get('net') : (authored.url ?? authored);
  if (configured === 'off' || configured === '0') return null;
  const fallback = new URL(page);
  fallback.protocol = page.protocol === 'https:' ? 'wss:' : 'ws:';
  fallback.port = '7790'; fallback.pathname = '/'; fallback.search = ''; fallback.hash = '';
  const result = new URL(configured || fallback.href, fallback);
  if (!['ws:', 'wss:'].includes(result.protocol) || result.username || result.password) throw new Error('Multiplayer benötigt eine ws:// oder wss:// URL ohne Zugangsdaten.');
  return result.href;
}

export class BrowserNet {
  constructor(game, url, onError, Socket = globalThis.WebSocket) {
    this.game = game; this.socket = new Socket(url); this.socket.binaryType = 'arraybuffer';
    this.status = {connected:false,my_entity:null,rtt:0,corrections:0,entity_count:0,players:0,positions:{},spawned:0,removed:0};
    this.socket.onmessage = event => {
      try { game.receive(typeof event.data === 'string' ? event.data : new TextDecoder().decode(event.data)); }
      catch (error) { this.close(); onError(error); }
    };
    this.socket.onclose = () => { game.disconnect(); this.status.connected = false; this.status.players = 0; };
    this.socket.onerror = () => { this.status.connected = false; };
  }
  tick(inputs, now) {
    if (this.socket.readyState !== 1) return null;
    if (this.socket.bufferedAmount > 4 * 1024 * 1024) {
      this.close(); throw new Error('Multiplayer-Verbindung zu langsam. Bitte neu verbinden.');
    }
    const result = JSON.parse(this.game.tick(JSON.stringify(inputs), now));
    for (const message of result.outgoing) this.socket.send(message);
    Object.assign(this.status, {
      connected:result.connected,my_entity:result.my_entity,rtt:result.stats.rtt_ms,
      corrections:result.stats.corrections,entity_count:result.transforms.length,players:result.players,
      positions:Object.fromEntries(result.transforms.map(e => [e.id,e.transform.position])),
      spawned:this.status.spawned + result.spawned.length,removed:this.status.removed + result.removed.length,
    });
    return result;
  }
  close() {
    this.socket.onclose = this.socket.onmessage = this.socket.onerror = null;
    this.game.disconnect(); this.status.connected = false; this.status.players = 0; this.socket.close();
  }
}
