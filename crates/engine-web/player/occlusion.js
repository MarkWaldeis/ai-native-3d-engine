export class TerrainOcclusion {
  constructor(heightAt, {enabled=true, margin=2, steps=32, budget=96} = {}) {
    this.heightAt=heightAt; this.enabled=enabled; this.margin=margin; this.steps=steps; this.budget=budget;
    this.cache=new Map(); this.cursor=0; this.frame=0; this.tests=0;
  }
  beginFrame() {
    this.frame++; this.tests=0;
    for(const [key,item] of this.cache) if(this.frame-item.frame>120) this.cache.delete(key);
    const keys=[...this.cache.keys()]; this.allowed=new Set();
    for(let i=0;i<Math.min(this.budget,keys.length);i++) this.allowed.add(keys[(this.cursor+i)%keys.length]);
    this.cursor=keys.length ? (this.cursor+this.budget)%keys.length : 0;
  }
  safe(eye,box) {
    const m=this.margin;
    return !this.enabled || !box || !Number.isFinite(box.max.y) ||
      (eye.x>=box.min.x-m && eye.x<=box.max.x+m && eye.z>=box.min.z-m && eye.z<=box.max.z+m) ||
      eye.y<=this.heightAt(eye.x,eye.z)+m;
  }
  test(eye,box) {
    if(this.safe(eye,box)) return false;
    for(const x of [box.min.x,(box.min.x+box.max.x)/2,box.max.x]) for(const z of [box.min.z,(box.min.z+box.max.z)/2,box.max.z]) {
      const distance=Math.hypot(x-eye.x,z-eye.z), skip=Math.max(0.02,this.margin/Math.max(1,distance));
      let blocked=false;
      for(let i=1;i<=this.steps;i++) {
        const t=i/(this.steps+1); if(t<skip || t>1-skip) continue;
        const h=this.heightAt(eye.x+(x-eye.x)*t,eye.z+(z-eye.z)*t);
        if(Number.isFinite(h) && h>eye.y+(box.max.y-eye.y)*t+this.margin) {blocked=true;break;}
      }
      if(!blocked) return false;
    }
    return true;
  }
  occluded(key,eye,box) {
    let item=this.cache.get(key);
    if(!item) {item={streak:0,hidden:false};this.cache.set(key,item);}
    item.frame=this.frame;
    if(this.safe(eye,box)) {item.streak=0;item.hidden=false;return false;}
    if(item.testFrame!==this.frame && this.tests<this.budget && (this.allowed?.has(key) || this.cache.size<=this.budget)) {
      this.tests++; item.testFrame=this.frame;
      item.streak=this.test(eye,box) ? item.streak+1 : 0; item.hidden=item.streak>=2;
    }
    return item.hidden;
  }
}
