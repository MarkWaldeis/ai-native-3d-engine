import * as THREE from 'three';

export function cloneSkinned(source,clips=source.animations) {
  const clone = source.clone(true), copies = new Map();
  function pair(a,b) { copies.set(a,b); a.children.forEach((child,i)=>pair(child,b.children[i])); }
  pair(source,clone);
  source.traverse(mesh=>{
    if (!mesh.isSkinnedMesh) return;
    const copy=copies.get(mesh), skeleton=mesh.skeleton.clone();
    skeleton.bones=mesh.skeleton.bones.map(bone=>{
      const mapped=copies.get(bone);
      if(!mapped)throw Error('Skinned mesh references a bone outside its GLB scene');
      return mapped;
    });
    copy.bind(skeleton,mesh.bindMatrix);
  });
  const uuids=new Map([...copies].map(([source,copy])=>[source.uuid,copy.uuid]));
  clone.animations=clips.map(clip=>{
    const copy=clip.clone();
    for(const track of copy.tracks){
      const dot=track.name.indexOf('.'),target=uuids.get(track.name.slice(0,dot));
      if(target)track.name=target+track.name.slice(dot);
    }
    return copy;
  });
  return clone;
}

export class Animations {
  constructor() { this.objects=new Map();this.states=new Map(); }
  add(entity,root,clips) {
    if(!entity.animator)return;
    this.remove(entity.id);
    const mixer=new THREE.AnimationMixer(root),actions=new Map();
    for(const [name,state] of Object.entries(entity.animator.machine.states)) {
      const clip=clips.find(clip=>clip.name===state.clip);
      if(!clip)throw Error(`Missing animation clip ${state.clip}`);
      const action=mixer.clipAction(new THREE.AnimationClip(name,clip.duration,clip.tracks,clip.blendMode));
      action.setLoop(THREE.LoopOnce,1);action.clampWhenFinished=true;
      action.play();action.setEffectiveWeight(0);action.setEffectiveTimeScale(0);
      actions.set(name,action);
    }
    this.objects.set(entity.id,{root,mixer,actions});
    this.sample(entity.id);
  }
  apply(updates=[]) {
    for(const {id,state} of updates){this.states.set(id,state);this.sample(id);}
  }
  sample(id) {
    const item=this.objects.get(id),state=this.states.get(id);
    if(!item||!state)return;
    const weight=state.previous_state&&state.blend_time>0?Math.min(1,state.blend_elapsed/state.blend_time):1;
    for(const [name,action] of item.actions) {
      action.enabled=true;
      action.time=name===state.current_state?state.time:state.previous_time;
      action.setEffectiveWeight(name===state.current_state?weight:name===state.previous_state?1-weight:0);
    }
    item.mixer.update(0);
    item.root.updateMatrixWorld(true);
  }
  inspect(id) {
    const item=this.objects.get(id);
    return {state:this.states.get(id),actions:item?[...item.actions].map(([name,a])=>({name,time:a.time,weight:a.getEffectiveWeight()})):[]};
  }
  remove(id) {
    const item=this.objects.get(id);if(!item)return;
    item.mixer.stopAllAction();item.mixer.uncacheRoot(item.root);
    item.root.traverse(o=>{if(o.isSkinnedMesh)o.skeleton.dispose();});
    this.objects.delete(id);
  }
  clear() { for(const id of this.objects.keys())this.remove(id);this.states.clear(); }
}
