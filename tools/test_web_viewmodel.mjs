import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
registerHooks({resolve(s,c,next){return next(s==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:s,c);}});
const T=await import('three');
const {FirstPerson}=await import('../crates/engine-web/player/fps.js');

test('delayed authored viewmodel mounting measures in model space, not the animated weapon parent',async()=>{
  const make=()=>{const f=Object.create(FirstPerson.prototype);f.weapon=new T.Group();f.muzzleFlash=new T.Sprite();return f;};
  const model=()=>{const g=new T.Group();g.add(new T.Mesh(new T.BoxGeometry(.096,.39566,.928).translate(0,.04857,-.208)));return g;};
  const early=make();early.mountModel(model(),true);
  const late=make();late.weapon.position.set(.16,-.15,-.32);late.weapon.rotation.set(.08,.03,.02);late.weapon.updateMatrixWorld(true);
  await Promise.resolve();late.mountModel(model(),true);
  assert.deepEqual(late.model.position.toArray(),early.model.position.toArray());
  assert.deepEqual(late.model.scale.toArray(),early.model.scale.toArray());
  late.weapon.rotation.set(0,0,0);late.weapon.updateMatrixWorld(true);
  const bounds=new T.Box3().setFromObject(late.model),center=bounds.getCenter(new T.Vector3());
  assert.ok(center.distanceTo(late.weapon.position)<1e-7);
  assert.ok(Math.abs(bounds.getSize(new T.Vector3()).z-.4)<1e-7);
  assert.ok(bounds.max.z<-.1,'the receiver stays in front of the overlay camera, not around it');
  late.mountModel(model(),true);assert.equal(late.weapon.children.filter(o=>o!==late.muzzleFlash).length,1);
});
