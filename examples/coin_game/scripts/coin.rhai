fn on_tick(ctx, dt) {
    let rotation = ctx.rotation();
    rotation[1] += 90.0 * dt;
    ctx.set_rotation(rotation);
}
