export class GpuTimer {
  constructor(gl) {
    this.gl = gl;
    this.ext = gl.getExtension('EXT_disjoint_timer_query_webgl2');
    this.pending = null;
    this.active = false;
    this.ms = null;
  }

  begin() {
    const {gl, ext} = this;
    if (!ext || gl.isContextLost()) return;
    if (gl.getParameter(ext.GPU_DISJOINT_EXT)) {
      this.dispose();
      return;
    }
    if (this.pending) {
      if (!gl.getQueryParameter(this.pending, gl.QUERY_RESULT_AVAILABLE)) return;
      this.ms = gl.getQueryParameter(this.pending, gl.QUERY_RESULT) / 1e6;
      gl.deleteQuery(this.pending);
    }
    this.pending = gl.createQuery();
    if (!this.pending) return;
    gl.beginQuery(ext.TIME_ELAPSED_EXT, this.pending);
    this.active = true;
  }

  end() {
    if (!this.active) return;
    this.gl.endQuery(this.ext.TIME_ELAPSED_EXT);
    this.active = false;
  }

  dispose() {
    this.end();
    if (this.pending) this.gl.deleteQuery(this.pending);
    this.pending = null;
    this.ms = null;
  }
}
