// Reference enemy: chases with steering, fires when it has line of sight, dies at 0 health.
// Far enemies are paused by the web player's simulation region (sim_radius) and cost nothing.
fn on_tick(ctx, dt) {
    if ctx.health() <= 0.0 {
        ctx.emit_fx("explosion", ctx.position(), [0.0, 1.0, 0.0]);
        ctx.remove(ctx.id());
        return;
    }
    let target = ctx.player_pos();
    if target.len() == 0 { return; }
    let player = ctx.player_id();
    let distance = ctx.distance_to(target);
    let cooldown = ctx.memo("cooldown");
    if cooldown == () { cooldown = 1.0 + ctx.id() % 5 * 0.2; }
    cooldown -= dt;
    let v = ctx.velocity();
    if ctx.health_of(player) > 0.0 && distance < 40.0 && ctx.can_see(player, 40.0) {
        ctx.set_velocity([0.0, v[1], 0.0]);
        if cooldown <= 0.0 {
            cooldown = 1.1;
            let me = ctx.position();
            let dir = [target[0] - me[0], target[1] - me[1], target[2] - me[2]];
            ctx.emit_fx("muzzle", me, dir);
            ctx.emit_fx("tracer", me, dir);
            let damage = 12.0 - distance * 0.2;
            if damage < 3.0 { damage = 3.0; }
            ctx.damage(player, damage);
        }
    } else if distance < 90.0 {
        ctx.set_velocity(ctx.steer(target, 3.2));
    } else {
        ctx.set_velocity([0.0, v[1], 0.0]);
    }
    ctx.set_memo("cooldown", cooldown);
}
