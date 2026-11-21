import * as THREE from 'three';
import { hdrSettings } from './graphics-settings.js';
import { ContactOcclusion, contactComposite, depthUniforms, readDepth, updateCameraDepth } from './contact-occlusion.js';

const vertexShader = /* glsl */`
  varying vec2 vUv;
  void main() {
    vUv = position.xy * 0.5 + 0.5;
    gl_Position = vec4(position, 1.0);
  }
`;

const prefilter = /* glsl */`
  uniform sampler2D image;
  uniform vec2 texel;
  uniform float threshold;
  varying vec2 vUv;
  vec3 highlight(vec2 uv) {
    vec3 color = texture2D(image, uv).rgb;
    float peak = max(color.r, max(color.g, color.b));
    float weight = smoothstep(threshold, threshold + 0.6, peak);
    return color * weight;
  }
  void main() {
    vec3 color = highlight(vUv + texel * vec2(-0.5, -0.5))
               + highlight(vUv + texel * vec2(-0.5,  0.5))
               + highlight(vUv + texel * vec2( 0.5, -0.5))
               + highlight(vUv + texel * vec2( 0.5,  0.5));
    gl_FragColor = vec4(color * 0.25, 1.0);
  }
`;

const blur = /* glsl */`
  uniform sampler2D image;
  uniform vec2 stepSize;
  varying vec2 vUv;
  void main() {
    vec3 color = texture2D(image, vUv).rgb * 0.227027;
    color += (texture2D(image, vUv + stepSize * 1.384615).rgb
            + texture2D(image, vUv - stepSize * 1.384615).rgb) * 0.316216;
    color += (texture2D(image, vUv + stepSize * 3.230769).rgb
            + texture2D(image, vUv - stepSize * 3.230769).rgb) * 0.070270;
    gl_FragColor = vec4(color, 1.0);
  }
`;

const scatter = /* glsl */`
  uniform sampler2D image;
  uniform vec2 stepSize;
  varying vec2 vUv;
  void main() {
    vec3 color = vec3(0.0);
    for (int y = -1; y <= 1; ++y) {
      for (int x = -1; x <= 1; ++x) {
        float weight = float((2 - abs(x)) * (2 - abs(y)));
        color += texture2D(image, vUv + vec2(x, y) * stepSize).rgb * weight;
      }
    }
    gl_FragColor = vec4(color / 16.0, 1.0);
  }
`;

const heightFog = /* glsl */`
  #ifdef HEIGHT_FOG
  uniform mat4 fogCameraWorld;
  uniform bool fogPerspective;
  uniform vec3 fogColor;
  uniform vec3 fogProfile;

  float fogAmount(vec2 uv) {
    float depth = texture2D(sceneDepth, uv).r;
    if (depth >= 1.0) return 0.0;
    vec4 clip = vec4(uv * 2.0 - 1.0, logFar > 0.0 ? 0.0 : depth * 2.0 - 1.0, 1.0);
    vec4 unprojected = inverseProjection * clip;
    vec3 view = logFar > 0.0 ? unprojected.xyz * (viewZ(depth) / unprojected.z)
                            : unprojected.xyz / unprojected.w;
    vec3 origin = fogPerspective ? vec3(0.0) : vec3(view.xy, 0.0);
    float startY = (fogCameraWorld * vec4(origin, 1.0)).y;
    float endY = (fogCameraWorld * vec4(view, 1.0)).y;
    float lo = (min(startY, endY) - fogProfile.y) / fogProfile.z;
    float hi = (max(startY, endY) - fogProfile.y) / fogProfile.z;
    // Integrate a constant density below the base and exponential density above it.
    float span = hi - lo;
    float density = exp(-max(0.5 * (lo + hi), 0.0));
    if (span > 0.001) {
      float below = max(min(hi, 0.0) - lo, 0.0);
      float above = max(hi, 0.0) - max(lo, 0.0);
      density = (below + exp(-max(lo, 0.0)) * (1.0 - exp(-above))) / span;
    }
    return 1.0 - exp(-fogProfile.x * density * length(view - origin));
  }
  #endif
`;

