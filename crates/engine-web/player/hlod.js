import * as THREE from 'three';
import {canInstance,entityRadius} from './streaming.js';

function distanceXZ(box,eye) {
  return Math.hypot(Math.max(box.min.x-eye.x,0,eye.x-box.max.x),Math.max(box.min.z-eye.z,0,eye.z-box.max.z));
}

export function groupRuns(geometry,ranges,visible) {
  geometry.clearGroups();
  let run;
  for(const range of ranges) {
    if(!visible.has(range.key)) {run=null;continue;}
    if(run && run.start+run.count===range.start) run.count+=range.count;
    else {geometry.addGroup(range.start,range.count,0);run=geometry.groups.at(-1);}
  }
}

export function proxyGeometry(e) {
  switch(e.kind) {
    case 'box': return new THREE.BoxGeometry(...e.size);
    case 'plane': return new THREE.PlaneGeometry(e.size,e.size).rotateX(-Math.PI/2);
    case 'sphere': return new THREE.SphereGeometry(e.radius,4,3);
    case 'cylinder': return new THREE.CylinderGeometry(e.radius,e.radius,e.height,4,1);
    case 'gltf': {
      const bounds=e.props?.web_mesh_bounds;
      if(!Array.isArray(bounds) || bounds.length!==2 || bounds.some(v=>!Array.isArray(v) || v.length!==3 || !v.every(Number.isFinite))) return null;
      const [min,max]=bounds;
      return new THREE.BoxGeometry(...min.map((v,i)=>max[i]-v)).translate(...min.map((v,i)=>(v+max[i])/2));
    }
    default: return null;
  }
}

