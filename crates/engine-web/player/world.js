import * as THREE from 'three';
import { AssetStream } from './asset-streaming.js';
import { GLTFLoader } from './vendor/addons/loaders/GLTFLoader.js';
import { ChunkGrid, batchKey, canInstance, entityRadius, selectLOD } from './streaming.js';
import { isCollectible } from './presentation.js';
import { primitiveMaterial, tuneAssetTextures } from './materials.js';
import { materialMapSettings, sunShadowSettings } from './graphics-settings.js';

function disposeChunk(group) {
  if (!group) return;
  group.removeFromParent();
  for (const child of group.children) if (child.isInstancedMesh) child.dispose();
  group.clear();
}

export function disposeObject(object) {
  object.removeFromParent();
  object.traverse(child => {
    if (child.isInstancedMesh) child.dispose();
    if (child.userData.sharedAsset) return;
    child.geometry?.dispose();
    for (const mat of (Array.isArray(child.material) ? child.material : [child.material])) {
      if (!mat) continue;
      for (const value of Object.values(mat)) if (value?.isTexture && !value.userData.shared) value.dispose();
      mat.dispose();
    }
  });
}

function impostorMaterial(texture, center, views) {
  const material = new THREE.MeshBasicMaterial({map:texture, transparent:true, alphaTest:0.1, side:THREE.DoubleSide});
  material.customProgramCacheKey = () => `impostor:${views}`;
  material.onBeforeCompile = shader => {
    shader.uniforms.impostorCenter = {value:center};
    shader.vertexShader = 'uniform vec3 impostorCenter;\n' + shader.vertexShader;
    const atlas = views === 4 ? `
      vec3 toEye = cameraPosition - (billboardModel * vec4(impostorCenter, 1.0)).xyz;
      vec3 xAxis = billboardModel[0].xyz, zAxis = billboardModel[2].xyz;
      vec2 localEye = vec2(dot(toEye, xAxis) / max(dot(xAxis, xAxis), 1e-8),
                           dot(toEye, zAxis) / max(dot(zAxis, zAxis), 1e-8));
      float yaw = dot(localEye, localEye) > 1e-8 ? atan(localEye.x, localEye.y) : 0.0;
      float cell = mod(floor(yaw * 0.63661977 + 0.5), 4.0);
      vec2 tile = vec2(mod(cell, 2.0), floor(cell * 0.5));
      vMapUv = (tile + mix(vec2(0.5 / 64.0), vec2(63.5 / 64.0), vMapUv)) * 0.5;
    ` : '';
    shader.vertexShader = shader.vertexShader.replace('#include <project_vertex>', `
      mat4 billboardModel = modelMatrix;
      #ifdef USE_INSTANCING
        billboardModel *= instanceMatrix;
      #endif
      mat4 billboardView = viewMatrix * billboardModel;
      vec4 mvPosition = billboardView * vec4(impostorCenter, 1.0);
      mvPosition.xy += (transformed.xy - impostorCenter.xy)
        * vec2(length(billboardView[0].xyz), length(billboardView[1].xyz));
      gl_Position = projectionMatrix * mvPosition;
      ${atlas}
    `);
  };
  return material;
}

