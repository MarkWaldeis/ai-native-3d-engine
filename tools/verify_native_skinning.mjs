import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {registerHooks} from 'node:module';
registerHooks({resolve(specifier,context,nextResolve){return nextResolve(specifier==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:specifier,context);}});
const THREE=await import('three');
const {GLTFLoader}=await import('../crates/engine-web/player/vendor/addons/loaders/GLTFLoader.js');
const {Animations}=await import('../crates/engine-web/player/animation.js');
if(!process.argv[2])throw Error('Usage: node tools/verify_native_skinning.mjs NATIVE_POSES.json');
const native=JSON.parse(await readFile(process.argv[2],'utf8')),bytes=await readFile(native.asset);
assert.ok(native.cases.length>=10,'include simulation-derived and manual blend poses');
let maxError=0,vertices=0,nodes=0;
function near(a,b,label) {
  assert.equal(a.length,b.length,label);
  for(let i=0;i<a.length;i++) {
    if(Array.isArray(a[i]))near(a[i],b[i],`${label}/${i}`);
    else {const error=Math.abs(a[i]-b[i]);assert.ok(Number.isFinite(error)&&error<2e-5,`${label}/${i}: ${a[i]} vs ${b[i]}`);maxError=Math.max(maxError,error);}
  }
}
for(const item of native.cases) {
  const gltf=await new GLTFLoader().parseAsync(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),'');
  const animations=new Animations();
  if(item.animator) {
    animations.add({id:1,animator:item.animator},gltf.scene,gltf.animations);
    animations.apply([{id:1,state:{...item.animator,current_state:item.animator.current_state||item.animator.machine.initial}}]);
  }
  gltf.scene.updateMatrixWorld(true);
  const positions=[],matrices=[];
  gltf.scene.traverse(object=>{
    const index=gltf.parser.associations.get(object)?.nodes;
    if(index!==undefined)matrices[index]=[0,1,2,3].map(c=>object.matrixWorld.elements.slice(c*4,c*4+4));
    if(object.isMesh){const p=new THREE.Vector3();for(let i=0;i<object.geometry.attributes.position.count;i++){object.getVertexPosition(i,p);p.applyMatrix4(object.matrixWorld);positions.push(p.toArray());}}
  });
  near(item.vertices,positions,`${item.name}/vertices`);vertices+=positions.length;
  for(let i=0;i<matrices.length;i++)if(matrices[i]){near(item.node_matrices[i],matrices[i],`${item.name}/node/${i}`);nodes++;}
  assert.ok(matrices.some(Boolean),'compare actual GLB node transforms');
  animations.clear();
}
assert.notDeepEqual(native.cases.find(c=>c.name==='idle-0.250').vertices,native.cases.find(c=>c.name==='walk-0.250').vertices,'skin must deform');
console.log(`PASS native/three.js skinning: ${native.cases.length} poses, ${nodes} node matrices, ${vertices} vertices; max error ${maxError.toExponential(3)} (<2e-5); idle/walk/jump, endpoints, crossfade and fixed-tick gameplay samples`);
