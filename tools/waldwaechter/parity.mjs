import assert from 'node:assert/strict';

export function compare(actual, expected, label='snapshot') {
  if(typeof actual==='number' && typeof expected==='number') {
    assert.ok(Number.isFinite(actual) && Math.abs(actual-expected)<=2e-5,`${label}: ${actual} != ${expected}`);
  } else if(Array.isArray(expected)) {
    assert.ok(Array.isArray(actual),label);assert.equal(actual.length,expected.length,label);
    expected.forEach((value,index)=>compare(actual[index],value,`${label}[${index}]`));
  } else if(expected && typeof expected==='object') {
    assert.deepEqual(Object.keys(actual).sort(),Object.keys(expected).sort(),label);
    for(const [key,value] of Object.entries(expected))compare(actual[key],value,`${label}.${key}`);
  } else assert.equal(actual,expected,label);
}
