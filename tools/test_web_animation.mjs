import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import test from 'node:test';
registerHooks({resolve(specifier,context,nextResolve) {return nextResolve(specifier==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:specifier,context);}});
const THREE=await import('three');
const {Animations,cloneSkinned}=await import('../crates/engine-web/player/animation.js');
const {canInstance}=await import('../crates/engine-web/player/streaming.js');

function fixture() {
  const root=new THREE.Group(),bone=new THREE.Bone();bone.name='joint';root.add(bone);
  const geometry=new THREE.BufferGeometry();
  geometry.setAttribute('position',new THREE.Float32BufferAttribute([0,0,0],3));
  geometry.setAttribute('skinIndex',new THREE.Uint16BufferAttribute([0,0,0,0],4));
  geometry.setAttribute('skinWeight',new THREE.Float32BufferAttribute([1,0,0,0],4));
  const mesh=new THREE.SkinnedMesh(geometry,new THREE.MeshBasicMaterial());root.add(mesh);root.updateMatrixWorld(true);mesh.bind(new THREE.Skeleton([bone]));
  const clips=[new THREE.AnimationClip('idle',1,[new THREE.VectorKeyframeTrack('joint.position',[0,1],[0,0,0,0,0,0])]),new THREE.AnimationClip('walk',1,[new THREE.VectorKeyframeTrack('joint.position',[0,1],[0,0,0,1,0,0])])];
  const entity={id:1,kind:'gltf',animator:{machine:{states:{idle:{clip:'idle'},walk:{clip:'walk'}}}}};
  return {root,bone,mesh,clips,entity};
}

test('skinned clones rebind independent skeletons and sim snapshots deterministically crossfade actual bones',()=>{
  const {root,bone,mesh,clips,entity}=fixture(),a=cloneSkinned(root),b=cloneSkinned(root),system=new Animations();
  const aBone=a.getObjectByName('joint'),bBone=b.getObjectByName('joint');
  assert.notEqual(aBone,bone);assert.notEqual(aBone,bBone);assert.equal(a.children.find(c=>c.isSkinnedMesh).skeleton.bones[0],aBone);
  system.apply([{id:1,state:{current_state:'walk',time:.5,previous_state:'idle',previous_time:0,blend_time:1,blend_elapsed:.5}}]);
  system.add(entity,a,clips);system.add({...entity,id:2},b,clips);
  assert.equal(aBone.position.x,.25);assert.equal(bBone.position.x,0);assert.equal(bone.position.x,0);
  system.sample(1);assert.equal(aBone.position.x,.25,'render sampling cannot advance animation time');
  system.apply([{id:1,state:{current_state:'walk',time:1,previous_state:null}}]);assert.equal(aBone.position.x,1);
  system.apply([{id:1,state:{current_state:'walk',time:0,previous_state:null}}]);assert.equal(aBone.position.x,0,'looped sim time resets action pose');
  assert.equal(canInstance(entity),false);system.clear();assert.equal(system.objects.size,0);assert.equal(system.states.size,0);
  assert.equal(mesh.skeleton.bones[0],bone);
});

test('empty/nonanimated objects are harmless; missing clips and invalid external skeletons fail explicitly',()=>{
  const {root,mesh,entity}=fixture(),system=new Animations();
  system.apply();system.sample(999);system.remove(999);system.add({id:1},root,[]);assert.equal(system.objects.size,0);
  assert.throws(()=>system.add(entity,root,[]),/Missing animation clip/);
  mesh.skeleton.bones=[new THREE.Bone()];assert.throws(()=>cloneSkinned(root),/outside its GLB scene/);
});
