import * as THREE from 'three';
import { glyphSdf, ShelfPacker, layoutText } from './sdf-font.js';

export class SdfAtlas {
  constructor() {
    this.glyphs = new Map(); this.warnings = []; this.packer = new ShelfPacker();
    this.data = new Uint8Array(1024 * 1024);
    this.texture = new THREE.DataTexture(this.data, 1024, 1024, THREE.RedFormat);
    this.texture.minFilter = this.texture.magFilter = THREE.LinearFilter;
    this.texture.generateMipmaps = false; this.texture.unpackAlignment = 1; this.texture.needsUpdate = true;
    this.canvas = document.createElement('canvas'); this.context = this.canvas.getContext('2d', { willReadFrequently: true });
  }
  glyph(char) {
    if (this.glyphs.has(char)) return this.glyphs.get(char);
    const ctx = this.context; ctx.font = '48px sans-serif';
    const m = ctx.measureText(char), left = Math.ceil(m.actualBoundingBoxLeft), top = Math.ceil(m.actualBoundingBoxAscent);
    const width = Math.max(0, left + Math.ceil(m.actualBoundingBoxRight)) + 16;
    const height = Math.max(0, top + Math.ceil(m.actualBoundingBoxDescent)) + 16;
    const glyph = { advance: m.width / 48, left: (-left - 8) / 48, top: (top + 8) / 48, width: 0, height: 0 };
    if (char.trim()) {
      const rect = this.packer.pack(width + 2, height + 2);
      if (rect) {
        this.canvas.width = width; this.canvas.height = height;
        ctx.font = '48px sans-serif'; ctx.fillStyle = '#fff'; ctx.textBaseline = 'alphabetic';
        ctx.fillText(char, 8 + left, 8 + top);
        const rgba = ctx.getImageData(0, 0, width, height).data;
        const sdf = glyphSdf(Uint8Array.from({ length: width * height }, (_, i) => rgba[i * 4 + 3]), width, height, 8);
        for (let y = 0; y < height; y++) this.data.set(sdf.subarray(y * width, (y + 1) * width), (rect.y + 1 + y) * 1024 + rect.x + 1);
        Object.assign(glyph, { x: rect.x + 1, y: rect.y + 1, pixelsWidth: width, pixelsHeight: height, width: width / 48, height: height / 48 });
        this.texture.needsUpdate = true;
      } else if (!this.warnings.length) this.warnings.push('Text: SDF-Atlas ist voll; weitere Glyphen werden ausgelassen.');
    }
    this.glyphs.set(char, glyph); return glyph;
  }
  dispose() { this.texture.dispose(); }
}

export class SdfLabel extends THREE.Object3D {
  constructor(atlas, config) {
    super(); this.isSdfLabel = true; this.atlas = atlas; this.config = config; this.text = null;
    this.material = new THREE.ShaderMaterial({ transparent: true, depthWrite: false, depthTest: config.depth_test, side: THREE.DoubleSide,
      uniforms: { atlas: { value: atlas.texture }, color: { value: new THREE.Color(config.color) }, outlineColor: { value: new THREE.Color(config.outline_color) }, outlineWidth: { value: config.outline_width }, opacity: { value: 1 } },
      vertexShader: `#include <common>
        #include <logdepthbuf_pars_vertex>
        varying vec2 vUv; void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);
        #include <logdepthbuf_vertex>
        }`,
      fragmentShader: `#include <logdepthbuf_pars_fragment>
        uniform sampler2D atlas; uniform vec3 color; uniform vec3 outlineColor; uniform float outlineWidth; uniform float opacity; varying vec2 vUv;
        void main(){
        #include <logdepthbuf_fragment>
        float d=texture2D(atlas,vUv).r;float w=max(fwidth(d),0.001);float fill=smoothstep(0.5-w,0.5+w,d);
        float alpha=smoothstep(0.5-outlineWidth-w,0.5-outlineWidth+w,d);if(alpha<0.003)discard;
        gl_FragColor=vec4(mix(outlineColor,color,fill),alpha*opacity);
        #include <tonemapping_fragment>
        #include <colorspace_fragment>
        }`,
    });
    this.mesh = new THREE.Mesh(new THREE.BufferGeometry(), this.material); this.mesh.frustumCulled = false; this.add(this.mesh);
    this.position.fromArray(config.offset || [0, 0.4, 0]);
    this.worldPosition = new THREE.Vector3(); this.cameraPosition = new THREE.Vector3(); this.worldScale = new THREE.Vector3(); this.rotationWorld = new THREE.Quaternion();
    this.bounds = new THREE.Box3();
  }
  setText(text) {
    if (text === this.text) return false;
    this.text = text;
    const positions = [], uvs = [], indices = [];
    for (const q of layoutText(text, char => this.atlas.glyph(char), { size: this.config.size, maxWidth: this.config.max_width, align: this.config.align })) {
      const i = positions.length / 3, x = q.x, y = q.y, w = q.width, h = q.height;
      positions.push(x, y, 0, x + w, y, 0, x + w, y + h, 0, x, y + h, 0);
      const u = this.atlas.glyph(q.char);
      const u0 = u.x / 1024, u1 = (u.x + u.pixelsWidth) / 1024, v0 = u.y / 1024, v1 = (u.y + u.pixelsHeight) / 1024;
      uvs.push(u0, v1, u1, v1, u1, v0, u0, v0); indices.push(i, i + 1, i + 2, i, i + 2, i + 3);
    }
    const geometry = new THREE.BufferGeometry(); geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2)); geometry.setIndex(indices);
    this.mesh.geometry.dispose(); this.mesh.geometry = geometry; return true;
  }
  update(camera) {
    if (this.boundsObject) {
      this.parent.getWorldPosition(this.worldPosition);
      this.bounds.setFromObject(this.boundsObject);
      if (!this.bounds.isEmpty()) this.worldPosition.y = this.bounds.max.y;
      this.worldPosition.y += 0.4;
      this.position.copy(this.parent.worldToLocal(this.worldPosition));
    }
    this.getWorldPosition(this.worldPosition); camera.getWorldPosition(this.cameraPosition);
    if (this.config.billboard) {
      const yaw = Math.atan2(this.cameraPosition.x - this.worldPosition.x, this.cameraPosition.z - this.worldPosition.z);
      this.rotationWorld.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, yaw);
      this.parent.getWorldQuaternion(this.quaternion).invert().multiply(this.rotationWorld);
    }
    this.parent.getWorldScale(this.worldScale);
    this.scale.set(1 / (this.worldScale.x || 1), 1 / (this.worldScale.y || 1), 1 / (this.worldScale.z || 1));
    const distance = this.worldPosition.distanceTo(this.cameraPosition), fade = this.config.fade_distance;
    this.material.uniforms.opacity.value = fade ? Math.max(0, Math.min(1, (fade - distance) / Math.max(0.01, fade * 0.2))) : 1;
    this.visible = this.material.uniforms.opacity.value > 0;
  }
  dispose() { this.mesh.geometry.dispose(); this.material.dispose(); this.removeFromParent(); }
}