const meter = /* glsl */`
  uniform sampler2D image;
  uniform sampler2D history;
  uniform vec2 limits;
  uniform vec2 speeds;
  uniform float elapsed;
  uniform bool initialized;
  #ifdef HEIGHT_FOG
  ${readDepth}
  #endif
  ${heightFog}
  void main() {
    float sum = 0.0;
    for (int y = 0; y < 8; ++y) {
      for (int x = 0; x < 8; ++x) {
        vec2 uv = (vec2(x, y) + 0.5) / 8.0;
        vec3 color = texture2D(image, uv).rgb;
        #ifdef HEIGHT_FOG
        color = mix(color, fogColor, fogAmount(uv));
        #endif
        float luma = dot(max(color, 0.0), vec3(0.2126, 0.7152, 0.0722));
        sum += log2(clamp(luma, 0.001, 64.0));
      }
    }
    float ev = clamp(log2(0.18) - sum / 64.0, limits.x, limits.y);
    if (initialized) {
      float last = texture2D(history, vec2(0.5)).r;
      ev = mix(last, ev, 1.0 - exp(-elapsed * (ev > last ? speeds.x : speeds.y)));
    }
    gl_FragColor = vec4(ev, 0.0, 0.0, 1.0);
  }
`;

const composite = /* glsl */`
  #include <common>
  #include <dithering_pars_fragment>
  uniform sampler2D image;
  #ifdef AUTO_EXPOSURE
  uniform sampler2D exposureMeter;
  #endif
  #ifdef COLOR_GRADE
  uniform vec2 colorGrade;
  #endif
  #ifdef BLOOM
  uniform sampler2D bloom;
  uniform float strength;
  #endif
  #ifdef BLOOM_SCATTER
  uniform sampler2D wideBloom;
  uniform float scatterRatio;
  #endif
  uniform vec2 texel;
  varying vec2 vUv;
  #if defined(CONTACT_OCCLUSION) || defined(HEIGHT_FOG)
  ${readDepth}
  #endif
  ${contactComposite}
  ${heightFog}
  #ifdef EDGE_AA
  float perceptualLuma(vec3 color) {
    float luma = dot(max(color, 0.0), vec3(0.2126, 0.7152, 0.0722));
    return sqrt(luma / (1.0 + luma));
  }
  vec3 smoothEdges(vec3 center) {
    float sw = perceptualLuma(texture2D(image, vUv + texel * vec2(-1, -1)).rgb);
    float se = perceptualLuma(texture2D(image, vUv + texel * vec2( 1, -1)).rgb);
    float nw = perceptualLuma(texture2D(image, vUv + texel * vec2(-1,  1)).rgb);
    float ne = perceptualLuma(texture2D(image, vUv + texel * vec2( 1,  1)).rgb);
    float lum = perceptualLuma(center);
    float lo = min(lum, min(min(sw, se), min(nw, ne)));
    float hi = max(lum, max(max(sw, se), max(nw, ne)));
    if (hi - lo < max(0.04, 0.12 * hi)) return center;
    // Filter along the luminance edge, not across the interiors it separates.
    vec2 edge = vec2(sw + se - nw - ne, se + ne - sw - nw);
    float noiseFloor = max(0.02, (sw + se + nw + ne) * 0.03125);
    edge = clamp(edge / (min(abs(edge.x), abs(edge.y)) + noiseFloor), -4.0, 4.0) * texel;
    vec3 a = 0.5 * (texture2D(image, vUv - edge / 6.0).rgb + texture2D(image, vUv + edge / 6.0).rgb);
    vec3 b = 0.5 * a + 0.25 * (texture2D(image, vUv - edge * 0.5).rgb + texture2D(image, vUv + edge * 0.5).rgb);
    float luma = perceptualLuma(b);
    return luma < lo || luma > hi ? a : b;
  }
  #endif
  void main() {
    vec3 color = texture2D(image, vUv).rgb;
    #ifdef EDGE_AA
    color = smoothEdges(color);
    #endif
    #ifdef CONTACT_OCCLUSION
    color *= contactFactor(vUv);
    #endif
    #ifdef BLOOM
    vec3 glow = texture2D(bloom, vUv).rgb;
    #ifdef BLOOM_SCATTER
    glow = mix(glow, texture2D(wideBloom, vUv).rgb, scatterRatio);
    #endif
    color += glow * strength;
    #endif
    #ifdef HEIGHT_FOG
    color = mix(color, fogColor, fogAmount(vUv));
    #endif
    #ifdef AUTO_EXPOSURE
    color *= exp2(texture2D(exposureMeter, vec2(0.5)).r);
    #endif
    gl_FragColor = vec4(color, 1.0);
    #include <tonemapping_fragment>
    #ifdef COLOR_GRADE
    vec3 display = gl_FragColor.rgb;
    float luma = dot(display, vec3(0.2126, 0.7152, 0.0722));
    display = clamp(mix(vec3(luma), display, colorGrade.x), 0.0, 1.0);
    gl_FragColor.rgb = mix(display, smoothstep(vec3(0.0), vec3(1.0), display), colorGrade.y - 1.0);
    #endif
    #include <colorspace_fragment>
    #include <dithering_fragment>
  }
`;

