import * as THREE from 'three';
import { GLTFLoader } from './vendor/addons/loaders/GLTFLoader.js';
import { ChunkGrid, batchKey, canInstance, entityRadius, selectLOD } from './streaming.js';
import { isCollectible } from './presentation.js';
import { primitiveMaterial, tuneAssetTextures } from './materials.js';
import { sunShadowSettings } from './graphics-settings.js';

export class WebAssets {
  constructor(anisotropy = 1) {
    this.anisotropy = anisotropy;
    this.cache = new Map();
    this.loader = new GLTFLoader();
  }
  load(path) {
    if (!this.cache.has(path)) this.cache.set(path, this.loader.loadAsync(path).then(gltf => {
      tuneAssetTextures(gltf.scene, this.anisotropy);
      return gltf;
    }));
    return this.cache.get(path);
  }
}

export class StreamingWorld {
  constructor(scene, renderer, config, assets = new WebAssets()) {
    this.scene = scene; this.renderer = renderer; this.config = config;
    this.assets = assets;
    this.grid = new ChunkGrid(config); this.entities = new Map(); this.groups = new Map(); this.builds = new Map(); this.templates = new Map(); this.dirty = new Set();
    this.frustum = new THREE.Frustum(); this.matrix = new THREE.Matrix4(); this.dummy = new THREE.Object3D();
  }
  setOcclusion(occluder) { this.occluder=occluder; }
  async template(e, level) {
    const key = `${batchKey(e)}:${level}`;
    if (this.templates.has(key)) return this.templates.get(key);
    const promise = (async () => {
      if (e.kind === 'gltf') {
        const lodPath = e.props?.web_lods?.[level];
        const gltf = await this.assets.load(lodPath || e.path);
        if (level === 2 && !lodPath) return this.impostor(gltf.scene);
        gltf.scene.updateMatrixWorld(true);
        const parts = [];
        gltf.scene.traverse(o => { if (o.isMesh) parts.push({geometry:o.geometry,material:o.material,matrix:o.matrixWorld.clone()}); });
        return parts;
      }
      const segments = level ? 6 : e.segments || 24;
      let geometry;
      switch (e.kind) {
        case 'box': geometry = new THREE.BoxGeometry(...e.size); break;
        case 'plane': geometry = new THREE.PlaneGeometry(e.size,e.size); geometry.rotateX(-Math.PI/2); break;
        case 'sphere': geometry = new THREE.SphereGeometry(e.radius,segments,Math.max(4,segments/2)); break;
        case 'cylinder': geometry = new THREE.CylinderGeometry(e.radius,e.radius,e.height,segments); break;
        default: return [];
      }
      return [{geometry,material:primitiveMaterial(e),matrix:new THREE.Matrix4()}];
    })();
    this.templates.set(key,promise); return promise;
  }
  impostor(source) {
    const scene = new THREE.Scene(), model = source.clone(true); scene.add(model);
    const bounds = new THREE.Box3().setFromObject(model), center = bounds.getCenter(new THREE.Vector3()), size = bounds.getSize(new THREE.Vector3());
    const span = Math.max(size.x,size.y,size.z) * 1.1;
    const camera = new THREE.OrthographicCamera(-span/2,span/2,span/2,-span/2,0.01,span*5);
    camera.position.copy(center).add(new THREE.Vector3(0,0,span*2)); camera.lookAt(center);
    scene.add(new THREE.HemisphereLight(0xffffff,0x888888,3));
    const target = new THREE.WebGLRenderTarget(128,128);
    const previous = this.renderer.getRenderTarget(), color = this.renderer.getClearColor(new THREE.Color()), alpha = this.renderer.getClearAlpha();
    this.renderer.setRenderTarget(target); this.renderer.setClearColor(0,0); this.renderer.render(scene,camera);
    this.renderer.setRenderTarget(previous); this.renderer.setClearColor(color,alpha);
    const geometry = new THREE.PlaneGeometry(span,span); geometry.translate(center.x,center.y,center.z);
    return [{geometry,material:new THREE.MeshBasicMaterial({map:target.texture,transparent:true,alphaTest:0.1,side:THREE.DoubleSide}),matrix:new THREE.Matrix4(),billboard:true}];
  }
  add(e, object) {
    object?.removeFromParent();
    const t = e.transform || {position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]};
    const radius = entityRadius(e);
    const key = this.grid.register(e.id,t.position,radius);
    const shadows = object ? null : sunShadowSettings(e.props?.ai_sun_shadow);
    const signature = shadows ? `${batchKey(e)}:${shadows.castShadow}:${shadows.receiveShadow}` : null;
    this.entities.set(e.id,{e,object,key,signature,shadows,level:undefined}); this.dirty.add(key);
  }
  batched(e) { return this.config.instancing && canInstance(e); }
  remove(id) {
    const item = this.entities.get(id); if (!item) return;
    this.dirty.add(item.key); item.object?.removeFromParent(); this.entities.delete(id); this.grid.remove(id);
    const group = this.groups.get(item.key);
    group?.traverse(o => { if (o.isInstancedMesh) for (const index of o.userData.ids?.get(id) || []) { o.setMatrixAt(index,new THREE.Matrix4().makeScale(0,0,0)); o.instanceMatrix.needsUpdate = true; } });
  }
  move(id, transform) {
    const item = this.entities.get(id); if (!item) return;
    const old = item.key; item.e.transform = transform; item.key = this.grid.move(id,transform.position);
    if (old !== item.key || !item.object) { this.dirty.add(old); this.dirty.add(item.key); }
  }
  async rebuild(key) {
    const old = this.groups.get(key), group = new THREE.Group(); this.builds.set(key,group);
    const buckets = new Map(), objects = [];
    for (const {id} of this.grid.chunks.get(key)?.values() || []) {
      const item = this.entities.get(id); if (!item) continue;
      if (item.object) { objects.push(item.object); continue; }
      if (item.e.visible === false || item.level === 3) continue;
      const batch = `${item.signature}:${item.level || 0}:${!!item.e.collider}`;
      if (!buckets.has(batch)) buckets.set(batch,[]); buckets.get(batch).push(item);
    }
    try {
      for (const items of buckets.values()) {
        const parts = await this.template(items[0].e,items[0].level || 0);
        if (this.builds.get(key) !== group) return;
        for (const part of parts) {
          const mesh = new THREE.InstancedMesh(part.geometry,part.material,items.length); mesh.userData.ids = new Map(); mesh.userData.billboard = part.billboard; mesh.userData.items = items;
          mesh.castShadow = items[0].shadows.castShadow && !part.billboard;
          mesh.receiveShadow = items[0].shadows.receiveShadow;
          mesh.userData.support = !!items[0].e.collider;
          items.forEach((item,index) => { this.setMatrix(mesh,index,item,part.matrix); mesh.userData.ids.set(item.e.id,[index]); });
          mesh.computeBoundingSphere(); group.add(mesh);
        }
      }
      if (this.builds.get(key) !== group) return;
      for (const object of objects) group.add(object);
      group.userData.bounds = new THREE.Box3().setFromObject(group);
      old?.removeFromParent(); old?.traverse(o => { if (o.isInstancedMesh) o.dispose(); });
      this.groups.set(key,group);
      if (this.grid.loaded.has(key)) this.scene.add(group);
    } finally {
      if (this.builds.get(key) === group) this.builds.delete(key);
      if (this.groups.get(key) !== group) group.traverse(o => { if (o.isInstancedMesh) o.dispose(); });
    }
  }
  setMatrix(mesh,index,item,local = new THREE.Matrix4(),camera) {
    const t = item.e.transform;
    this.dummy.position.fromArray(t.position); this.dummy.scale.fromArray(t.scale);
    this.dummy.rotation.set(...t.rotation_deg.map(THREE.MathUtils.degToRad),'ZYX');
    if (camera) this.dummy.quaternion.copy(camera.quaternion);
    this.dummy.updateMatrix(); mesh.setMatrixAt(index,this.matrix.copy(this.dummy.matrix).multiply(local));
  }
  update(camera) {
    const tasks = []; this.occludedChunks=0;
    const change = this.grid.update(camera.position.toArray());
    for (const key of change.unload) this.groups.get(key)?.removeFromParent();
    for (const key of change.load) { if (this.groups.has(key)) this.scene.add(this.groups.get(key)); else this.dirty.add(key); }
    this.frustum.setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
    for (const key of this.grid.loaded) {
      for (const {id,position,radius} of this.grid.chunks.get(key)?.values() || []) {
        const item = this.entities.get(id); if (!item) continue;
        const level = selectLOD(Math.max(0,camera.position.distanceTo(new THREE.Vector3(...position))-radius),item.level,this.config);
        if (level !== item.level) {
          item.level = level;
          if (!item.object) this.dirty.add(key);
          else if (['sphere','cylinder'].includes(item.e.kind)) {
            const mesh = item.object.children[0];
            const segments = level ? 6 : item.e.segments || 24;
            mesh.geometry.dispose();
            mesh.geometry = item.e.kind === 'sphere' ? new THREE.SphereGeometry(item.e.radius,segments,Math.max(4,segments/2)) : new THREE.CylinderGeometry(item.e.radius,item.e.radius,item.e.height,segments);
          }
        }
        if (item.object) item.object.visible = item.e.visible !== false && level !== 3;
        if (item.object && item.e.kind === 'gltf' && !item.e.animator) {
          const lodPath = item.e.props?.web_lods?.[level];
          if (lodPath) {
            item.lods ??= new Map(); item.lodTasks ??= new Map();
            if (!item.lodTasks.has(lodPath)) {
              const task = this.assets.load(lodPath).then(gltf => {
                if (this.entities.get(id) !== item) return;
                const lod = gltf.scene.clone(true); item.lods.set(lodPath,lod); item.object.add(lod);
              });
              item.lodTasks.set(lodPath,task); tasks.push(task); task.catch(error => console.error(error));
            }
            item.object.children[0].visible = !item.lods.has(lodPath);
            for (const [path,lod] of item.lods) lod.visible = path === lodPath;
            if (item.impostor) item.impostor.visible = false;
            continue;
          }
          for (const lod of item.lods?.values() || []) lod.visible = false;
          if (level === 2 && !item.impostorTask) {
            item.impostorTask = this.template(item.e,2).then(([part]) => {
              if (!this.entities.has(id)) return;
              item.impostor = new THREE.Mesh(part.geometry,part.material); item.impostor.userData.sharedAsset = true;
              item.object.add(item.impostor);
            });
            item.impostorTask.catch(error => console.error(error));
          }
          item.object.children[0].visible = level !== 2 || !item.impostor;
          if (item.impostor) {
            item.impostor.visible = level === 2;
            item.impostor.quaternion.copy(item.object.quaternion).invert().multiply(camera.quaternion);
          }
        }
      }
      const group = this.groups.get(key);
      if (group) {
        for (const {id} of this.grid.chunks.get(key)?.values() || []) {
          const item = this.entities.get(id);
          if (item?.object && group.userData.bounds) group.userData.bounds.expandByObject(item.object);
        }
        group.visible = !group.userData.bounds || this.frustum.intersectsBox(group.userData.bounds);
        if(group.visible && this.occluder?.occluded(`chunk:${key}`,camera.position,group.userData.bounds)) {group.visible=false;this.occludedChunks++;}
        group.traverse(o => { if (o.userData.billboard) { o.userData.items.forEach((item,i) => this.setMatrix(o,i,item,undefined,camera)); o.instanceMatrix.needsUpdate = true; } });
      }
    }
    for (const key of this.dirty) if (this.grid.loaded.has(key)) { this.dirty.delete(key); const task = this.rebuild(key); tasks.push(task); task.catch(error => { console.error(error); }); }
    return tasks;
  }
  support(position) {
    const result = [];
    const size = this.config.chunk_size, reach = Math.ceil((20 + this.grid.maxRadius)/size);
    const cx = Math.floor(position.x/size), cz = Math.floor(position.z/size);
    for (let x=cx-reach;x<=cx+reach;x++) for (let z=cz-reach;z<=cz+reach;z++) {
      this.groups.get(`${x},${z}`)?.traverse(o => { if (o.userData.support) result.push(o); });
    }
    return result;
  }
  activeObjects() {
    const result = new Map();
    for (const key of this.grid.loaded) for (const {id} of this.grid.chunks.get(key)?.values() || []) {
      const object = this.entities.get(id)?.object;
      if (object) result.set(id,object);
    }
    return result;
  }
  objectiveEntries(collectibleTag) {
    const result = [];
    for (const key of this.grid.loaded) for (const {id} of this.grid.chunks.get(key)?.values() || []) {
      const item = this.entities.get(id);
      if (item && item.e.visible !== false && item.level !== 3 && (item.e.tags?.some(t => ['player','portal','goal'].includes(t)) || isCollectible({tags:item.e.tags || []},collectibleTag))) result.push({id,tags:item.e.tags,position:item.e.transform.position});
    }
    return result;
  }
  reset() { for (const group of this.groups.values()) { group.removeFromParent(); group.traverse(o => { if (o.isInstancedMesh) o.dispose(); }); } this.groups.clear(); this.builds.clear(); this.entities.clear(); this.grid = new ChunkGrid(this.config); this.dirty.clear(); }
}
