const smooth = (a,b,x) => {const t=Math.max(0,Math.min(1,(x-a)/(b-a)));return t*t*(3-2*t);};

export function terrainWeights(height, slope, water = 0.6, noise = 0.5) {
  if (![height,slope,water,noise].every(Number.isFinite)) return [1,0,0,0];
  const rock=smooth(0.18,0.55,slope), mud=(1-smooth(water+0.4,water+5,height+(noise-0.5)*2))*(1-rock);
  const dirt=smooth(0.06,0.28,slope+(noise-0.5)*0.15)*(1-rock-mud);
  return [1-rock-mud-dirt,dirt,rock,mud];
}

export function surfacePixels(size = 64) {
  const albedo=new Uint8Array(size*size*16),normal=new Uint8Array(albedo.length),width=size*2;
  const palettes=[[0.18,0.29,0.065],[0.29,0.17,0.075],[0.33,0.32,0.27],[0.29,0.255,0.15]];
  const noise=(x,y,seed)=>{let h=Math.imul(x+seed,374761393)^Math.imul(y-seed,668265263);h=Math.imul(h^(h>>>13),1274126177);return ((h^(h>>>16))>>>0)/4294967295;};
  const patchNoise=(x,y,cells,seed)=>{
    const u=x/size*cells,v=y/size*cells,i=Math.floor(u),j=Math.floor(v),a=smooth(0,1,u-i),b=smooth(0,1,v-j);
    const h=(dx,dy)=>noise((i+dx)%cells,(j+dy)%cells,seed);
    return (h(0,0)*(1-a)+h(1,0)*a)*(1-b)+(h(0,1)*(1-a)+h(1,1)*a)*b;
  };
  for(let layer=0;layer<4;layer++) {
    const heights=new Float32Array(size*size);
    for(let y=0;y<size;y++)for(let x=0;x<size;x++) {
      const patch=patchNoise(x,y,7,layer+31)*0.7+patchNoise(x,y,17,layer+43)*0.3,grain=noise(x,y,layer+17);
      heights[y*size+x]=layer===0?grain*0.08+noise(x,Math.floor(y/3),73)*0.16:layer===2?patch*0.8+grain*0.1:patch*0.3+grain*0.12;
      const offset=((y+Math.floor(layer/2)*size)*width+x+(layer%2)*size)*4;
      const variation=0.65+patch*0.5+grain*0.3;
      for(let c=0;c<3;c++)albedo[offset+c]=Math.round(Math.min(1,palettes[layer][c]*variation)*255);
      if(layer===0)albedo[offset+1]+=Math.round(patch*14);
      albedo[offset+3]=255;
    }
    const height=(x,y)=>heights[((y+size)%size)*size+(x+size)%size];
    for(let y=0;y<size;y++)for(let x=0;x<size;x++) {
      const dx=(height(x-1,y)-height(x+1,y))*1.6,dy=(height(x,y-1)-height(x,y+1))*1.6,len=Math.hypot(dx,dy,1);
      const offset=((y+Math.floor(layer/2)*size)*width+x+(layer%2)*size)*4;
      normal[offset]=Math.round((dx/len*0.5+0.5)*255);normal[offset+1]=Math.round((dy/len*0.5+0.5)*255);normal[offset+2]=Math.round((1/len*0.5+0.5)*255);normal[offset+3]=255;
    }
  }
  return {albedo,normal,width};
}

const textures=new WeakMap();
export function surfaceTextures(T) {
  if(textures.has(T))return textures.get(T);
  const pixels=surfacePixels(),make=data=>{
    const texture=new T.DataTexture(data,pixels.width,pixels.width);
    texture.wrapS=texture.wrapT=T.RepeatWrapping;texture.magFilter=T.LinearFilter;texture.minFilter=T.LinearMipmapLinearFilter;
    texture.generateMipmaps=true;texture.needsUpdate=true;return texture;
  };
  const value={albedo:make(pixels.albedo),normal:make(pixels.normal)};textures.set(T,value);return value;
}

const surfaceGLSL=`
uniform sampler2D surfaceAlbedo;
uniform sampler2D surfaceNormal;
uniform float surfaceDetail;
float surfaceNoise(vec2 p) {
  vec2 i=floor(p),f=fract(p); f=f*f*(3.0-2.0*f);
  vec4 h=fract(sin(vec4(dot(i,vec2(127.1,311.7)),dot(i+vec2(1,0),vec2(127.1,311.7)),dot(i+vec2(0,1),vec2(127.1,311.7)),dot(i+vec2(1),vec2(127.1,311.7))))*43758.5453);
  return mix(mix(h.x,h.y,f.x),mix(h.z,h.w,f.x),f.y);
}
vec2 layerUv(vec2 uv,vec2 tile) {return (tile+mix(vec2(0.5/64.0),vec2(63.5/64.0),fract(uv)))*0.5;}
vec3 layerColor(vec2 uv,vec2 tile) {return texture2D(surfaceAlbedo,layerUv(uv,tile)).rgb;}
vec2 layerNormal(vec2 uv,vec2 tile) {return texture2D(surfaceNormal,layerUv(uv,tile)).rg*2.0-1.0;}
`;

