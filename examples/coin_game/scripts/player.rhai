fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.log("Collect three gold coins and reach the green goal.");
}

fn on_tick(ctx, dt) {
    ctx.set_velocity([ctx.input("move_x") * 2.0, 0.0, 0.0]);
    let position = ctx.position();
    for coin in ctx.find_by_tag("coin") {
        let target = ctx.entity_position(coin);
        let dx = position[0] - target[0];
        let dy = position[1] - target[1];
        let dz = position[2] - target[2];
        if dx * dx + dy * dy + dz * dz < 0.36 {
            ctx.remove(coin);
            ctx.state_set("score", ctx.state_get("score") + 1);
            ctx.log("Collected a coin");
        }
    }
}