function pass(fragmentShader, uniforms, toneMapped = false) {
  return new THREE.ShaderMaterial({
    vertexShader, fragmentShader, uniforms, toneMapped, dithering: toneMapped,
    depthTest: false, depthWrite: false, blending: THREE.NoBlending,
  });
}

function target(depthBuffer, depthTexture = null) {
  return new THREE.WebGLRenderTarget(1, 1, { type: THREE.HalfFloatType, depthBuffer, depthTexture });
}

class ExposureMeter {
  constructor(image, {minEV, maxEV, brightenSpeed, darkenSpeed}, fogUniforms) {
    this.buffers = [target(false), target(false)];
    this.next = 0;
    this.material = pass(meter, {
      image: {value: image}, history: {value: null},
      limits: {value: new THREE.Vector2(minEV, maxEV)},
      speeds: {value: new THREE.Vector2(brightenSpeed, darkenSpeed)},
      elapsed: {value: 0}, initialized: {value: false},
      ...fogUniforms,
    });
    if (fogUniforms) this.material.defines.HEIGHT_FOG = 1;
  }

  render(draw, elapsed) {
    const {uniforms} = this.material;
    uniforms.history.value = this.buffers[1 - this.next].texture;
    uniforms.elapsed.value = elapsed;
    const output = this.buffers[this.next];
    draw(this.material, output);
    this.next = 1 - this.next;
    uniforms.initialized.value = true;
    return output.texture;
  }

  reset() { this.material.uniforms.initialized.value = false; }

  dispose() {
    for (const buffer of this.buffers) buffer.dispose();
    this.material.dispose();
  }
}

export class HDRPipeline {
  constructor(renderer, config) {
    this.renderer = renderer;
    this.supported = renderer.extensions.has('EXT_color_buffer_float');
    this.strength = config.bloomStrength;
    this.threshold = config.bloomThreshold;
    this.renderScale = config.renderScale ?? 1;
    this.scatterRatio = config.bloomScatter ?? 0;
    this.edgeAA = config.hdrAntialias !== false;
    this.contactConfig = config;
    this.exposureConfig = config.autoExposure;
    this.fogConfig = config.heightFog;
    this.colorGrade = config.colorGrade;
    this.meterSupported = renderer.capabilities.isWebGL2;
    this.contactSupported = renderer.capabilities.isWebGL2;
    this.fogSupported = renderer.capabilities.isWebGL2;
    this.size = new THREE.Vector2();
  }

