function resources(gltf) {
  const geometry=new Set(), materials=new Set(), textures=new Set(), buffers=new Set();
  gltf.scene.traverse(o=>{
    if(o.geometry) geometry.add(o.geometry);
    for(const mat of (Array.isArray(o.material)?o.material:[o.material])) if(mat) {
      materials.add(mat);
      for(const value of Object.values(mat)) if(value?.isTexture) textures.add(value);
    }
  });
  for(const g of geometry) for(const a of [...Object.values(g.attributes || {}),g.index,...Object.values(g.morphAttributes || {}).flat()]) {
    const buffer=a?.data?.array?.buffer || a?.array?.buffer;
    if(buffer) buffers.add(buffer);
  }
  let bytes=[...buffers].reduce((n,b)=>n+b.byteLength,0);
  for(const t of textures) {
    const images=Array.isArray(t.image)?t.image:[t.image];
    for(const image of images) bytes+=image?.data?.byteLength ?? ((image?.width || 0)*(image?.height || 0)*4*4/3);
  }
  return {geometry,materials,textures,bytes};
}

export class AssetStream {
  constructor(loader,renderer,config={},changed=()=>{},evicted=()=>{}) {
    this.loader=loader; this.renderer=renderer; this.config={asset_concurrency:4,texture_uploads_per_frame:2,asset_cache_mb:256,...config};
    this.changed=changed; this.evicted=evicted; this.entries=new Map(); this.clock=0; this.evictions=0;
  }
  get(path) { const e=this.entries.get(path); return e?.state==='ready'?e.gltf:null; }
  update(wanted,pinned=new Set(wanted.keys())) {
    this.clock++;
    for(const [path,e] of this.entries) {
      e.pinned=pinned.has(path); e.priority=wanted.get(path) ?? Infinity;
      if(e.pinned) e.used=this.clock;
      if(!wanted.has(path) && (e.state==='queued' || e.state==='error' && !e.resources)) this.entries.delete(path);
    }
    for(const [path,priority] of wanted) if(!this.entries.has(path)) this.entries.set(path,{state:'queued',priority,pinned:pinned.has(path),used:this.clock});
    let loading=[...this.entries.values()].filter(e=>e.state==='loading').length;
    for(const [path,e] of [...this.entries].filter(([,e])=>e.state==='queued').sort((a,b)=>a[1].priority-b[1].priority)) {
      if(loading>=this.config.asset_concurrency) break;
      loading++; e.state='loading';
      Promise.resolve().then(()=>this.loader(path)).then(gltf=>{
        e.gltf=gltf; e.resources=resources(gltf); e.pending=[...e.resources.textures]; e.state='uploading';
      }).catch(error=>{ e.state='error'; e.error=error; this.changed(path,error); });
    }
    let budget=this.config.texture_uploads_per_frame;
    for(const [path,e] of [...this.entries].sort((a,b)=>a[1].priority-b[1].priority)) if(e.state==='uploading') {
      try {
        while(budget>0 && e.pending.length) { this.renderer.initTexture(e.pending[0]); e.pending.shift(); budget--; }
        if(!e.pending.length) { e.state='ready'; this.changed(path); }
      } catch(error) { e.state='error'; e.pending=[]; this.changed(path,error); }
    }
    let bytes=[...this.entries.values()].reduce((n,e)=>n+(e.resources?.bytes || 0),0);
    for(const [path,e] of [...this.entries].filter(([,e])=>!e.pinned && e.resources).sort((a,b)=>a[1].used-b[1].used)) {
      if(bytes<=this.config.asset_cache_mb*1024*1024 && e.state!=='error') continue;
      this.entries.delete(path); bytes-=e.resources.bytes; this.evictions++; this.evicted(path);
      for(const kind of ['geometry','materials','textures']) for(const resource of e.resources[kind]) {
        if(![...this.entries.values()].some(other=>other.resources?.[kind].has(resource))) resource.dispose();
      }
    }
  }
  stats() {
    const entries=[...this.entries.values()];
    return {assetsQueued:entries.filter(e=>e.state==='queued').length,assetsLoading:entries.filter(e=>e.state==='loading').length,assetsLoaded:entries.filter(e=>e.state==='ready').length,assetMB:entries.reduce((n,e)=>n+(e.resources?.bytes || 0),0)/1024/1024,textureUploadsPending:entries.reduce((n,e)=>n+(e.pending?.length || 0),0),assetEvictions:this.evictions};
  }
}
