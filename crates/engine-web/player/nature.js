import * as T from 'three';
import { mergeGeometries } from './vendor/addons/utils/BufferGeometryUtils.js';
import { foliagePlacements, foliageSettings } from './nature-settings.js';

function merge(parts) {
  const geometry=mergeGeometries(parts);
  for (const part of parts) part.dispose();
  return geometry;
}

function vegetationGeometry() {
  const blades=[], crowns=[], branches=[];
  for(let i=0;i<7;i++) {
    const height=0.5+(i%3)*0.16, blade=new T.PlaneGeometry(0.07,height,1,4);
    blade.translate(0,height/2,0);
    const p=blade.attributes.position;
    for(let j=0;j<p.count;j++){const y=p.getY(j);p.setX(j,p.getX(j)*Math.max(0,1-y/height)+y*y*0.28);}
    blade.rotateY(i*2.4);blade.translate(Math.sin(i*2.4)*0.2,0,Math.cos(i*2.4)*0.2);blade.computeVertexNormals();blades.push(blade);
  }
  const trunk=new T.CylinderGeometry(0.13,0.32,4.5,8);trunk.translate(0,2.25,0);branches.push(trunk);
  for(let i=0;i<11;i++) {
    const angle=i*2.4, x=Math.sin(angle)*1.65,z=Math.cos(angle)*1.65,y=4.7+(i%3)*0.5;
    const crown=new T.IcosahedronGeometry(1.1,1);crown.scale(1.1,0.52,0.8);crown.rotateY(angle);crown.translate(x,y,z);crowns.push(crown);
    if(i<5){const end=new T.Vector3(x,y-0.2,z),start=new T.Vector3(0,3,0),direction=end.clone().sub(start);const branch=new T.CylinderGeometry(0.045,0.12,direction.length(),5);branch.applyQuaternion(new T.Quaternion().setFromUnitVectors(new T.Vector3(0,1,0),direction.normalize()));branch.translate(...start.add(end).multiplyScalar(0.5).toArray());branches.push(branch);}
  }
  return [merge(blades),merge(crowns),merge(branches)];
}

