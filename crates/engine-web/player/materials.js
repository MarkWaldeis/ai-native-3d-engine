import * as THREE from 'three';
import { materialSettings } from './graphics-settings.js';

export function primitiveMaterial(entity) {
  const { clearcoat, clearcoatRoughness, ...settings } = materialSettings(entity.props?.ai_material);
  const parameters = { ...settings, color: new THREE.Color(...(entity.color || [0.8, 0.8, 0.8])) };
  return clearcoat
    ? new THREE.MeshPhysicalMaterial({ ...parameters, clearcoat, clearcoatRoughness })
    : new THREE.MeshStandardMaterial(parameters);
}