export class HLOD {
  constructor(scene,config) {
    this.scene=scene;this.config=config;this.clusters=new Map();this.entities=new Map();this.owners=new Set();this.desired=new Set();
    this.material=new THREE.MeshStandardMaterial({vertexColors:true,roughness:0.95});
    this.frustum=new THREE.Frustum();this.matrix=new THREE.Matrix4();this.builds=0;
  }
  setOcclusion(occluder) {this.occluder=occluder;}
  clusterKey(chunk) {return chunk.split(',').map(v=>Math.floor(Number(v)/this.config.hlod_cluster)).join(',');}
  eligible(e) {return e.visible!==false && canInstance(e) && entityRadius(e)>=this.config.hlod_min_radius && (e.kind!=='gltf' || !!e.props?.web_mesh_bounds);}
  add(e,chunk) {
    const key=this.clusterKey(chunk);
    if(!this.eligible(e)) {this.remove(e.id);return;}
    if(this.entities.get(e.id)!==key) this.remove(e.id);
    if(!this.clusters.has(key)) this.clusters.set(key,{key,items:new Map(),dirty:true,mesh:null,ranges:[]});
    const cluster=this.clusters.get(key);
    cluster.items.set(e.id,{e,chunk});cluster.dirty=true;cluster.box=null;this.entities.set(e.id,key);
  }
  remove(id) {
    const key=this.entities.get(id);if(key===undefined) return;
    const cluster=this.clusters.get(key);cluster.items.delete(id);cluster.dirty=true;cluster.box=null;this.entities.delete(id);
    if(!cluster.items.size) {this.dispose(cluster);this.clusters.delete(key);}
  }
  bounds(cluster) {
    const box=new THREE.Box3(), size=this.config.chunk_size;
    for(const {e,chunk} of cluster.items.values()) {
      const [x,z]=chunk.split(',').map(Number), p=e.transform?.position || [0,0,0], r=entityRadius(e);
      box.expandByPoint(new THREE.Vector3(Math.min(x*size,p[0]-r),p[1]-r,Math.min(z*size,p[2]-r)));
      box.expandByPoint(new THREE.Vector3(Math.max((x+1)*size,p[0]+r),p[1]+r,Math.max((z+1)*size,p[2]+r)));
    }
    return box;
  }
  build(cluster) {
    const positions=[],normals=[],colors=[],indices=[],ranges=[],dummy=new THREE.Object3D(),size=this.config.chunk_size;
    const items=[...cluster.items.values()].sort((a,b)=>a.chunk.localeCompare(b.chunk) || a.e.id-b.e.id);
    for(const {e,chunk} of items) {
      const source=proxyGeometry(e);if(!source) continue;
      const geometry=source.index ? source.toNonIndexed() : source;
      const t=e.transform || {}, start=indices.length, base=positions.length/3;
      dummy.position.fromArray(t.position || [0,0,0]);dummy.scale.fromArray(t.scale || [1,1,1]);
      dummy.rotation.set(...(t.rotation_deg || [0,0,0]).map(THREE.MathUtils.degToRad),'ZYX');dummy.updateMatrix();geometry.applyMatrix4(dummy.matrix);
      const color=new THREE.Color(...(e.color || [0.8,0.8,0.8]));
      positions.push(...geometry.attributes.position.array);normals.push(...geometry.attributes.normal.array);
      for(let i=0;i<geometry.attributes.position.count;i++) {indices.push(base+i);colors.push(color.r,color.g,color.b);}
      const previous=ranges.at(-1);
      let range=previous;
      if(previous?.key===chunk) previous.count+=indices.length-start;
      else {
        const [x,z]=chunk.split(',').map(Number);
        range={key:chunk,start,count:indices.length-start,box:new THREE.Box3(new THREE.Vector3(x*size,-Infinity,z*size),new THREE.Vector3((x+1)*size,Infinity,(z+1)*size))};ranges.push(range);
      }
      geometry.computeBoundingBox();range.box.union(geometry.boundingBox);
      geometry.dispose();if(source!==geometry) source.dispose();
    }
    const geometry=new THREE.BufferGeometry();
    geometry.setAttribute('position',new THREE.Float32BufferAttribute(positions,3));geometry.setAttribute('normal',new THREE.Float32BufferAttribute(normals,3));geometry.setAttribute('color',new THREE.Float32BufferAttribute(colors,3));geometry.setIndex(indices);
    geometry.computeBoundingBox();geometry.computeBoundingSphere();
    this.dispose(cluster);
    const mesh=new THREE.Mesh(geometry,[this.material]);mesh.castShadow=false;mesh.receiveShadow=false;
    cluster.mesh=mesh;cluster.ranges=ranges;cluster.dirty=false;cluster.box=this.bounds(cluster);this.scene.add(mesh);
  }
  wantsFar(key) {return this.desired.has(key);}
  isFar(key) {return this.owners.has(key);}
  hasChunk(key) {return [...(this.clusters.get(this.clusterKey(key))?.items.values() || [])].some(item=>item.chunk===key);}
  chunkDistance(key,eye) {
    const range=this.clusters.get(this.clusterKey(key))?.ranges.find(range=>range.key===key);
    if(range) return distanceXZ(range.box,eye);
    const [x,z]=key.split(',').map(Number),size=this.config.chunk_size;
    return Math.hypot(Math.max(x*size-eye.x,0,eye.x-(x+1)*size),Math.max(z*size-eye.z,0,eye.z-(z+1)*size));
  }
  update(camera,nearReady=()=>true) {
    this.builds=0;const old=this.owners;this.owners=new Set();
    this.frustum.setFromProjectionMatrix(this.matrix.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
    const {draw_distance:near,hlod_distance:far,hysteresis:h}=this.config;
    const clusters=[...this.clusters.values()].map(cluster=>({cluster,d:distanceXZ(cluster.box ??= this.bounds(cluster),camera.position)})).sort((a,b)=>a.d-b.d);
    for(const {cluster,d} of clusters) {
      if(d>far+h) {for(const range of cluster.ranges) this.desired.delete(range.key);this.dispose(cluster);continue;}
      if(d<far && (cluster.dirty || !cluster.mesh) && this.builds<1) {this.build(cluster);this.builds++;}
      if(!cluster.mesh || cluster.dirty) {if(cluster.mesh) cluster.mesh.visible=false;continue;}
      const visible=new Set();
      for(const range of cluster.ranges) {
        const distance=distanceXZ(range.box,camera.position), was=this.desired.has(range.key);
        const wanted=distance>near+(was?0:h) && distance<far;
        if(wanted) this.desired.add(range.key);else this.desired.delete(range.key);
        if(distance<far && (wanted || old.has(range.key) && !nearReady(range.key))) {visible.add(range.key);this.owners.add(range.key);}
      }
      groupRuns(cluster.mesh.geometry,cluster.ranges,visible);
      cluster.mesh.visible=visible.size>0 && this.frustum.intersectsBox(cluster.mesh.geometry.boundingBox) && !this.occluder?.occluded(`hlod:${cluster.key}`,camera.position,cluster.mesh.geometry.boundingBox);
    }
  }
  stats() {
    let hlodClusters=0,hlodDrawCalls=0,hlodTriangles=0;
    for(const {mesh} of this.clusters.values()) if(mesh?.visible) {hlodClusters++;hlodDrawCalls+=mesh.geometry.groups.length;for(const group of mesh.geometry.groups) hlodTriangles+=group.count/3;}
    return {hlodClusters,hlodDrawCalls,hlodTriangles};
  }
  dispose(cluster) {if(cluster.mesh) {cluster.mesh.removeFromParent();cluster.mesh.geometry.dispose();cluster.mesh=null;}}
  reset() {for(const cluster of this.clusters.values()) this.dispose(cluster);this.clusters.clear();this.entities.clear();this.owners.clear();this.desired.clear();}
}
