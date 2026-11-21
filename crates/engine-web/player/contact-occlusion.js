import * as THREE from 'three';

const readDepth = /* glsl */`
  uniform mat4 inverseProjection;
  uniform float logFar;
  float viewZ(float depth) {
    if (logFar > 0.0) return 1.0 - exp2(depth * logFar);
    vec4 pos = inverseProjection * vec4(0.0, 0.0, depth * 2.0 - 1.0, 1.0);
    return pos.z / pos.w;
  }
`;

export const contactFragment = /* glsl */`
  uniform sampler2D sceneDepth;
  ${readDepth}
  uniform vec2 projectionScale;
  uniform vec2 texel;
  uniform float radius;
  uniform bool perspective;
  varying vec2 vUv;

  vec3 positionAt(vec2 uv, float depth) {
    vec2 pixelCenter = (floor(clamp(uv, texel * 0.5, 1.0 - texel * 0.5) / texel) + 0.5) * texel;
    if (logFar > 0.0) {
      vec3 ray = (inverseProjection * vec4(pixelCenter * 2.0 - 1.0, 0.0, 1.0)).xyz;
      return ray * (viewZ(depth) / ray.z);
    }
    vec4 pos = inverseProjection * vec4(pixelCenter * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
    return pos.xyz / pos.w;
  }
  vec3 positionAt(vec2 uv) {
    return positionAt(uv, texture2D(sceneDepth, uv).r);
  }
  vec3 surfaceAxis(vec3 pos, vec2 step) {
    vec3 before = pos - positionAt(vUv - step);
    vec3 after = positionAt(vUv + step) - pos;
    return abs(before.z) < abs(after.z) ? before : after;
  }
  float obscurance(vec3 pos, vec3 normal, vec2 uv) {
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) return 0.0;
    float depth = texture2D(sceneDepth, uv).r;
    if (depth >= 1.0) return 0.0;
    vec3 delta = positionAt(uv, depth) - pos;
    float distance = length(delta);
    float elevation = dot(normal, delta) / max(distance, 0.0001);
    return max(elevation - 0.12, 0.0) * (1.0 - smoothstep(radius * 0.3, radius, distance));
  }
  void main() {
    float depth = texture2D(sceneDepth, vUv).r;
    gl_FragColor = vec4(0.0);
    if (depth >= 1.0) return;
    vec3 pos = positionAt(vUv, depth);
    vec2 footprint = radius * projectionScale * 0.5 / (perspective ? -pos.z : 1.0);
    footprint = min(footprint, 42.0 * texel);
    if (min(footprint.x / texel.x, footprint.y / texel.y) < 2.0) return;
    vec3 n = cross(surfaceAxis(pos, vec2(texel.x, 0.0)), surfaceAxis(pos, vec2(0.0, texel.y)));
    float magnitude = length(n);
    if (magnitude < 1e-10) return;
    n /= magnitude;
    float occlusion = 0.0;
    for (int i = 0; i < 8; i++) {
      float angle = float(i) * 0.78539816;
      float ring = mod(float(i), 2.0) == 0.0 ? 0.37 : 0.73;
      vec2 uv = vUv + vec2(cos(angle), sin(angle)) * footprint * ring;
      occlusion += obscurance(pos, n, uv);
    }
    gl_FragColor.r = clamp(occlusion * 0.32, 0.0, 1.0);
  }
`;

export const contactComposite = /* glsl */`
  #ifdef CONTACT_OCCLUSION
  uniform sampler2D sceneDepth;
  uniform sampler2D contactMap;
  uniform vec2 contactSize;
  uniform float contactStrength;
  uniform float contactRadius;
  ${readDepth}
  float contactAt(vec2 uv, float centerZ, float weight, inout float totalWeight) {
    float depth = texture2D(sceneDepth, uv).r;
    if (depth >= 1.0) return 0.0;
    weight *= 1.0 - smoothstep(0.05, 0.25, abs(viewZ(depth) - centerZ) / contactRadius);
    totalWeight += weight;
    return texture2D(contactMap, uv).r * weight;
  }
  float contactFactor(vec2 uv) {
    float depth = texture2D(sceneDepth, uv).r;
    if (depth >= 1.0) return 1.0;
    vec2 pixel = uv * contactSize - 0.5;
    vec2 f = fract(pixel);
    vec2 pos = (floor(pixel) + 0.5) / contactSize;
    vec2 step = 1.0 / contactSize;
    float z = viewZ(depth), weight = 0.0;
    float sum = contactAt(pos, z, (1.0 - f.x) * (1.0 - f.y), weight);
    sum += contactAt(pos + vec2(step.x, 0.0), z, f.x * (1.0 - f.y), weight);
    sum += contactAt(pos + vec2(0.0, step.y), z, (1.0 - f.x) * f.y, weight);
    sum += contactAt(pos + step, z, f.x * f.y, weight);
    return 1.0 - sum * contactStrength / max(weight, 0.2);
  }
  #endif
`;

export class ContactOcclusion {
  constructor(pass, sceneDepth, texel, config, logarithmicDepth) {
    this.logarithmicDepth = logarithmicDepth;
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      depthBuffer: false, format: THREE.RedFormat,
      minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter,
    });
    this.size = new THREE.Vector2();
    this.uniforms = {
      sceneDepth: {value: sceneDepth}, texel,
      inverseProjection: {value: new THREE.Matrix4()}, projectionScale: {value: new THREE.Vector2()},
      radius: {value: config.contactRadius}, perspective: {value: true}, logFar: {value: 0},
    };
    this.material = pass(contactFragment, this.uniforms);
  }
  compositeUniforms(strength) {
    return {
      sceneDepth: this.uniforms.sceneDepth, inverseProjection: this.uniforms.inverseProjection,
      logFar: this.uniforms.logFar,
      contactRadius: this.uniforms.radius, contactStrength: {value: strength},
      contactMap: {value: this.target.texture}, contactSize: {value: this.size},
    };
  }
  resize(width, height) {
    this.size.set(Math.ceil(width / 2), Math.ceil(height / 2));
    this.target.setSize(this.size.x, this.size.y);
  }
  render(draw, camera) {
    const projection = camera.projectionMatrix.elements;
    this.uniforms.inverseProjection.value.copy(camera.projectionMatrixInverse);
    this.uniforms.projectionScale.value.set(projection[0], projection[5]);
    this.uniforms.perspective.value = !!camera.isPerspectiveCamera;
    // Three.js stores log2(1 - viewZ) / log2(1 + far) for perspective cameras.
    this.uniforms.logFar.value = this.logarithmicDepth && camera.isPerspectiveCamera ? Math.log2(1 + camera.far) : 0;
    draw(this.material, this.target);
  }
  dispose() {
    this.target.dispose();
    this.material.dispose();
  }
}
