export class ActionLatch {
  held = new Set();
  queued = false;
  press(id) {
    if (!this.held.has(id)) this.queued = true;
    this.held.add(id);
  }
  release(id, cancelled = false) {
    this.held.delete(id);
    if (cancelled && !this.held.size) this.queued = false;
  }
  sample() {
    const value = Number(this.queued || this.held.size > 0);
    this.queued = false;
    return value;
  }
  reset() { this.held.clear(); this.queued = false; }
}

export function viewMovement(x, z, offset) {
  const length = Math.max(1, Math.hypot(x, z));
  const horizontal = Math.hypot(offset[0], offset[2]);
  const bx = horizontal > 0 ? offset[0] / horizontal : 0;
  const bz = horizontal > 0 ? offset[2] / horizontal : 1;
  return { x: (bz * x + bx * z) / length, z: (-bx * x + bz * z) / length };
}

export function followDistance(aspect, fov = 55) {
  if (!Number.isFinite(aspect) || aspect <= 0) throw new RangeError('aspect must be positive');
  return Math.max(8.5, 6.1 / (2 * Math.tan(fov * Math.PI / 360) * aspect));
}
