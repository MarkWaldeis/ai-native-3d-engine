import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

test('asset GC: dry-run, liveness, history, locking, malformed input and Blender-source protection',()=>{
  const root=fileURLToPath(new URL('../',import.meta.url));
  const result=spawnSync(process.platform==='win32'?'python':'python3',['-m','unittest','discover','-s','tools','-p','test_asset_gc.py','-v'],{cwd:root,encoding:'utf8',timeout:240000});
  assert.ifError(result.error);
  assert.equal(result.status,0,result.stdout+result.stderr);
  console.log(result.stdout+result.stderr);
});
