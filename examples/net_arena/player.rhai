fn on_start(ctx) {
    ctx.state_set("score_" + ctx.id().to_string(), 0);
}

fn on_tick(ctx, dt) {
    let velocity = ctx.velocity();
    ctx.set_velocity([ctx.input("move_x") * 2.0, velocity[1], ctx.input("move_z") * 2.0]);
    let position = ctx.position();
    if position[0].abs() < 1.5 {
        ctx.state_set("score_" + ctx.id().to_string(), 1);
    }
}
