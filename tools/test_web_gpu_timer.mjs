import test from 'node:test';
import assert from 'node:assert/strict';
import { GpuTimer } from '../crates/engine-web/player/gpu-timer.js';

function fixture(supported = true) {
  const state = {ready:false, disjoint:false, lost:false, events:[]};
  const ext = {GPU_DISJOINT_EXT:1, TIME_ELAPSED_EXT:2};
  const gl = {
    QUERY_RESULT_AVAILABLE:3, QUERY_RESULT:4,
    isContextLost: () => state.lost,
    getExtension: name => name === 'EXT_disjoint_timer_query_webgl2' && supported ? ext : null,
    getParameter: () => state.disjoint,
    createQuery: () => { state.events.push('create'); return {}; },
    deleteQuery: () => state.events.push('delete'),
    beginQuery: () => state.events.push('begin'),
    endQuery: () => state.events.push('end'),
    getQueryParameter: (_query, key) => {
      if (key === gl.QUERY_RESULT_AVAILABLE) return state.ready;
      assert.ok(state.ready && !state.disjoint, 'only completed, valid queries may be read');
      state.events.push('read');
      return 2500000;
    },
  };
  return {state, timer:new GpuTimer(gl)};
}

test('GPU timing keeps one in-flight query and reads completed nanoseconds as milliseconds', () => {
  const {state, timer} = fixture();
  timer.begin();
  timer.end();
  timer.begin();
  timer.end();
  assert.deepEqual(state.events, ['create', 'begin', 'end']);
  assert.equal(timer.ms, null);
  state.ready = true;
  timer.begin();
  timer.end();
  assert.equal(timer.ms, 2.5);
  assert.deepEqual(state.events.slice(3), ['read', 'delete', 'create', 'begin', 'end']);
  timer.dispose();
  assert.equal(state.events.at(-1), 'delete');
});

test('disjoint GPU results clear the display and delete the query without reading it', () => {
  const {state, timer} = fixture();
  timer.begin();
  timer.end();
  state.ready = true;
  timer.begin();
  timer.end();
  assert.equal(timer.ms, 2.5);
  state.disjoint = true;
  timer.begin();
  assert.equal(timer.ms, null);
  assert.equal(timer.pending, null);
  assert.deepEqual(state.events.slice(-2), ['end', 'delete']);
});

test('unsupported or lost contexts never begin a query', () => {
  for (const [supported, lost] of [[false, false], [true, true]]) {
    const {state, timer} = fixture(supported);
    state.lost = lost;
    timer.begin();
    timer.end();
    assert.equal(timer.ms, null);
    assert.deepEqual(state.events, []);
  }
});
