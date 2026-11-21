fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.state_set("won", false);
    ctx.state_set("_spawn", ctx.position());
    ctx.state_set("_respawns", 0);
    ctx.log("Sammle drei Münzen und erreiche das grüne Ziel.");
}

fn on_tick(ctx, dt) {
    let position = ctx.position();
    if position[0] < -5.5 || position[0] > 5.5 || position[2] < -2.5 || position[2] > 2.5 || position[1] < -4.0 {
        ctx.set_position(ctx.state_get("_spawn"));
        ctx.set_velocity([0.0, 0.0, 0.0]);
        ctx.state_set("_respawns", ctx.state_get("_respawns") + 1);
        ctx.log("Zurück auf dem Münzenpfad. Weiter geht's!");
        return;
    }
    ctx.set_velocity([ctx.input("move_x") * 2.0, 0.0, ctx.input("move_z") * 2.0]);
    for coin in ctx.find_by_tag("coin") {
        let target = ctx.entity_position(coin);
        let dx = position[0] - target[0];
        let dy = position[1] - target[1];
        let dz = position[2] - target[2];
        if dx * dx + dy * dy + dz * dz < 0.81 {
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
