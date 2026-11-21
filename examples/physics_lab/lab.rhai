fn on_start(ctx) {
    ctx.set_memo("start_position", ctx.position());
    ctx.set_memo("start_rotation", ctx.rotation());
    ctx.state_set("Hitbox", "—");
    ctx.state_set("Health", 100);
    ctx.state_set("Sensor", "waiting");
}

fn on_tick(ctx, dt) {
    let ball = ctx.find_by_name("Drop ball");
    let gate = ctx.find_by_name("Sensor gate");
    let gate_pos = ctx.entity_position(gate);
    let overlap = ctx.overlap(#{ origin: gate_pos, shape: #{ shape: "box", half_extents: [0.7, 0.05, 0.7] } });
    if overlap.ids.contains(ball) {
        ctx.state_set("Sensor", "crossed");
    }
    let pressed = ctx.input("action") > 0.0;
    let fresh = pressed && ctx.memo("held") != true;
    ctx.set_memo("held", pressed);
    if !fresh { return; }
    let target = ctx.id();
    if ctx.ragdoll_active(target) {
        ctx.set_position(ctx.memo("start_position"));
        ctx.set_rotation(ctx.memo("start_rotation"));
        ctx.set_ragdoll(target, false);
        ctx.set_health(target, 100.0);
        ctx.state_set("Hitbox", "—");
        ctx.state_set("Health", 100);
        return;
    }
    let p = ctx.entity_position(target);
    let hit = ctx.raycast([p[0], p[1] + 0.75, p[2] - 4.0], [0.0, 0.0, 1.0], 5.0);
    if hit.hit && hit.id == target {
        ctx.state_set("Hitbox", hit.hitbox);
        ctx.state_set("Health", ctx.damage(hit, 60.0, [0.0, 0.8, 1.5]));
    }
}
