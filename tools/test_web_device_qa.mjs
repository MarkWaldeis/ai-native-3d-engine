import test from 'node:test';
import assert from 'node:assert/strict';
import {Trace,digest,instrument,validateReport} from './device_qa/trace.mjs';
test('device traces retain exact inputs, samples, reset/region ordering and bounded reports',()=>{
  const trace=new Trace();trace.call('load',['{}','{}']);trace.call('set_active_region',[0,0,128,10]);
  trace.call('tick',['{"move_x":0.5}'],JSON.stringify({tick:1,state:{score:0,secret:'not exported'}}));
  trace.call('tick',['{}'],JSON.stringify({tick:30,state:{score:3,won:true}}));trace.call('clear_active_region',[]);
  const report=trace.report();validateReport(report);assert.equal(report.trace.last.state.won,true);assert.deepEqual(report.trace.calls[2].input,{move_x:.5});assert.equal(JSON.stringify(report).includes('secret'),false);
  report.trace.calls.length=0;assert.equal(trace.calls.length,5);assert.equal(digest({a:1,b:2}),digest({b:2,a:1}));
});
test('empty, late, malformed and truncated traces do not claim complete replay',()=>{
  assert.throws(()=>new Trace({maxCalls:0}));const empty=new Trace();assert.throws(()=>validateReport(empty.report()));
  empty.call('tick',['{}'],'{"tick":12}');assert.throws(()=>validateReport(empty.report()));
  const capped=new Trace({maxCalls:1,maxEvents:1});capped.event('one');capped.event('two');capped.call('tick',['{}'],'{"tick":1}');capped.call('tick',['{}'],'{"tick":2}');assert.equal(capped.calls.length,1);assert.equal(capped.report().eventsTruncated,true);assert.throws(()=>validateReport(capped.report()));
  assert.throws(()=>new Trace().call('tick',['{"x":"text"}'],'{}'));assert.throws(()=>new Trace().call('unknown',[]));assert.throws(()=>validateReport({}));
});
test('instrumentation preserves receivers, return values, exceptions and gameplay when recording fails',()=>{
  class Game {load(){return '{}';}tick(){this.count=(this.count||0)+1;return JSON.stringify({tick:this.count,state:{}});}set_active_region(){}clear_active_region(){}}
  const original=Game.prototype.tick,trace=new Trace(),undo=instrument(Game,trace),game=new Game();assert.equal(JSON.parse(game.tick('{}')).tick,1);assert.equal(trace.calls.length,1);
  game.tick('bad JSON');assert.equal(game.count,2);assert.equal(trace.stopped,true);undo();assert.equal(Game.prototype.tick,original);
  class Broken extends Game {tick(){throw Error('game error');}}
  const brokenTrace=new Trace();brokenTrace.call('tick',['{}'],'{"tick":1}');instrument(Broken,brokenTrace);assert.throws(()=>new Broken().tick('{}'),/game error/);assert.equal(brokenTrace.report().trace.complete,false);assert.equal(brokenTrace.events[0].kind,'runtime-error');
  const undoFailure=instrument(Game,new Trace(),()=>{throw Error('callback failed');});assert.doesNotThrow(()=>new Game().tick('bad JSON'));undoFailure();
  class Missing extends Game {}Object.defineProperty(Missing.prototype,'clear_active_region',{value:null});assert.throws(()=>instrument(Missing,new Trace()),/Missing/);assert.equal(Object.hasOwn(Missing.prototype,'load'),false);
});
