fn on_start(ctx) {
    ctx.state_set("Höhle", 0);
}

fn on_tick(ctx, dt) {
    let p = ctx.position();
    ctx.set_velocity([ctx.input("move_x") * 4.0, 0.0, ctx.input("move_z") * 4.0]);
    let cave = p[0] > -7.0 && p[0] < 7.0 && p[2] < -4.0 && p[2] > -28.0;
    if cave { ctx.state_set("Höhle", 1); } else { ctx.state_set("Höhle", 0); }
    if p[0] < -30.0 || p[0] > 30.0 || p[2] < -38.0 || p[2] > 22.0 {
        ctx.set_position([0.0, 0.5, 4.0]);
    }
}
