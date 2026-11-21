import * as T from 'three';
import { mergeGeometries } from './vendor/addons/utils/BufferGeometryUtils.js';
import { foliagePlacements, foliageSettings, foliageExclusion, bakeWaterDepth, junglePlacements, jungleSettings } from './nature-settings.js';

function merge(parts) {
  const geometry=mergeGeometries(parts);
  for (const part of parts) part.dispose();
  return geometry;
}

function vegetationGeometry() {
  const blades=[], crowns=[], branches=[];
  for(let i=0;i<9;i++) {
    const height=0.22+(i%3)*0.055, blade=new T.PlaneGeometry(0.035,height,1,2);
    blade.translate(0,height/2,0);
    const p=blade.attributes.position;
    for(let j=0;j<p.count;j++){const y=p.getY(j);p.setX(j,p.getX(j)*Math.max(0,1-y/height)+y*y*0.28);}
    blade.rotateY(i*2.4);blade.translate(Math.sin(i*2.4)*0.12,0,Math.cos(i*2.4)*0.12);blade.computeVertexNormals();blades.push(blade);
  }
  const trunk=new T.CylinderGeometry(0.13,0.32,4.5,8);trunk.translate(0,2.25,0);branches.push(trunk);
  for(let i=0;i<11;i++) {
    const angle=i*2.4, x=Math.sin(angle)*1.65,z=Math.cos(angle)*1.65,y=4.7+(i%3)*0.5;
    const crown=new T.IcosahedronGeometry(1.1,1);crown.scale(1.1,0.52,0.8);crown.rotateY(angle);crown.translate(x,y,z);crowns.push(crown);
    if(i<5){const end=new T.Vector3(x,y-0.2,z),start=new T.Vector3(0,3,0),direction=end.clone().sub(start);const branch=new T.CylinderGeometry(0.045,0.12,direction.length(),5);branch.applyQuaternion(new T.Quaternion().setFromUnitVectors(new T.Vector3(0,1,0),direction.normalize()));branch.translate(...start.add(end).multiplyScalar(0.5).toArray());branches.push(branch);}
  }
  const ferns=[],flowers=[],cards=[];
  for(let i=0;i<7;i++) {
    for(let j=0;j<3;j++) {
      const leaf=new T.PlaneGeometry(0.12,0.24);
      const p=leaf.attributes.position;for(let k=0;k<p.count;k++)p.setX(k,p.getX(k)*(p.getY(k)>0?0.12:1));
      leaf.rotateX(-0.6-j*0.22);leaf.translate(0,0.15+j*0.055,0.09+j*0.09);leaf.rotateY(i*2.4);ferns.push(leaf);
    }
    const petal=new T.CircleGeometry(0.035,5);petal.rotateX(-Math.PI/2);petal.translate(Math.sin(i)*0.05,0.24,Math.cos(i)*0.05);flowers.push(petal);
  }
  const stem=new T.CylinderGeometry(0.005,0.008,0.24,3);stem.translate(0,0.12,0);flowers.push(stem);
  for(let i=0;i<4;i++) {
    const card=new T.BufferGeometry();card.setAttribute('position',new T.Float32BufferAttribute([-0.16,0,0,0.16,0,0,0.05,0.32,0],3));card.rotateY(i*2.4);card.computeVertexNormals();cards.push(card);
  }
  const geometries=[merge(blades),merge(crowns),merge(branches),merge(ferns),merge(flowers),merge(cards)];
  for(const index of [0,3,4,5]) {
    const geometry=geometries[index],p=geometry.attributes.position,colors=[];
    for(let i=0;i<p.count;i++) {
      const t=T.MathUtils.clamp(p.getY(i)/0.35,0,1);
      const color=index===4&&t>0.6?new T.Color(0xffd78b):new T.Color().lerpColors(new T.Color(0x344d16),new T.Color(0x92bd4e),t);
      colors.push(color.r,color.g,color.b);
    }
    geometry.setAttribute('color',new T.Float32BufferAttribute(colors,3));
  }
  return geometries;
}

