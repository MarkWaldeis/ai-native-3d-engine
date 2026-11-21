fn limit(value, low, high) {
    if value < low { low } else if value > high { high } else { value }
}

fn on_tick(ctx, dt) {
    let p = ctx.position();
    let next_x = p[0] + ctx.input("move_x") * 2.4 * dt;
    let next_z = p[2] + ctx.input("move_z") * 2.4 * dt;
    ctx.set_position([limit(next_x, -5.8, 5.8), p[1], limit(next_z, 0.5, 4.0)]);
}
