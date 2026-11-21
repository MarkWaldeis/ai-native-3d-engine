import * as THREE from 'three';
import { materialSettings } from './graphics-settings.js';
import { roadSurface } from './surfaces.js';

export function textureAnisotropy(level, maxAnisotropy) {
  return Math.max(1, Math.min([1, 4, 8][level] ?? 1, maxAnisotropy));
}

export function tuneAssetTextures(scene, anisotropy) {
  const seen = new Set();
  scene.traverse(mesh => {
    if (!mesh.isMesh) return;
    mesh.userData.sharedAsset = true;
    if (anisotropy <= 1) return;
    for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      if (seen.has(material)) continue;
      seen.add(material);
      for (const texture of Object.values(material)) {
        if (!texture?.isTexture || texture.magFilter === THREE.NearestFilter || texture.anisotropy >= anisotropy) continue;
        if (texture.minFilter !== THREE.NearestMipmapLinearFilter && texture.minFilter !== THREE.LinearMipmapLinearFilter) continue;
        if (!texture.generateMipmaps && texture.mipmaps.length <= 1) continue;
        texture.anisotropy = anisotropy;
        texture.needsUpdate = true;
      }
    }
  });
}

export function primitiveMaterial(entity) {
  const { clearcoat, clearcoatRoughness, ...settings } = materialSettings(entity.props?.ai_material);
  const parameters = { ...settings, color: new THREE.Color(...(entity.color || [0.8, 0.8, 0.8])) };
  const material = clearcoat || settings.anisotropy || settings.sheen || settings.iridescence
    ? new THREE.MeshPhysicalMaterial({ ...parameters, clearcoat, clearcoatRoughness })
    : new THREE.MeshStandardMaterial(parameters);
  if(entity.tags?.includes('road') && entity.kind==='box')roadSurface(THREE,material);
  return material;
}
