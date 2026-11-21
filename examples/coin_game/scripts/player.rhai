fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.state_set("won", false);
    ctx.log("Sammle drei Münzen und erreiche das grüne Ziel.");
}

fn on_tick(ctx, dt) {
    ctx.set_velocity([ctx.input("move_x") * 2.0, 0.0, ctx.input("move_z") * 2.0]);
    let position = ctx.position();
    for coin in ctx.find_by_tag("coin") {
        let target = ctx.entity_position(coin);
        let dx = position[0] - target[0];
        let dy = position[1] - target[1];
        let dz = position[2] - target[2];
        if dx * dx + dy * dy + dz * dz < 0.36 {
            ctx.remove(coin);
            ctx.state_set("score", ctx.state_get("score") + 1);
            ctx.log("Münze gesammelt");
        }
    }
    if ctx.state_get("score") == 3 && !ctx.state_get("won") {
        let goal = ctx.entity_position(ctx.find_by_name("goal-pad"));
        let dx = position[0] - goal[0];
        let dz = position[2] - goal[2];
        if dx * dx + dz * dz < 0.49 {
            ctx.state_set("won", true);
        }
    }
}