  resize(level, width, height, pixelRatio) {
    const meterEnabled = !!(level > 0 && this.supported && this.meterSupported && this.exposureConfig);
    const fogEnabled = !!(level > 0 && this.supported && this.fogSupported && this.fogConfig?.density > 0);
    const settings = hdrSettings(level, this.supported, this.strength, width, height, pixelRatio, this.edgeAA,
      this.contactSupported && this.contactConfig.contactStrength > 0, meterEnabled || fogEnabled || !!this.colorGrade);
    const wideBloom = settings.bloom && level === 2 && this.scatterRatio > 0;
    if (!settings.enabled || !!this.contact !== settings.contact || !!this.wide !== wideBloom
      || !!this.exposureMeter !== meterEnabled || !!this.fogEnabled !== fogEnabled) this.release();
    if (settings.enabled && !this.sceneTarget && !this.init(settings.bloom, settings.contact, wideBloom, meterEnabled, fogEnabled)) {
      this.resize(level, width, height, pixelRatio);
      return;
    }
    const renderer = this.renderer;
    renderer.setPixelRatio((this.sceneTarget ? settings.pixelRatio : pixelRatio) * this.renderScale);
    renderer.setSize(width, height, false);
    if (!this.sceneTarget) return;
    renderer.getDrawingBufferSize(this.size);
    const w = Math.max(1, this.size.x), h = Math.max(1, this.size.y);
    this.sceneTarget.setSize(w, h);
    this.contact?.resize(w, h);
    this.composite.uniforms.texel.value.set(1 / w, 1 / h);
    const edgeAA = this.edgeAA && level === 2;
    if (edgeAA !== ('EDGE_AA' in this.composite.defines)) {
      if (edgeAA) this.composite.defines.EDGE_AA = 1;
      else delete this.composite.defines.EDGE_AA;
      this.composite.needsUpdate = true;
    }
    if (settings.bloom) {
      const bw = Math.max(1, Math.ceil(w * settings.bloomScale)), bh = Math.max(1, Math.ceil(h * settings.bloomScale));
      this.bright.setSize(bw, bh); this.scratch.setSize(bw, bh);
      if (this.wide) {
        this.wide.setSize(Math.ceil(w / 8), Math.ceil(h / 8));
        this.scatter.uniforms.stepSize.value.set(5 / bw, 5 / bh);
      }
    }
  }

  init(bloomEnabled, contactEnabled, wideBloom, meterEnabled, fogEnabled) {
    const renderer = this.renderer, gl = renderer.getContext();
    const depth = contactEnabled || fogEnabled ? new THREE.DepthTexture(1, 1) : null;
    const texel = {value: new THREE.Vector2()};
    this.sceneTarget = target(true, depth);
    if (contactEnabled) this.contact = new ContactOcclusion(pass, depth, texel, this.contactConfig, renderer.capabilities.logarithmicDepthBuffer);
    const contactUniforms = this.contact?.compositeUniforms(this.contactConfig.contactStrength);
    let fogUniforms = null;
    if (fogEnabled) {
      const {sceneDepth, inverseProjection, logFar} = contactUniforms ?? depthUniforms(depth);
      fogUniforms = {
        sceneDepth, inverseProjection, logFar,
        fogColor: {value: new THREE.Color(this.fogConfig.color)},
        fogProfile: {value: new THREE.Vector3(this.fogConfig.density, this.fogConfig.baseHeight, this.fogConfig.falloff)},
        fogCameraWorld: {value: null}, fogPerspective: {value: true},
      };
    }
    if (meterEnabled) this.exposureMeter = new ExposureMeter(this.sceneTarget.texture, this.exposureConfig, fogUniforms);
    const prev = renderer.getRenderTarget();
    const complete = renderTarget => {
      renderer.setRenderTarget(renderTarget);
      return gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
    };
    let sceneComplete, contactComplete;
    let meterComplete = true;
    try {
      sceneComplete = complete(this.sceneTarget);
      contactComplete = !this.contact || (sceneComplete && complete(this.contact.target));
      if (sceneComplete && contactComplete && meterEnabled) meterComplete = this.exposureMeter.buffers.every(complete);
    } finally { renderer.setRenderTarget(prev); }
    if (!sceneComplete || !contactComplete || !meterComplete) {
      if (!sceneComplete) {
        if (!depth) this.supported = false;
        if (contactEnabled) this.contactSupported = false;
        if (fogEnabled) this.fogSupported = false;
      } else if (!contactComplete) this.contactSupported = false;
      else this.meterSupported = false;
      this.exposureMeter?.dispose(); this.exposureMeter = null;
      this.contact?.dispose(); this.contact = null;
      this.sceneTarget.dispose(); this.sceneTarget = null;
      return false;
    }
    if (bloomEnabled) {
      this.bright = target(false); this.scratch = target(false);
      this.prefilter = pass(prefilter, {
        image: {value: this.sceneTarget.texture}, texel, threshold: {value: this.threshold},
      });
      this.blur = pass(blur, { image: {value: null}, stepSize: {value: new THREE.Vector2()} });
    }
    if (wideBloom) {
      this.wide = target(false);
      this.scatter = pass(scatter, { image: {value: this.bright.texture}, stepSize: {value: new THREE.Vector2()} });
    }
    this.fogEnabled = fogEnabled;
    this.composite = pass(composite, {
      image: {value: this.sceneTarget.texture}, bloom: {value: this.bright?.texture},
      wideBloom: {value: this.wide?.texture}, scatterRatio: {value: this.scatterRatio},
      strength: {value: this.strength}, texel, exposureMeter: {value: null},
      ...fogUniforms,
      ...contactUniforms,
    }, true);
    if (bloomEnabled) this.composite.defines.BLOOM = 1;
    if (wideBloom) this.composite.defines.BLOOM_SCATTER = 1;
    if (contactEnabled) this.composite.defines.CONTACT_OCCLUSION = 1;
    if (meterEnabled) this.composite.defines.AUTO_EXPOSURE = 1;
    if (fogEnabled) this.composite.defines.HEIGHT_FOG = 1;
    if (this.colorGrade) {
      this.composite.defines.COLOR_GRADE = 1;
      this.composite.uniforms.colorGrade = {value: new THREE.Vector2(this.colorGrade.saturation, this.colorGrade.contrast)};
    }
    this.quadScene = new THREE.Scene(); this.camera = new THREE.Camera();
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
    this.quad = new THREE.Mesh(geometry, this.composite);
    this.quad.frustumCulled = false;
    this.quadScene.add(this.quad);
    return true;
  }