export class NatureVisuals {
  constructor(scene,config,level) {
    Object.assign(this,{scene,config,tiles:new Map(),time:{value:0},viewer:{value:new T.Vector3()},fade:{value:64}});
    this.root=new T.Group();scene.add(this.root);
    this.geometries=vegetationGeometry();
    this.materials=[this.material(0x72b63c,false),this.material(0x438b38,true),this.material(0x796047,true)];
    this.depths=this.materials.map(m=>{const depth=new T.MeshDepthMaterial({depthPacking:T.RGBADepthPacking,side:T.DoubleSide});depth.onBeforeCompile=m.onBeforeCompile;return depth;});
    this.ray=new T.Raycaster();this.ray.ray.direction.set(0,-1,0);
    this.sampler=(x,z)=>this.groundHeight(x,z);
    this.sample=(x,z)=>this.sampler(x,z);
    if(config.water) this.createWater();
    this.setQuality(level);
    window.__aiEngineVisuals=this;
  }
  material(color,tree) {
    const material=new T.MeshStandardMaterial({color,roughness:0.9,side:T.DoubleSide,emissive:color,emissiveIntensity:0.045});
    material.onBeforeCompile=shader=>{
      Object.assign(shader.uniforms,{uTime:this.time,uViewer:this.viewer,uFade:this.fade,uNear:{value:tree?7:0}});
      shader.vertexShader='uniform float uTime; uniform vec3 uViewer; uniform float uFade; uniform float uNear;\n'+shader.vertexShader;
      shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>',`#include <begin_vertex>
        vec3 base = (modelMatrix * instanceMatrix * vec4(0.,0.,0.,1.)).xyz;
        float d = distance(base.xz,uViewer.xz);
        float fade = (1. - smoothstep(uFade * 0.75,uFade,d)) * smoothstep(uNear,uNear+2.,d);
        transformed *= fade;
        transformed.x += sin(uTime * 1.5 + base.x * 0.3 + base.z * 0.2) * max(position.y,0.) * 0.055 * fade;`);
    };
    return material;
  }
  groundHeight(x,z) {
    this.ray.ray.origin.set(x,10000,z);
    const hit=this.ray.intersectObjects(this.grounds || [],true)[0];
    if(!hit) return 0;
    return hit.face?.normal.clone().transformDirection(hit.object.matrixWorld).y >= 0.7 ? hit.point.y : NaN;
  }
  setHeightSampler(sample) {
    if(typeof sample!=='function') throw new TypeError('Height sampler must be a function');
    this.sampler=sample;this.clearTiles();
  }
  createWater() {
    const data=new Uint8Array(64*64*4);
    for(let y=0;y<64;y++) for(let x=0;x<64;x++){const i=(y*64+x)*4;data[i]=128+Math.sin(x*0.7+y*0.3)*30;data[i+1]=128+Math.cos(y*0.8+x*0.2)*30;data[i+2]=250;data[i+3]=255;}
    this.normal=new T.DataTexture(data,64,64);this.normal.wrapS=this.normal.wrapT=T.RepeatWrapping;this.normal.needsUpdate=true;
    this.water=new T.Mesh(new T.PlaneGeometry(2000,2000),new T.MeshPhysicalMaterial({color:this.config.water.color,metalness:0.3,roughness:0.18,normalMap:this.normal,normalScale:new T.Vector2(0.4,0.4),transparent:true,opacity:0.9,clearcoat:1}));
    this.water.rotation.x=-Math.PI/2;this.water.position.y=this.config.water.level;this.root.add(this.water);
  }
  setQuality(level) {
    this.level=level;this.settings=foliageSettings(this.config.foliage,level);this.fade.value=this.settings.distance;
    if(this.water){this.normal.repeat.setScalar([24,48,80][level]);this.water.material.normalScale.setScalar([0.18,0.3,0.4][level]);}
    this.clearTiles();
  }
  tile(x,z) {
    const placements=foliagePlacements(x,z,{density:this.settings.density,sample:this.sample,water:this.water?.position.y ?? -Infinity});
    const trees=placements.filter(p=>p.tree),group=new T.Group(),dummy=new T.Object3D();
    for(let type=0;type<3;type++) {
      const items=type?trees:placements,mesh=new T.InstancedMesh(this.geometries[type],this.materials[type],items.length);
      items.forEach((p,i)=>{dummy.position.set(p.x,p.y,p.z);dummy.rotation.set(0,p.rotation,0);dummy.scale.setScalar(p.scale);dummy.updateMatrix();mesh.setMatrixAt(i,dummy.matrix);mesh.setColorAt(i,new T.Color().setHSL(type===2?0.08:0.25+(i%7)*0.006,0.18,0.72+(i%5)*0.035));});
      mesh.castShadow=type>0;mesh.receiveShadow=true;mesh.customDepthMaterial=this.depths[type];group.add(mesh);
    }
    this.root.add(group);this.tiles.set(`${x},${z}`,group);
  }
  removeTile(key,group) {group.removeFromParent();group.traverse(o=>{if(o.isInstancedMesh)o.dispose();});this.tiles.delete(key);}
  clearTiles() {for(const [key,group] of this.tiles)this.removeTile(key,group);}
  update(elapsed,camera) {
    this.time.value+=elapsed;this.viewer.value.copy(camera.position);
    if(this.water){this.water.position.x=camera.position.x;this.water.position.z=camera.position.z;this.normal.offset.set(this.time.value*0.006,this.time.value*0.004);}
    if(!this.config.foliage || this.settings.density===0) return;
    const cx=Math.floor(camera.position.x/16),cz=Math.floor(camera.position.z/16),r=Math.ceil(this.settings.distance/16);
    for(const [key,group] of this.tiles){const [x,z]=key.split(',').map(Number);if(Math.hypot(x-cx,z-cz)>r)this.removeTile(key,group);}
    for(let d=0;d<=r;d++)for(let x=cx-d;x<=cx+d;x++)for(let z=cz-d;z<=cz+d;z++)if(Math.hypot(x-cx,z-cz)<=r&&!this.tiles.has(`${x},${z}`)){
      this.grounds=[];this.scene.updateMatrixWorld(true);this.scene.traverse(o=>{if(o.userData.support)this.grounds.push(o);});
      this.tile(x,z);return;
    }
  }
  stats() {return {foliageTiles:this.tiles.size,quality:['low','balanced','ultra'][this.level]};}
  dispose() {
    this.clearTiles();this.root.removeFromParent();
    for(const resource of [...this.geometries,...this.materials,...this.depths,this.water?.geometry,this.water?.material,this.normal])resource?.dispose();
    if(window.__aiEngineVisuals===this)delete window.__aiEngineVisuals;
  }
}