export function terrainSurface(T, material, water = 0.6) {
  const maps=surfaceTextures(T),detail={value:0};
  material.onBeforeRender=renderer=>{detail.value=Number(renderer.domElement.dataset.quality)>0?1:0;};
  material.onBeforeCompile=shader=>{
    Object.assign(shader.uniforms,{surfaceAlbedo:{value:maps.albedo},surfaceNormal:{value:maps.normal},surfaceDetail:detail,surfaceWater:{value:water}});
    shader.vertexShader='varying vec3 vSurfaceWorld; varying vec3 vSurfaceNormal;\n'+shader.vertexShader;
    shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>','#include <begin_vertex>\nvSurfaceWorld=(modelMatrix*vec4(position,1.0)).xyz; vSurfaceNormal=normalize(mat3(modelMatrix)*normal);');
    shader.fragmentShader=surfaceGLSL+'uniform float surfaceWater; varying vec3 vSurfaceWorld; varying vec3 vSurfaceNormal;\n'+shader.fragmentShader;
    shader.fragmentShader=shader.fragmentShader.replace('#include <color_fragment>',`#include <color_fragment>
      vec3 p=vSurfaceWorld,n=normalize(vSurfaceNormal);
      float slope=1.0-abs(n.y), breakup=surfaceDetail>0.5?surfaceNoise(p.xz*0.12):0.5, macro=surfaceNoise(p.xz*0.01);
      float rock=smoothstep(0.18,0.55,slope);
      float mud=(1.0-smoothstep(surfaceWater+0.4,surfaceWater+5.0,p.y+(breakup-0.5)*2.0))*(1.0-rock);
      float dirt=smoothstep(0.06,0.28,slope+(breakup-0.5)*0.15)*(1.0-rock-mud);
      vec4 weights=vec4(1.0-rock-mud-dirt,dirt,rock,mud);
      vec3 base=vec3(0.18,0.29,0.065)*weights.x+vec3(0.29,0.17,0.075)*weights.y+vec3(0.33,0.32,0.27)*weights.z+vec3(0.29,0.255,0.15)*weights.w;
      float surfaceFade=1.0-smoothstep(35.0,180.0,length(vViewPosition));
      vec3 surfacePerturb=vec3(0.0);
      if(surfaceDetail>0.5 && surfaceFade>0.01) {
        vec3 axes=pow(abs(n),vec3(4.0));axes/=max(dot(axes,vec3(1.0)),0.001);
        vec2 uv=p.xz*0.45+vec2(breakup,macro)*0.7;
        vec3 stone=layerColor(p.zy*0.22,vec2(0,1))*axes.x+layerColor(p.xz*0.22,vec2(0,1))*axes.y+layerColor(p.xy*0.22,vec2(0,1))*axes.z;
        vec3 detailed=layerColor(uv,vec2(0))*weights.x+layerColor(uv*0.7,vec2(1,0))*weights.y+stone*weights.z+layerColor(uv*0.5,vec2(1))*weights.w;
        base=mix(base,detailed,surfaceFade);
        vec2 groundN=layerNormal(uv,vec2(0))*weights.x+layerNormal(uv*0.7,vec2(1,0))*weights.y+layerNormal(uv*0.5,vec2(1))*weights.w;
        vec2 nx=layerNormal(p.zy*0.22,vec2(0,1)),ny=layerNormal(p.xz*0.22,vec2(0,1)),nz=layerNormal(p.xy*0.22,vec2(0,1));
        surfacePerturb=vec3(groundN.x,0,groundN.y)+(vec3(0,nx.y,nx.x)*axes.x+vec3(ny.x,0,ny.y)*axes.y+vec3(nz.x,nz.y,0)*axes.z)*weights.z;
      }
      diffuseColor.rgb=base*mix(0.77,1.2,macro)*(0.93+breakup*0.14);`);
    shader.fragmentShader=shader.fragmentShader.replace('#include <normal_fragment_maps>','#include <normal_fragment_maps>\nnormal=normalize(normal+mat3(viewMatrix)*surfacePerturb*surfaceFade*0.45);');
  };
}

export function roadSurface(T, material) {
  const maps=surfaceTextures(T),detail={value:0};
  material.roughness=1;material.metalness=0;
  material.onBeforeRender=renderer=>{detail.value=Number(renderer.domElement.dataset.quality)>0?1:0;};
  material.onBeforeCompile=shader=>{
    Object.assign(shader.uniforms,{surfaceAlbedo:{value:maps.albedo},surfaceNormal:{value:maps.normal},surfaceDetail:detail});
    shader.vertexShader='varying vec3 vTrackLocal; varying vec3 vTrackWorld;\n'+shader.vertexShader;
    shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>',`#include <begin_vertex>
      vTrackLocal=position;
      vec4 trackWorld=vec4(position,1.0);
      #ifdef USE_INSTANCING
        trackWorld=instanceMatrix*trackWorld;
      #endif
      vTrackWorld=(modelMatrix*trackWorld).xyz;`);
    shader.fragmentShader=surfaceGLSL+'varying vec3 vTrackLocal; varying vec3 vTrackWorld;\n'+shader.fragmentShader;
    shader.fragmentShader=shader.fragmentShader.replace('#include <color_fragment>',`#include <color_fragment>
      float edgeNoise=surfaceNoise(vTrackWorld.xz*1.2),macro=surfaceNoise(vTrackWorld.xz*0.01);
      float edge=smoothstep(2.15,3.25,abs(vTrackLocal.x)+(edgeNoise-0.5)*0.8);
      float rut=1.0-smoothstep(0.18,0.5,abs(abs(vTrackLocal.x)-1.05));
      vec3 track=mix(vec3(0.30,0.19,0.09),vec3(0.17,0.105,0.045),rut*0.65);
      if(surfaceDetail>0.5)track*=layerColor(vTrackWorld.xz*0.4,vec2(1,0))*3.8;
      diffuseColor.rgb=mix(track,vec3(0.18,0.29,0.065),edge)*mix(0.77,1.2,macro)*(0.87+edgeNoise*0.26);`);
  };
}
