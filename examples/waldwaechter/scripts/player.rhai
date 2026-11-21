fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.state_set("won", false);
    ctx.state_set("Erwischt", 0);
    ctx.state_set("_checkpoint", 0.0);
    ctx.state_set("_shield", 0);
    ctx.state_set("_held", false);
    ctx.state_set("_patrol_visits", 0);
    ctx.log("Fünf Relikte warten am goldenen Waldpfad. Halte Abstand zu den Wächtern!");
}

fn on_tick(ctx, dt) {
    if ctx.state_get("won") { ctx.set_velocity([0.0, 0.0, 0.0]); return; }
    let p = ctx.position();
    let shield = ctx.state_get("_shield");
    if shield > 0 { ctx.state_set("_shield", shield - 1); }
    let caught = p[1] < -4.0 || abs(p[0]) > 250.0 || abs(p[2]) > 250.0;
    if shield == 0 {
        for guard in ctx.find_by_tag("guard") {
            let q = ctx.entity_position(guard);
            let dx = p[0] - q[0]; let dz = p[2] - q[2];
            if dx*dx + dz*dz < 3.24 { caught = true; }
        }
    }
    if caught {
        ctx.set_position([ctx.state_get("_checkpoint"), 0.85, 0.0]);
        ctx.set_velocity([0.0, 0.0, 0.0]);
        ctx.state_set("Erwischt", ctx.state_get("Erwischt") + 1);
        ctx.state_set("_shield", 90);
        ctx.log("Erwischt! Zurück zum Wald-Anker. Deine Relikte bleiben erhalten.");
        return;
    }
    let v = ctx.velocity();
    let x = ctx.input("move_x"); let z = ctx.input("move_z");
    let length = (x*x + z*z).sqrt();
    if length > 1.0 { x /= length; z /= length; }
    v[0] = x * 4.0; v[2] = z * 4.0;
    let pressed = ctx.input("action") > 0.5;
    if pressed && !ctx.state_get("_held") && ctx.grounded() { v[1] = 5.0; }
    ctx.state_set("_held", pressed);
    ctx.set_velocity(v);
    if ctx.has_animator() {
        ctx.animator_set("speed", abs(v[0]) + abs(v[2]));
        ctx.animator_set("grounded", ctx.grounded() && v[1] < 0.5);
    }
    for relic in ctx.find_by_tag("relic") {
        let q = ctx.entity_position(relic);
        let dx = p[0] - q[0]; let dz = p[2] - q[2];
        if dx*dx + dz*dz < 1.3225 {
            ctx.remove(relic);
            let score = ctx.state_get("score") + 1;
            ctx.state_set("score", score);
            if score >= 3 { ctx.state_set("_checkpoint", q[0]); }
            ctx.log("Relikt geborgen: " + score.to_string() + " / 5");
        }
    }
    if ctx.state_get("score") == 5 { ctx.state_set("won", true); }
}
