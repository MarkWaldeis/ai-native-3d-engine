import * as T from 'three';

export function valleyDensity(height, base, falloff) {
  return Number.isFinite(height) ? Math.exp(-Math.max(0,height-base)*falloff) : 0;
}

const cloudNoise=`
float cloudNoise(vec2 p) {
  vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
  vec4 h=fract(sin(vec4(dot(i,vec2(127.1,311.7)),dot(i+vec2(1,0),vec2(127.1,311.7)),dot(i+vec2(0,1),vec2(127.1,311.7)),dot(i+vec2(1),vec2(127.1,311.7))))*43758.5453);
  return mix(mix(h.x,h.y,f.x),mix(h.z,h.w,f.x),f.y);
}`;

export class Atmosphere {
  constructor(scene,config,level) {
    Object.assign(this,{scene,config,patched:new WeakSet(),time:0,lastScan:-Infinity});
    this.uniforms={hazeLevel:{value:level},hazeDensity:{value:config.haze?.density??0},hazeBase:{value:config.haze?.height??18},hazeFalloff:{value:config.haze?.falloff??0.045},hazeHorizon:{value:new T.Color(config.horizon)},hazeSun:{value:new T.Color(config.sunColor)},hazeDirection:{value:new T.Vector3(...config.sunDirection)}};
    if(config.clouds) {
      const material=new T.ShaderMaterial({side:T.BackSide,transparent:true,depthWrite:false,fog:false,
        uniforms:{...this.uniforms,cloudTime:{value:0},cloudCoverage:{value:config.clouds.coverage}},
        vertexShader:`varying vec3 vCloudDirection;
          void main(){vCloudDirection=position;vec4 p=projectionMatrix*mat4(mat3(viewMatrix))*vec4(position,1.0);gl_Position=p.xyww;}`,
        fragmentShader:`varying vec3 vCloudDirection;uniform float cloudTime;uniform float cloudCoverage;
          uniform float hazeLevel;uniform vec3 hazeHorizon;uniform vec3 hazeSun;uniform vec3 hazeDirection;
          ${cloudNoise}
          void main(){
            vec3 d=normalize(vCloudDirection);if(d.y<0.02)discard;
            vec2 p=d.xz/max(d.y,0.08)*2.5+vec2(cloudTime*0.004,cloudTime*0.0015);
            float n=0.0,a=0.56;
            for(int i=0;i<4;i++){n+=a*cloudNoise(p);p=mat2(1.6,-1.2,1.2,1.6)*p+vec2(7.0,13.0);a*=0.5;if(i==1&&hazeLevel<1.5)break;}
            float cloud=smoothstep(0.78-cloudCoverage*0.65,0.97-cloudCoverage*0.65,n);
            float light=pow(max(dot(d,hazeDirection),0.0),8.0);
            vec3 color=mix(hazeHorizon*0.85,vec3(1.15),smoothstep(0.3,0.9,n))+hazeSun*light*0.15;
            gl_FragColor=vec4(color,cloud*smoothstep(0.02,0.22,d.y)*0.85);
            #include <tonemapping_fragment>
            #include <colorspace_fragment>
          }`});
      this.clouds=new T.Mesh(new T.SphereGeometry(1,32,16),material);
      this.clouds.frustumCulled=false;this.clouds.renderOrder=-10000;scene.add(this.clouds);
    }
    this.setQuality(level);
  }
  setQuality(level) {this.uniforms.hazeLevel.value=level;if(this.clouds)this.clouds.visible=level>0;}
  patch(material) {
    if(!material.fog||material.isShaderMaterial||this.patched.has(material))return;
    this.patched.add(material);
    const compile=material.onBeforeCompile,key=material.customProgramCacheKey;
    material.customProgramCacheKey=()=>(key===T.Material.prototype.customProgramCacheKey?compile.toString():key.call(material))+'-valley-haze-v1';
    material.onBeforeCompile=shader=>{
      compile.call(material,shader);Object.assign(shader.uniforms,this.uniforms);
      shader.vertexShader='varying vec3 vHazeWorld;\n'+shader.vertexShader;
      shader.vertexShader=shader.vertexShader.replace('#include <fog_vertex>',`#include <fog_vertex>
        vec4 hazePosition=vec4(transformed,1.0);
        #ifdef USE_INSTANCING
          hazePosition=instanceMatrix*hazePosition;
        #endif
        vHazeWorld=(modelMatrix*hazePosition).xyz;`);
      shader.fragmentShader=`varying vec3 vHazeWorld;uniform float hazeLevel;uniform float hazeDensity;uniform float hazeBase;uniform float hazeFalloff;uniform vec3 hazeHorizon;uniform vec3 hazeSun;uniform vec3 hazeDirection;\n`+shader.fragmentShader;
      shader.fragmentShader=shader.fragmentShader.replace('#include <fog_fragment>',`
        if(hazeLevel<0.5){
          #include <fog_fragment>
          #ifdef USE_FOG
            float lowValley=clamp(1.0-(vHazeWorld.y-hazeBase)*hazeFalloff,0.0,1.0);
            float lowHaze=clamp(vFogDepth*hazeDensity*0.2,0.0,0.15)*lowValley;
            gl_FragColor.rgb=mix(gl_FragColor.rgb,hazeHorizon,lowHaze);
          #endif
        }else{
        vec3 hazeRay=vHazeWorld-cameraPosition;float hazeDistance=length(hazeRay);
        float h0=max(0.0,cameraPosition.y-hazeBase)*hazeFalloff,h1=max(0.0,vHazeWorld.y-hazeBase)*hazeFalloff;
        float valley=abs(h1-h0)>0.001?(exp(-h0)-exp(-h1))/(h1-h0):exp(-h0);
        float optical=valley*hazeDistance*hazeDensity;
        #ifdef FOG_EXP2
          optical+=fogDensity*fogDensity*hazeDistance*hazeDistance;
        #endif
        float aerial=1.0-exp(-optical);
        float sunWarm=hazeLevel<0.5?0.0:pow(max(dot(hazeRay/max(hazeDistance,0.001),hazeDirection),0.0),8.0)*0.16;
        gl_FragColor.rgb=mix(gl_FragColor.rgb,hazeHorizon+hazeSun*sunWarm,clamp(aerial,0.0,1.0));
        }`);
    };
    material.needsUpdate=true;
  }
  update(dt) {
    this.time+=dt;
    if(this.clouds)this.clouds.material.uniforms.cloudTime.value=this.time;
    if(this.config.haze&&this.time-this.lastScan>0.25) {
      this.scene.traverse(o=>{if(o.material)for(const m of Array.isArray(o.material)?o.material:[o.material])this.patch(m);});
      this.lastScan=this.time;
    }
  }
  dispose(){if(this.clouds){this.clouds.removeFromParent();this.clouds.geometry.dispose();this.clouds.material.dispose();}}
}
