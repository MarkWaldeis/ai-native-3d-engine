import * as THREE from 'three';
import { hdrSettings } from './graphics-settings.js';
import { ContactOcclusion, contactComposite } from './contact-occlusion.js';

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

const composite = /* glsl */`
  #include <common>
  #include <dithering_pars_fragment>
  uniform sampler2D image;
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
  ${contactComposite}
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
    gl_FragColor = vec4(color, 1.0);
    #include <tonemapping_fragment>
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
    this.contactSupported = renderer.capabilities.isWebGL2 && !renderer.capabilities.logarithmicDepthBuffer;
    this.size = new THREE.Vector2();
  }

  resize(level, width, height, pixelRatio) {
    const settings = hdrSettings(level, this.supported, this.strength, width, height, pixelRatio, this.edgeAA,
      this.contactSupported && this.contactConfig.contactStrength > 0);
    const wideBloom = settings.bloom && level === 2 && this.scatterRatio > 0;
    if (!settings.enabled || !!this.contact !== settings.contact || !!this.wide !== wideBloom) this.release();
    if (settings.enabled && !this.sceneTarget && !this.init(settings.bloom, settings.contact, wideBloom)) {
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

  init(bloomEnabled, contactEnabled, wideBloom) {
    const renderer = this.renderer, gl = renderer.getContext();
    const depth = contactEnabled ? new THREE.DepthTexture(1, 1) : null;
    const texel = {value: new THREE.Vector2()};
    this.sceneTarget = target(true, depth);
    if (depth) this.contact = new ContactOcclusion(pass, depth, texel, this.contactConfig);
    const prev = renderer.getRenderTarget();
    const complete = renderTarget => {
      renderer.setRenderTarget(renderTarget);
      return gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
    };
    let supported;
    try {
      supported = complete(this.sceneTarget) && (!this.contact || complete(this.contact.target));
    } finally { renderer.setRenderTarget(prev); }
    if (!supported) {
      if (contactEnabled) this.contactSupported = false;
      else this.supported = false;
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
    this.composite = pass(composite, {
      image: {value: this.sceneTarget.texture}, bloom: {value: this.bright?.texture},
      wideBloom: {value: this.wide?.texture}, scatterRatio: {value: this.scatterRatio},
      strength: {value: this.strength}, texel, ...this.contact?.compositeUniforms(this.contactConfig.contactStrength),
    }, true);
    if (bloomEnabled) this.composite.defines.BLOOM = 1;
    if (wideBloom) this.composite.defines.BLOOM_SCATTER = 1;
    if (contactEnabled) this.composite.defines.CONTACT_OCCLUSION = 1;
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
    this.sceneTarget = this.bright = this.scratch = this.wide = null;
    this.quadScene = this.camera = this.quad = this.prefilter = this.blur = this.scatter = this.composite = null;
  }

  render(scene, camera) {
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
      renderer.setRenderTarget(this.sceneTarget); renderer.render(scene, camera);
      this.contact?.render(draw, camera);
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
