import * as THREE from 'three';
import { hdrSettings } from './graphics-settings.js';

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

const composite = /* glsl */`
  uniform sampler2D image;
  uniform sampler2D bloom;
  uniform vec2 texel;
  uniform float strength;
  varying vec2 vUv;
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
    gl_FragColor = vec4(color + texture2D(bloom, vUv).rgb * strength, 1.0);
    #include <tonemapping_fragment>
    #include <colorspace_fragment>
  }
`;

function pass(fragmentShader, uniforms, toneMapped = false) {
  return new THREE.ShaderMaterial({
    vertexShader, fragmentShader, uniforms, toneMapped,
    depthTest: false, depthWrite: false, blending: THREE.NoBlending,
  });
}

function target(depthBuffer) {
  return new THREE.WebGLRenderTarget(1, 1, { type: THREE.HalfFloatType, depthBuffer });
}

export class HDRPipeline {
  constructor(renderer, config) {
    this.renderer = renderer;
    this.supported = renderer.extensions.has('EXT_color_buffer_float');
    this.strength = config.bloomStrength;
    this.threshold = config.bloomThreshold;
    this.edgeAA = config.hdrAntialias !== false;
    this.size = new THREE.Vector2();
  }

  resize(level, width, height, pixelRatio) {
    const settings = hdrSettings(level, this.supported, this.strength, width, height, pixelRatio);
    if (settings.enabled && !this.sceneTarget) this.init();
    if (!settings.enabled) this.release();
    const renderer = this.renderer;
    renderer.setPixelRatio(this.sceneTarget ? settings.pixelRatio : pixelRatio);
    renderer.setSize(width, height, false);
    if (!this.sceneTarget) return;
    renderer.getDrawingBufferSize(this.size);
    const w = Math.max(1, this.size.x), h = Math.max(1, this.size.y);
    this.sceneTarget.setSize(w, h);
    this.prefilter.uniforms.texel.value.set(1 / w, 1 / h);
    const edgeAA = this.edgeAA && level === 2;
    if (edgeAA !== ('EDGE_AA' in this.composite.defines)) {
      this.composite.defines = edgeAA ? { EDGE_AA: 1 } : {};
      this.composite.needsUpdate = true;
    }
    const bw = Math.max(1, Math.ceil(w * settings.bloomScale)), bh = Math.max(1, Math.ceil(h * settings.bloomScale));
    this.bright.setSize(bw, bh); this.scratch.setSize(bw, bh);
  }

  init() {
    const renderer = this.renderer, gl = renderer.getContext();
    this.sceneTarget = target(true);
    const prev = renderer.getRenderTarget();
    let complete;
    try {
      renderer.setRenderTarget(this.sceneTarget);
      complete = gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
    } finally { renderer.setRenderTarget(prev); }
    if (!complete) {
      this.supported = false;
      this.sceneTarget.dispose(); this.sceneTarget = null;
      return;
    }
    this.bright = target(false); this.scratch = target(false);
    this.prefilter = pass(prefilter, {
      image: {value: this.sceneTarget.texture}, texel: {value: new THREE.Vector2()}, threshold: {value: this.threshold},
    });
    this.blur = pass(blur, { image: {value: null}, stepSize: {value: new THREE.Vector2()} });
    this.composite = pass(composite, {
      image: {value: this.sceneTarget.texture}, bloom: {value: this.bright.texture},
      strength: {value: this.strength}, texel: this.prefilter.uniforms.texel,
    }, true);
    this.quadScene = new THREE.Scene(); this.camera = new THREE.Camera();
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
    this.quad = new THREE.Mesh(geometry, this.prefilter);
    this.quad.frustumCulled = false;
    this.quadScene.add(this.quad);
  }

  release() {
    if (!this.sceneTarget) return;
    for (const resource of [this.sceneTarget, this.bright, this.scratch, this.prefilter, this.blur, this.composite, this.quad.geometry]) resource.dispose();
    this.sceneTarget = this.bright = this.scratch = null;
    this.quadScene = this.camera = this.quad = this.prefilter = this.blur = this.composite = null;
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
      draw(this.prefilter, this.bright);
      this.blur.uniforms.image.value = this.bright.texture;
      this.blur.uniforms.stepSize.value.set(1 / this.bright.width, 0);
      draw(this.blur, this.scratch);
      this.blur.uniforms.image.value = this.scratch.texture;
      this.blur.uniforms.stepSize.value.set(0, 1 / this.bright.height);
      draw(this.blur, this.bright);
      draw(this.composite, previous);
    } finally {
      renderer.setRenderTarget(previous);
      renderer.info.autoReset = autoReset;
    }
  }
}
