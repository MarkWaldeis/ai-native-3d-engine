import test from 'node:test';
import assert from 'node:assert/strict';
import { GameAudio, feedbackEvents, qualitySettings, nextObjective, objectiveBearing } from '../crates/engine-web/player/presentation.js';

test('objective guidance chooses the nearest remaining collectible, then the exit', () => {
  const goal={tags:['portal'],position:[1,0,0]},far={tags:['crystal'],position:[8,1,0]},near={tags:['coin'],position:[3,1,0]};
  assert.equal(nextObjective([], [0,0,0]),null);
  assert.equal(nextObjective([{tags:['ground'],position:[0,0,0]}],[0,0,0]),null);
  assert.equal(nextObjective([goal,far,near],[0,0,0]),near);
  assert.equal(nextObjective([goal,far],[0,0,0]),far);
  assert.equal(nextObjective([goal],[0,0,0]),goal);
  assert.equal(objectiveBearing(0,1),0);
  assert.equal(objectiveBearing(1,0),Math.PI/2);
  assert.equal(objectiveBearing(1,0,true),-Math.PI/2);
});

test('quality tiers bound retina rendering and keep small mobile shadow maps', () => {
  assert.deepEqual(qualitySettings(2,3),{pixelRatio:1.5,shadowSize:1024});
  assert.deepEqual(qualitySettings(1,3),{pixelRatio:1,shadowSize:512});
  assert.deepEqual(qualitySettings(0,3),{pixelRatio:0.8,shadowSize:512});
  assert.equal(qualitySettings(2,1).pixelRatio,1);
});
test('feedback only fires for new collection, confirmed jump and win transitions', () => {
  assert.deepEqual(feedbackEvents(null,{score:3,won:true}),{collect:false,jump:false,win:false});
  assert.deepEqual(feedbackEvents({},{}),{collect:false,jump:false,win:false});
  assert.deepEqual(feedbackEvents({score:1,'Sprünge':0},{score:2,'Sprünge':1,won:true}),{collect:true,jump:true,win:true});
  assert.deepEqual(feedbackEvents({score:3,won:true},{score:3,won:true}),{collect:false,jump:false,win:false});
  assert.equal(feedbackEvents({score:3},{score:0}).collect,false);
});
test('audio is gesture-lazy, synthesized, muteable and safe without WebAudio', () => {
  const original = globalThis.AudioContext, originalWebkit = globalThis.webkitAudioContext;
  let created=0, tones=0, volume=1;
  const parameter = {setValueAtTime(value){volume=value;},linearRampToValueAtTime(){},exponentialRampToValueAtTime(){}};
  globalThis.AudioContext = class {
    constructor(){created++;this.state='suspended';this.currentTime=0;}
    createGain(){return {gain:parameter,connect(){},disconnect(){}};}
    createOscillator(){tones++;return {frequency:parameter,connect(){},start(){},stop(){},disconnect(){}};}
    resume(){this.state='running';return Promise.resolve();}
    suspend(){this.state='suspended';return Promise.resolve();}
  };
  try {
    const audio=new GameAudio();audio.play('collect');assert.equal(created,0);
    audio.unlock();audio.play('jump');audio.play('collect');audio.play('win');assert.equal(tones,8);
    audio.muted=true;assert.equal(volume,0);audio.play('collect');assert.equal(tones,8);
    audio.muted=false;assert.equal(volume,1);audio.suspend();audio.play('collect');assert.equal(tones,8);
    delete globalThis.AudioContext;delete globalThis.webkitAudioContext;
    const unavailable=new GameAudio();assert.doesNotThrow(()=>{unavailable.unlock();unavailable.play('win');unavailable.suspend();});
  } finally {globalThis.AudioContext=original;globalThis.webkitAudioContext=originalWebkit;}
});
