fn on_start(ctx) {
    ctx.state_set("score", 0);
    ctx.state_set("won", false);
    ctx.state_set("gate_open", false);
    ctx.state_set("_held", false);
    ctx.state_set("_respawns", 0);
    ctx.state_set("_spawn", ctx.position());
}

fn on_tick(ctx, dt) {
    let p = ctx.position();
    if p[1] < -4.0 || abs(p[0]) > 7.0 || abs(p[2]) > 6.0 {
        ctx.set_position(ctx.state_get("_spawn"));
        ctx.set_velocity([0.0, 0.0, 0.0]);
        ctx.state_set("_respawns", ctx.state_get("_respawns") + 1);
        return;
    }
    let v = ctx.velocity();
    ctx.set_velocity([ctx.input("move_x") * 2.5, v[1], ctx.input("move_z") * 2.5]);
    let pressed = ctx.input("action") > 0.5;
    if pressed && !ctx.state_get("_held") {
        for lantern in ctx.find_by_tag("lantern") {
            let target = ctx.entity_position(lantern);
            let dx = p[0] - target[0];
            let dz = p[2] - target[2];
            if dx*dx + dz*dz < 0.81 {
                ctx.remove(lantern);
                let count = ctx.state_get("score") + 1;
                ctx.spawn_entity(#{name: "Licht-" + count.to_string(), kind: "sphere", radius: 0.18, segments: 12,
                    transform: #{position: target}, color: [1.0, 0.71, 0.15, 1.0], tags: ["lit_lantern", "floating"]});
                ctx.state_set("score", count);
                ctx.log("Eine Laterne leuchtet!");
                break;
            }
        }
    }
    ctx.state_set("_held", pressed);
    if ctx.state_get("score") == 4 && !ctx.state_get("gate_open") {
        ctx.remove(ctx.find_by_name("Tor-Riegel"));
        ctx.state_set("gate_open", true);
        ctx.log("Alle Laternen leuchten. Das Tor ist offen!");
    }
    if ctx.state_get("gate_open") {
        let goal = ctx.entity_position(ctx.find_by_name("Ausgang"));
        let dx = p[0] - goal[0];
        let dz = p[2] - goal[2];
        if dx*dx + dz*dz < 0.3025 {
            ctx.state_set("won", true);
        }
    }
}