  release() {
    if (!this.sceneTarget) return;
    for (const resource of [this.sceneTarget, this.bright, this.scratch, this.wide,
      this.prefilter, this.blur, this.scatter, this.composite, this.quad.geometry]) resource?.dispose();
    this.contact?.dispose(); this.contact = null;
    this.exposureMeter?.dispose(); this.exposureMeter = null;
    this.fogEnabled = false;
    this.sceneTarget = this.bright = this.scratch = this.wide = null;
    this.quadScene = this.camera = this.quad = this.prefilter = this.blur = this.scatter = this.composite = null;
  }

  resetExposure() { this.exposureMeter?.reset(); }

  render(scene, camera, elapsed = 0) {
    const renderer = this.renderer;
    if (!this.sceneTarget) { renderer.render(scene, camera); return; }
    const previous = renderer.getRenderTarget(), autoReset = renderer.info.autoReset;
    renderer.info.autoReset = false;
    renderer.info.reset();
    const draw = (material, destination) => {
      this.quad.material = material;
      renderer.setRenderTarget(destination);
      renderer.render(this.quadScene, this.camera);
    };
    try {
      renderer.setRenderTarget(this.sceneTarget);
      const fog = scene.fog;
      try {
        if (this.fogEnabled) scene.fog = null;
        renderer.render(scene, camera);
      } finally {
        if (this.fogEnabled) scene.fog = fog;
      }
      this.contact?.render(draw, camera);
      if (this.fogEnabled) {
        const {uniforms} = this.composite;
        if (!this.contact) updateCameraDepth(uniforms, camera, renderer.capabilities.logarithmicDepthBuffer);
        uniforms.fogCameraWorld.value = camera.matrixWorld;
        uniforms.fogPerspective.value = !!camera.isPerspectiveCamera;
      }
      if (this.exposureMeter) this.composite.uniforms.exposureMeter.value = this.exposureMeter.render(draw, elapsed);
      if (this.bright) {
        draw(this.prefilter, this.bright);
        this.blur.uniforms.image.value = this.bright.texture;
        this.blur.uniforms.stepSize.value.set(1 / this.bright.width, 0);
        draw(this.blur, this.scratch);
        this.blur.uniforms.image.value = this.scratch.texture;
        this.blur.uniforms.stepSize.value.set(0, 1 / this.bright.height);
        draw(this.blur, this.bright);
        if (this.wide) draw(this.scatter, this.wide);
      }
      draw(this.composite, previous);
    } finally {
      renderer.setRenderTarget(previous);
      renderer.info.autoReset = autoReset;
    }
  }
}
