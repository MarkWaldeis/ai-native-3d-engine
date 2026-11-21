fn on_tick(ctx, dt) {
    let rotation = ctx.rotation();
    rotation[1] += dt * 65.0;
    ctx.set_rotation(rotation);
}