function canopyGeometry(palm) {
  const tint = (geometry, hex) => {
    const color=new T.Color(hex), data=new Float32Array(geometry.attributes.position.count*3);
    for(let i=0;i<data.length;i+=3) color.toArray(data,i);
    geometry.setAttribute('color',new T.BufferAttribute(data,3));return geometry;
  };
  const height=palm?8:5.7;
  const stem=new T.CylinderGeometry(.14,.34,height,5,1,true);
  stem.translate(0,height/2,0);
  const parts=[tint(stem.toNonIndexed(),0x76583e)];stem.dispose();
  if(palm) {
    const leaf=[0,.1,0,-.56,.56,1.3,0,.65,1.3,0,.65,1.3,-.56,.56,1.3,0,-.45,3.2,
      0,.1,0,0,.65,1.3,.56,.56,1.3,0,.65,1.3,0,-.45,3.2,.56,.56,1.3];
    for(let i=0;i<8;i++) {
      const geometry=new T.BufferGeometry();
      geometry.setAttribute('position',new T.Float32BufferAttribute(leaf,3));
      geometry.setAttribute('uv',new T.Float32BufferAttribute(new Float32Array(24),2));
      geometry.rotateY(i*2.4);geometry.translate(0,height-i*.034,0);geometry.computeVertexNormals();
      parts.push(tint(geometry,i%2===0?0x4d8736:0x326f30));
    }
  } else {
    for(const [x,y,z,rx,ry,rz,color] of [
      [-1.5,5.9,.1,3.05,1.35,2.1,0x3d7537],
      [1.4,6.3,.58,2.5,1.65,2.3,0x2e6938],
      [.4,7.32,-1.05,2.6,1.22,2.3,0x4c8137],
    ]) {
      const crown=new T.IcosahedronGeometry(1);
      crown.rotateY(x);crown.scale(rx,ry,rz);crown.translate(x,y,z);parts.push(tint(crown,color));
    }
  }
  return merge(parts);
}

