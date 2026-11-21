fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.state_set("won", false);
    ctx.state_set("Sprünge", 0);
    ctx.state_set("Rettungen", 0);
    ctx.state_set("_held", false);
    ctx.state_set("_coyote", 0);
    ctx.state_set("_jump_buffer", 0.0);
    ctx.state_set("_checkpoint", 0.0);
    ctx.log("Willkommen, Milo! Joystick: laufen. Aktion: springen.");
}

fn on_tick(ctx, dt) {
    let p = ctx.position();
    if p[1] < -5.0 {
        ctx.set_position([ctx.state_get("_checkpoint"), 1.3, 0.0]);
        ctx.set_velocity([0.0, 0.0, 0.0]);
        ctx.state_set("Rettungen", ctx.state_get("Rettungen") + 1);
        ctx.state_set("_coyote", 0);
        ctx.state_set("_jump_buffer", 0.0);
        ctx.log("Der Wind hat dich aufgefangen. Weiter geht's!");
        return;
    }
    let coyote = ctx.state_get("_coyote");
    let velocity = ctx.velocity();
    let grounded = ctx.grounded() && velocity[1] < 0.5;
    if grounded { coyote = 10; } else if coyote > 0 { coyote -= 1; }
    let pressed = ctx.input("action") > 0.5 || ctx.input("jump") > 0.5;
    let buffer = ctx.state_get("_jump_buffer") - dt;
    if pressed && !ctx.state_get("_held") { buffer = 0.18; }
    let speed = if grounded { 3.2 } else { 3.0 };
    let acceleration = if grounded { 24.0 } else { 12.0 };
    for axis in [0, 2] {
        let target = ctx.input(if axis == 0 { "move_x" } else { "move_z" }) * speed;
        let change = target - velocity[axis];
        velocity[axis] += change.max(-acceleration * dt).min(acceleration * dt);
    }
    if buffer > 0.0 && coyote > 0 {
        velocity[1] = 5.0;
        coyote = 0;
        buffer = 0.0;
        ctx.state_set("Sprünge", ctx.state_get("Sprünge") + 1);
    }
    ctx.state_set("_held", pressed);
    ctx.state_set("_coyote", coyote);
    ctx.state_set("_jump_buffer", buffer.max(0.0));
    ctx.set_velocity(velocity);
    if ctx.has_animator() {
        ctx.animator_set("speed", velocity[0].abs() + velocity[2].abs());
        ctx.animator_set("grounded", grounded && velocity[1] < 0.5);
    }
    for crystal in ctx.find_by_tag("crystal") {
        let target = ctx.entity_position(crystal);
        let dx = p[0] - target[0]; let dy = p[1] - target[1]; let dz = p[2] - target[2];
        if dx * dx + dy * dy + dz * dz < 1.2 {
            ctx.remove(crystal);
            ctx.state_set("score", ctx.state_get("score") + 1);
            ctx.state_set("_checkpoint", target[0]);
            ctx.log("Kristall gerettet · neuer Wind-Anker!");
        }
    }
    let portal = ctx.entity_position(ctx.find_by_name("Portal"));
    let dx = p[0] - portal[0]; let dz = p[2] - portal[2];
    if ctx.state_get("score") == 3 && dx * dx + dz * dz < 0.8 && p[1] < 2.1 {
        ctx.state_set("won", true);
    }
}