export class WebAssets {
  constructor(anisotropy = 1) {
    this.anisotropy = anisotropy;
    this.cache = new Map();
    this.loader = new GLTFLoader();
    this.materialSources = new Map();
    this.materialVariants = new Map();
  }
  load(path) {
    if (!this.cache.has(path)) this.cache.set(path, this.loader.loadAsync(path).then(gltf => {
      tuneAssetTextures(gltf.scene, this.anisotropy);
      return gltf;
    }));
    return this.cache.get(path);
  }
  async preloadMaterialMaps(entities, loader = new THREE.TextureLoader()) {
    const paths = new Set();
    for (const entity of entities) {
      if (entity.kind === 'gltf') continue;
      const maps = materialMapSettings(entity.props?.ai_material);
      if (maps) for (const path of [maps.map, maps.normalMap, maps.ormMap]) {
        if (path) paths.add(path);
      }
    }
    await Promise.all([...paths].map(async path => {
      try { this.materialSources.set(path, await loader.loadAsync(path)); }
      catch (error) { console.warn(`Material texture unavailable: ${path}`, error); }
    }));
  }
  materialTexture(path, repeat, colorSpace = THREE.NoColorSpace) {
    const source = this.materialSources.get(path);
    if (!source) return null;
    const key = JSON.stringify([path, repeat, colorSpace]);
    if (!this.materialVariants.has(key)) {
      const texture = source.clone();
      texture.colorSpace = colorSpace;
      texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
      texture.repeat.fromArray(repeat);
      texture.anisotropy = this.anisotropy;
      texture.userData.shared = true;
      texture.needsUpdate = true;
      this.materialVariants.set(key, texture);
    }
    return this.materialVariants.get(key);
  }
}

