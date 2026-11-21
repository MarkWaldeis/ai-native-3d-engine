import * as THREE from 'three';
import { materialMapSettings, materialSettings } from './graphics-settings.js';

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

export function primitiveMaterial(entity, assets) {
  const authored = entity.props?.ai_material, maps = materialMapSettings(authored);
  const color = maps && assets?.materialTexture(maps.map, maps.repeat, THREE.SRGBColorSpace);
  const normal = maps && assets?.materialTexture(maps.normalMap, maps.repeat);
  const orm = maps && assets?.materialTexture(maps.ormMap, maps.repeat);
  const { clearcoat, clearcoatRoughness, ...settings } = materialSettings(authored, !!orm);
  const parameters = { ...settings, color: new THREE.Color(...(entity.color || [0.8, 0.8, 0.8])) };
  if (color) parameters.map = color;
  if (normal) {
    parameters.normalMap = normal;
    parameters.normalScale = new THREE.Vector2(maps.normalScale, maps.normalScale);
  }
  if (orm) parameters.aoMap = parameters.roughnessMap = parameters.metalnessMap = orm;
  return clearcoat || settings.anisotropy || settings.sheen || settings.iridescence
    ? new THREE.MeshPhysicalMaterial({ ...parameters, clearcoat, clearcoatRoughness })
    : new THREE.MeshStandardMaterial(parameters);
}
