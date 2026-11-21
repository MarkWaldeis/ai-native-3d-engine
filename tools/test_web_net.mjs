import test from 'node:test';
import assert from 'node:assert/strict';
import {netURL, BrowserNet} from '../crates/engine-web/player/net.js';

test('single-player is unchanged unless net query or scene opts in', () => {
  assert.equal(netURL('http://localhost:8000/arena/'), null);
  assert.equal(netURL('http://localhost:8000/arena/?net'), 'ws://localhost:7790/');
  assert.equal(netURL('https://game.example/arena/?net='), 'wss://game.example:7790/');
  assert.equal(netURL('http://localhost/?net=ws://localhost:7791'), 'ws://localhost:7791/');
  assert.equal(netURL('http://localhost/', {url:'ws://server:7790'}), 'ws://server:7790/');
  assert.equal(netURL('http://localhost/?net=off', 'ws://server:7790'), null);
  assert.throws(() => netURL('http://localhost/?net=https://example.com'), /ws:\/\//);
  assert.throws(() => netURL('http://localhost/?net=ws://user:secret@example.com'), /Zugangsdaten/);
});

class Socket {
  constructor(url) {this.url=url;this.readyState=0;this.bufferedAmount=0;this.sent=[];}
  send(bytes) {this.sent.push(bytes);}
  close() {this.readyState=3;this.onclose?.();}
}
function fixture() {
  const received=[], calls=[], errors=[];
  const result={outgoing:['protocol bytes'],connected:true,my_entity:3,players:2,stats:{rtt_ms:12,corrections:4},
    transforms:[{id:3,transform:{position:[1,2,3]}}],spawned:[{id:3}],removed:[]};
  const game={receive:bytes=>received.push(bytes),tick:(inputs,now)=>{calls.push([JSON.parse(inputs),now]);return JSON.stringify(result);},disconnect(){this.closed=true;}};
  return {game,received,calls,errors,result,net:new BrowserNet(game,'ws://localhost:7790',error=>errors.push(error),Socket)};
}

test('transport preserves text/binary bytes and forwards only Rust-derived presentation data', () => {
  const {net,game,received,calls}=fixture();
  assert.equal(net.tick({},0),null);
  net.socket.readyState=1;
  net.socket.onmessage({data:'text protocol'});
  net.socket.onmessage({data:new TextEncoder().encode('binary protocol').buffer});
  assert.deepEqual(received,['text protocol','binary protocol']);
  net.tick({move_x:1},100);
  assert.deepEqual(calls,[[{move_x:1},100]]);
  assert.deepEqual(net.socket.sent,['protocol bytes']);
  assert.equal(net.status.my_entity,3);assert.equal(net.status.players,2);
  assert.equal(net.status.rtt,12);assert.equal(net.status.entity_count,1);
  assert.deepEqual(net.status.positions,{3:[1,2,3]});
  net.socket.close();assert.equal(net.status.connected,false);assert.equal(game.closed,true);
});

test('malformed frames and slow consumers close rather than dropping protocol data', () => {
  const {net,game,errors}=fixture();
  game.receive=()=>{throw Error('invalid protocol');};
  net.socket.onmessage({data:'bad'});
  assert.equal(errors.length,1);assert.equal(game.closed,true);
  const slow=fixture();slow.net.socket.readyState=1;slow.net.socket.bufferedAmount=5*1024*1024;
  assert.throws(()=>slow.net.tick({},10),/langsam/);
  assert.equal(slow.game.closed,true);assert.deepEqual(slow.net.socket.sent,[]);
});

test('local close detaches old callbacks before a shared NetGame can be reloaded', () => {
  const {net,game}=fixture();
  net.close();
  assert.equal(game.closed,true);
  assert.equal(net.socket.onmessage,null);
  assert.equal(net.socket.onclose,null);
  assert.equal(net.socket.onerror,null);
  assert.equal(net.status.connected,false);
});