export class StreamingWorld {
  constructor(scene, renderer, config, assets = new WebAssets()) {
    this.scene = scene; this.renderer = renderer; this.config = config;
    this.assets = assets;
    this.grid = new ChunkGrid(config); this.entities = new Map(); this.groups = new Map(); this.builds = new Map(); this.templates = new Map(); this.dirty = new Set();
    this.matrix = new THREE.Matrix4(); this.dummy = new THREE.Object3D();
    this.streamTemplates=new Map();
    this.assetStream = new AssetStream(async path=>{
      const gltf=await assets.loader.loadAsync(path);
      tuneAssetTextures(gltf.scene,assets.anisotropy);
      return gltf;
    },renderer,config,path=>{
      for(const item of this.entities.values()) if(this.grid.loaded.has(item.key) && this.assetPaths(item.e).includes(path)) this.dirty.add(item.key);
    },path=>{
      for(const [key,parts] of this.streamTemplates) if(key===path) {
        for(const part of parts) {part.geometry.dispose();part.material.dispose();part.dispose?.();}
        this.streamTemplates.delete(key);
      }
    });
  }
  assetPaths(e) { return e.kind==='gltf' ? [...new Set([e.path,...Object.values(e.animator ? {} : e.props?.web_lods || {})].filter(Boolean))] : []; }
  assetChoice(item) {
    if(item.e.animator) return {request:item.e.path,path:item.e.path};
    const lods=item.e.props?.web_lods || {}, coarse=Object.keys(lods).map(Number).sort((a,b)=>b-a).map(i=>lods[i]).find(Boolean);
    const desired=lods[item.level || 0] || item.e.path;
    const ready=this.assetPaths(item.e).filter(path=>this.assetStream.get(path));
    return {request:coarse && !ready.length ? coarse : desired,path:this.assetStream.get(desired) ? desired : ready.includes(coarse) ? coarse : ready[0]};
  }
  placeholder(e) {
    const bounds=e.props?.web_mesh_bounds;
    if(!bounds) return [];
    const key=`placeholder:${JSON.stringify(bounds)}`;
    if(!this.templates.has(key)) {
      const geometry=new THREE.BoxGeometry(...bounds[0].map((v,i)=>bounds[1][i]-v));
      geometry.translate(...bounds[0].map((v,i)=>(bounds[1][i]+v)/2));
      this.templates.set(key,[{geometry,material:new THREE.MeshStandardMaterial({color:0x888888,wireframe:true}),matrix:new THREE.Matrix4()}]);
    }
    return this.templates.get(key);
  }
  streamedParts(item) {
    const {path}=this.assetChoice(item), gltf=this.assetStream.get(path);
    if(!gltf) return this.placeholder(item.e);
    if(item.level===2 && !item.e.props?.web_lods?.[2] && !item.e.animator) {
      if(!this.streamTemplates.has(path)) this.streamTemplates.set(path,this.impostor(gltf.scene));
      return this.streamTemplates.get(path);
    }
    gltf.scene.updateMatrixWorld(true);
    const parts=[];
    gltf.scene.traverse(o=>{if(o.isMesh) parts.push({geometry:o.geometry,material:o.material,matrix:o.matrixWorld.clone()});});
    return parts;
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
      return [{geometry,material:primitiveMaterial(e,this.assets),matrix:new THREE.Matrix4()}];
    })();
    this.templates.set(key,promise); return promise;
  }
  impostor(source) {
    const scene = new THREE.Scene(), model = source.clone(true); scene.add(model);
    const bounds = new THREE.Box3().setFromObject(model), center = bounds.getCenter(new THREE.Vector3()), size = bounds.getSize(new THREE.Vector3());
    const span = Math.max(size.x,size.y,size.z) * 1.1;
    const camera = new THREE.OrthographicCamera(-span/2,span/2,span/2,-span/2,0.01,span*5);
    scene.add(new THREE.HemisphereLight(0xffffff,0x888888,3));
    const views = this.config.impostor_views ?? 1, tileSize = views === 4 ? 64 : 128;
    const target = new THREE.WebGLRenderTarget(128,128), renderer = this.renderer;
    const previous = renderer.getRenderTarget(), face = renderer.getActiveCubeFace(), mip = renderer.getActiveMipmapLevel();
    const color = renderer.getClearColor(new THREE.Color()), alpha = renderer.getClearAlpha(), autoClear = renderer.autoClear;
    target.scissorTest = views > 1;
    try {
      renderer.setClearColor(0,0);
      renderer.autoClear = false;
      for (let i = 0; i < views; i++) {
        const angle = i * Math.PI / 2;
        camera.position.set(Math.sin(angle),0,Math.cos(angle)).multiplyScalar(span * 2).add(center);
        camera.lookAt(center);
        target.viewport.set((i % 2) * tileSize, Math.floor(i / 2) * tileSize, tileSize, tileSize);
        target.scissor.copy(target.viewport);
        renderer.setRenderTarget(target);
        renderer.clear();
        renderer.render(scene,camera);
      }
    } catch (error) {
      target.dispose();
      throw error;
    } finally {
      renderer.setRenderTarget(previous,face,mip); renderer.setClearColor(color,alpha);
      renderer.autoClear = autoClear;
    }
    const geometry = new THREE.PlaneGeometry(span,span); geometry.translate(center.x,center.y,center.z);
    return [{geometry,material:impostorMaterial(target.texture,center,views),matrix:new THREE.Matrix4(),billboard:true,dispose:()=>target.dispose()}];
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
    const old = this.groups.get(key), group = new THREE.Group();
    disposeChunk(this.builds.get(key));
    this.builds.set(key,group);
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
        const parts = items[0].e.kind==='gltf' ? this.streamedParts(items[0]) : await this.template(items[0].e,items[0].level || 0);
        if (this.builds.get(key) !== group) return;
        for (const part of parts) {
          const mesh = new THREE.InstancedMesh(part.geometry,part.material,items.length); mesh.userData.ids = new Map();
          mesh.castShadow = items[0].shadows.castShadow && !part.billboard;
          mesh.receiveShadow = items[0].shadows.receiveShadow;
          mesh.userData.support = !!items[0].e.collider;
          items.forEach((item,index) => { this.setMatrix(mesh,index,item,part.matrix); mesh.userData.ids.set(item.e.id,[index]); });
          mesh.computeBoundingSphere(); group.add(mesh);
        }
      }
      if (this.builds.get(key) !== group) return;
      for (const object of objects) group.add(object);
      disposeChunk(old);
      this.groups.set(key,group);
      // Three.js culls meshes per pass; offscreen chunks can still cast shadows.
      if (this.grid.loaded.has(key)) this.scene.add(group);
    } finally {
      if (this.builds.get(key) === group) this.builds.delete(key);
      if (this.groups.get(key) !== group) disposeChunk(group);
    }
  }
  setMatrix(mesh,index,item,local) {
    const t = item.e.transform;
    this.dummy.position.fromArray(t.position); this.dummy.scale.fromArray(t.scale);
    this.dummy.rotation.set(...t.rotation_deg.map(THREE.MathUtils.degToRad),'ZYX');
    this.dummy.updateMatrix(); mesh.setMatrixAt(index,this.matrix.copy(this.dummy.matrix).multiply(local));
  }
  update(camera) {
    const tasks = []; this.occludedChunks=0;
    const change = this.grid.update(camera.position.toArray());
    for (const key of change.unload) {
      for(const {id} of this.grid.chunks.get(key)?.values() || []) {
        const item=this.entities.get(id);
        if(item?.object?.userData.streamedGltf) {
          item.object.userData.streamedGltf(null,[]);
          item.streamedPath=undefined; item.placeholder=false;
        }
      }
      disposeChunk(this.groups.get(key));
      disposeChunk(this.builds.get(key));
      this.groups.delete(key); this.builds.delete(key); this.dirty.delete(key);
    }
    for (const key of change.load) this.dirty.add(key);
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
        if (item.object && item.e.kind === 'gltf' && !item.object.userData.streamedGltf && !item.e.animator) {
          const lodPath = item.e.props?.web_lods?.[level];
          if (lodPath) {
            item.lods ??= new Map(); item.lodTasks ??= new Map();
            if (!item.lodTasks.has(lodPath)) {
              const task = this.assets.load(lodPath).then(gltf => {
                if (this.entities.get(id) !== item) return;
                const lod = gltf.scene.clone(true), shadows = sunShadowSettings(item.e.props?.ai_sun_shadow);
                lod.traverse(child => { if (child.isMesh) Object.assign(child, shadows); });
                item.lods.set(lodPath,lod); item.object.add(lod);
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
          if (item.impostor) item.impostor.visible = level === 2;
        }
      }
      const group = this.groups.get(key);
      if (group) {
        const bounds = this.occluder ? new THREE.Box3().setFromObject(group) : null;
        group.visible = !this.occluder?.occluded(`chunk:${key}`,camera.position,bounds);
        if (!group.visible) this.occludedChunks++;
      }
    }
    const wanted=new Map(), pinned=new Set();
    for(const item of this.entities.values()) if(this.grid.loaded.has(item.key) && item.e.kind==='gltf' && (!item.object || item.object.userData.streamedGltf)) {
      for(const path of this.assetPaths(item.e)) pinned.add(path);
      if(item.level===3 || item.e.visible===false) continue;
      const [x,z]=item.key.split(',').map(Number), size=this.config.chunk_size;
      const distance=Math.hypot(Math.max(x*size-camera.position.x,0,camera.position.x-(x+1)*size),Math.max(z*size-camera.position.z,0,camera.position.z-(z+1)*size));
      const choice=this.assetChoice(item);
      wanted.set(choice.request,Math.min(distance,wanted.get(choice.request) ?? Infinity));
    }
    this.assetStream.update(wanted,pinned);
    for(const item of this.entities.values()) if(this.grid.loaded.has(item.key) && item.object?.userData.streamedGltf) {
      const {path}=this.assetChoice(item), gltf=this.assetStream.get(path);
      if(item.streamedPath!==path || item.streamedLevel!==item.level || item.streamedPath===undefined && !item.placeholder) {
        const billboard=gltf && item.level===2 && !item.e.props?.web_lods?.[2] && !item.e.animator;
        item.object.userData.streamedGltf(billboard?null:gltf,billboard?this.streamedParts(item):this.placeholder(item.e));
        item.streamedPath=path; item.streamedLevel=item.level; item.placeholder=!gltf;
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
  reset() {
    for (const group of this.groups.values()) disposeChunk(group);
    for (const group of this.builds.values()) disposeChunk(group);
    this.groups.clear(); this.builds.clear(); this.entities.clear(); this.grid = new ChunkGrid(this.config); this.dirty.clear();
  }
}