export class NatureVisuals {
  constructor(scene,config,level) {
    Object.assign(this,{scene,config,tiles:new Map(),time:{value:0},viewer:{value:new T.Vector3()},fade:{value:64}});
    this.root=new T.Group();scene.add(this.root);
    this.geometries=vegetationGeometry();
    this.materials=[this.material(0xffffff,false),this.material(0x438b38,true),this.material(0x796047,true),this.material(0xffffff,false),this.material(0xffffff,false),this.material(0xffffff,false)];
    for(const i of [0,3,4,5])this.materials[i].vertexColors=true;
    this.depths=this.materials.map(m=>{const depth=new T.MeshDepthMaterial({depthPacking:T.RGBADepthPacking,side:T.DoubleSide});depth.onBeforeCompile=m.onBeforeCompile;return depth;});
    if(config.foliage?.jungle) {
      this.canopyGeometries=[canopyGeometry(false),canopyGeometry(true)];
      this.canopyFade={value:0};
      this.canopyFrustum=new T.Frustum();
      this.canopyView=new T.Matrix4();
      const shadeCanopy=shader=>{
        Object.assign(shader.uniforms,{canopyViewer:this.viewer,canopyFade:this.canopyFade});
        shader.vertexShader='uniform vec3 canopyViewer; uniform float canopyFade;\n'+shader.vertexShader;
        shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>',`#include <begin_vertex>
          vec3 base=(modelMatrix*instanceMatrix*vec4(0.,0.,0.,1.)).xyz;
          float d=distance(base.xz,canopyViewer.xz);
          transformed*=smoothstep(20.,39.,d)*(1.-smoothstep(canopyFade-29.,canopyFade-10.,d));`);
      };
      this.canopyMaterials=[T.FrontSide,T.DoubleSide].map(side=>{
        const material=new T.MeshLambertMaterial({vertexColors:true,side});
        material.onBeforeCompile=shadeCanopy;
        return material;
      });
      const fern=this.materials[3];
      this.materials[3]=new T.MeshLambertMaterial({vertexColors:true,side:T.DoubleSide});
      this.materials[3].onBeforeCompile=fern.onBeforeCompile;
      fern.dispose();
    }
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
    const matrix=hit.object.matrixWorld.clone();
    if(hit.instanceId!==undefined&&hit.object.isInstancedMesh){const instance=new T.Matrix4();hit.object.getMatrixAt(hit.instanceId,instance);matrix.multiply(instance);}
    return hit.face?.normal.clone().transformDirection(matrix).y >= 0.7 ? hit.point.y : NaN;
  }
  setHeightSampler(sample, canopySample=this.sample, canopyVersion=()=>0) {
    if(typeof sample!=='function') throw new TypeError('Height sampler must be a function');
    this.sampler=sample;this.customSampler=true;this.waterRefresh=-Infinity;
    this.canopySample=canopySample;this.canopyVersion=canopyVersion;
    this.clearTiles();
  }
  setExclusions(entities) {
    this.exclude=foliageExclusion(entities);
    if(this.canopyMaterials)this.canopyExclude=foliageExclusion(entities,true);
    this.clearTiles();
  }
  createWater() {
    const data=new Uint8Array(64*64*4);
    for(let y=0;y<64;y++) for(let x=0;x<64;x++){const i=(y*64+x)*4;data[i]=128+Math.sin(x*0.7+y*0.3)*30;data[i+1]=128+Math.cos(y*0.8+x*0.2)*30;data[i+2]=250;data[i+3]=255;}
    this.normal=new T.DataTexture(data,64,64);this.normal.wrapS=this.normal.wrapT=T.RepeatWrapping;this.normal.needsUpdate=true;
    this.waterBounds={value:new T.Vector3(0,0,2000)};
    this.waterDepth={value:null};this.waterLayers={value:1};
    const material=new T.MeshPhysicalMaterial({color:this.config.water.color,metalness:0.12,roughness:0.22,normalMap:this.normal,normalScale:new T.Vector2(0.4,0.4),transparent:true,opacity:0.96,depthWrite:false,clearcoat:1,clearcoatRoughness:0.16});
    material.onBeforeCompile=shader=>{
      Object.assign(shader.uniforms,{waterTime:this.time,waterBounds:this.waterBounds,waterDepth:this.waterDepth,waterLayers:this.waterLayers});
      shader.vertexShader='varying vec2 vWaterWorld;\n'+shader.vertexShader;
      shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>','#include <begin_vertex>\nvWaterWorld = (modelMatrix * vec4(position, 1.0)).xz;');
      shader.fragmentShader='uniform float waterTime; uniform float waterLayers; uniform vec3 waterBounds; uniform sampler2D waterDepth; varying vec2 vWaterWorld;\n'+shader.fragmentShader;
      shader.fragmentShader=shader.fragmentShader.replace('#include <color_fragment>',`#include <color_fragment>
        vec2 shoreUv = (vWaterWorld - waterBounds.xy) / waterBounds.z + 0.5;
        if (any(lessThan(shoreUv,vec2(0.0))) || any(greaterThan(shoreUv,vec2(1.0)))) discard;
        vec2 packedDepth = texture2D(waterDepth, shoreUv).rg;
        float depth = dot(packedDepth,vec2(65280.0,255.0)) * (64.0/65535.0);
        if (depth < 0.02) discard;
        diffuseColor.rgb *= mix(vec3(1.35,1.65,1.2),vec3(0.32,0.65,0.72),smoothstep(0.0,7.0,depth));
        diffuseColor.a *= smoothstep(0.02,0.8,depth);`);
      const waves=this.level>0?`mix(texture2D(normalMap,vNormalMapUv + waterTime*vec2(0.007,0.003)),
        texture2D(normalMap,vNormalMapUv*1.73 + waterTime*vec2(-0.005,0.009)),waterLayers*0.45)`:'texture2D(normalMap,vNormalMapUv + waterTime*vec2(0.007,0.003))';
      shader.fragmentShader=shader.fragmentShader.replace('#include <normal_fragment_maps>',T.ShaderChunk.normal_fragment_maps.replaceAll('texture2D( normalMap, vNormalMapUv )',waves));
    };
    material.customProgramCacheKey=()=>`water-${this.level}`;
    this.water=new T.Mesh(new T.PlaneGeometry(2000,2000),material);
    this.water.rotation.x=-Math.PI/2;this.water.position.y=this.config.water.level;this.root.add(this.water);
  }
  setQuality(level) {
    this.level=level;this.settings=foliageSettings(this.config.foliage,level);this.fade.value=this.settings.distance;
    if(this.water){
      this.normal.repeat.setScalar([24,48,80][level]);this.water.material.normalScale.setScalar([0.18,0.3,0.4][level]);
      this.waterLayers.value=level>0?1:0;this.water.material.clearcoat=level>0?1:0;
      this.water.material.needsUpdate=true;
      const n=[32,64,128][level];this.depthTexture?.dispose();
      this.depthTexture=new T.DataTexture(new Uint8Array(n*n*4),n,n);
      this.depthTexture.minFilter=this.depthTexture.magFilter=T.LinearFilter;
      this.waterDepth.value=this.depthTexture;this.waterRefresh=-Infinity;
    }
    if(this.canopyMaterials) {
      this.canopySettings=jungleSettings(level);
      this.canopyFade.value=this.canopySettings.radius;
      for(const mesh of this.canopyMeshes||[]){mesh.removeFromParent();mesh.dispose();}
      this.canopyMeshes=this.canopyGeometries.slice(0,1+this.canopySettings.palm).flatMap((geometry,i)=>Array.from({length:4},()=>{
        const mesh=new T.InstancedMesh(geometry,this.canopyMaterials[i],this.canopySettings.budget);
        mesh.count=0;mesh.frustumCulled=false;
        mesh.instanceMatrix.setUsage(T.DynamicDrawUsage);this.root.add(mesh);return mesh;
      }));
    }
    this.clearTiles();
  }
  tile(x,z,far=false) {
    const placements=foliagePlacements(x,z,{density:this.settings.density*(far?0.22:1),trees:this.settings.trees,sample:this.sample,exclude:this.exclude,water:this.water?.position.y ?? -Infinity});
    const trees=placements.filter(p=>p.tree),group=new T.Group(),dummy=new T.Object3D();group.userData.far=far;
    const fernLimit=this.canopyMaterials?[.11,.17,.22][this.level]:.08;
    for(const type of far?[5]:[0,1,2,3,4]) {
      const items=type===1||type===2?trees:type===3?placements.filter(p=>p.kind<fernLimit):type===4?placements.filter(p=>p.kind>0.985):type===0?placements.filter(p=>p.kind>=fernLimit&&p.kind<=0.985):placements;
      if(!items.length)continue;
      const mesh=new T.InstancedMesh(this.geometries[type],this.materials[type],items.length);
      const jungleFern=type===3&&this.canopyMaterials, color=new T.Color();
      items.forEach((p,i)=>{
        const width=p.scale*(far?1.8:jungleFern?1.6+p.hue*1.7:1);
        const height=p.scale*(jungleFern?1.1+p.hue*1.4:.8+p.hue*.4);
        dummy.position.set(p.x,p.y,p.z);dummy.rotation.set(0,p.rotation,0);
        dummy.scale.set(width,height,width);
        dummy.updateMatrix();mesh.setMatrixAt(i,dummy.matrix);
        mesh.setColorAt(i,color.setHSL(type===2?.08:.20+p.hue*.10,.12+p.hue*.18,.62+p.hue*.25));
      });
      mesh.castShadow=type===1||type===2;mesh.receiveShadow=true;mesh.customDepthMaterial=this.depths[type];group.add(mesh);
    }
    this.root.add(group);this.tiles.set(`${x},${z}`,group);
  }
  removeTile(key,group) {group.removeFromParent();group.traverse(o=>{if(o.isInstancedMesh)o.dispose();});this.tiles.delete(key);}
  clearTiles() {
    for(const [key,group] of this.tiles)this.removeTile(key,group);
    this.canopyKey=null;
    for(const mesh of this.canopyMeshes||[])mesh.count=0;
  }
  updateCanopy(camera) {
    if(!this.canopyMeshes||!this.canopySample)return;
    const x=Math.round(camera.position.x/8)*8,z=Math.round(camera.position.z/8)*8,key=`${x},${z},${this.canopyVersion()}`;
    if(key!==this.canopyKey) this.rebuildCanopy(x,z,key);
    this.canopyFrustum.setFromProjectionMatrix(this.canopyView.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
    for(const mesh of this.canopyMeshes) mesh.visible=mesh.count>0&&this.canopyFrustum.intersectsBox(mesh.boundingBox);
  }
  rebuildCanopy(x,z,key) {
    this.canopyKey=key;
    const placements=junglePlacements(x,z,{...this.canopySettings,sample:this.canopySample,exclude:this.canopyExclude,water:this.config.water?.level});
    const dummy=new T.Object3D(),color=new T.Color();
    for(const mesh of this.canopyMeshes)mesh.count=0;
    for(const p of placements) {
      const quadrant=2*Number(p.x>=x)+Number(p.z>=z);
      const mesh=this.canopyMeshes[4*p.variant+quadrant],wide=p.scale*(1.04+p.hue*.48);
      dummy.position.set(p.x,p.y-.12,p.z);dummy.rotation.y=p.rotation;
      dummy.scale.set(wide,p.scale*(1.22-p.hue*.34),wide);dummy.updateMatrix();
      mesh.setMatrixAt(mesh.count,dummy.matrix);
      mesh.setColorAt(mesh.count++,color.setRGB(.79+p.hue*.19,.79+p.hue*.2,.76+p.hue*.24));
    }
    for(const mesh of this.canopyMeshes) if(mesh.count) {
      mesh.instanceMatrix.needsUpdate=true;mesh.instanceColor.needsUpdate=true;mesh.computeBoundingBox();
    }
  }
  update(elapsed,camera) {
    this.time.value+=elapsed;this.viewer.value.copy(camera.position);
    if(this.water){
      const x=Math.floor(camera.position.x/32)*32,z=Math.floor(camera.position.z/32)*32;
      if(x!==this.water.position.x||z!==this.water.position.z||this.time.value-this.waterRefresh>1){
        this.water.position.x=x;this.water.position.z=z;this.waterBounds.value.set(x,z,2000);
        this.collectGrounds();const image=this.depthTexture.image;
        bakeWaterDepth(this.sample,this.config.water.level,x,z,2000,image.width,image.data);
        this.depthTexture.needsUpdate=true;this.waterRefresh=this.time.value;
      }
    }
    if(!this.config.foliage || this.settings.density===0) return;
    this.updateCanopy(camera);
    const cx=Math.floor(camera.position.x/16),cz=Math.floor(camera.position.z/16),r=Math.ceil(this.settings.distance/16);
    for(const [key,group] of this.tiles){const [x,z]=key.split(',').map(Number),d=Math.hypot(x-cx,z-cz);if(d>r||group.userData.far!==(d>2.5))this.removeTile(key,group);}
    for(let d=0;d<=r;d++)for(let x=cx-d;x<=cx+d;x++)for(let z=cz-d;z<=cz+d;z++)if(Math.hypot(x-cx,z-cz)<=r&&!this.tiles.has(`${x},${z}`)){
      if(!this.customSampler)this.collectGrounds();this.tile(x,z,Math.hypot(x-cx,z-cz)>2.5);return;
    }
  }
  collectGrounds() {this.grounds=[];this.scene.updateMatrixWorld(true);this.scene.traverse(o=>{if(o.userData.support)this.grounds.push(o);});}
  stats() {return {foliageTiles:this.tiles.size,canopyInstances:this.canopyMeshes?.reduce((total,mesh)=>total+mesh.count,0)??0,quality:['low','balanced','ultra'][this.level]};}
  dispose() {
    this.clearTiles();this.root.removeFromParent();
    for(const mesh of this.canopyMeshes||[])mesh.dispose();
    for(const geometry of this.canopyGeometries||[])geometry.dispose();
    for(const material of this.canopyMaterials||[])material.dispose();
    for(const resource of [...this.geometries,...this.materials,...this.depths,this.water?.geometry,this.water?.material,this.normal,this.depthTexture])resource?.dispose();
    if(window.__aiEngineVisuals===this)delete window.__aiEngineVisuals;
  }
}
