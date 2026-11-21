// Reference first-person player: movement from look-relative inputs, hitscan rifle, reload.
fn on_start(ctx) {
    ctx.state_set("health", 100.0);
    ctx.state_set("ammo", 30.0);
    ctx.state_set("ammo_reserve", 120.0);
    ctx.state_set("kills", 0.0);
    ctx.state_set("objective", "Alle Gegner ausschalten");
    ctx.state_set("_cooldown", 0.0);
    ctx.state_set("_reload", 0.0);
    ctx.state_set("_shot", 0.0);
    ctx.state_set("won", false);
}

fn on_tick(ctx, dt) {
    let cfg = ctx.prop("ai_fps");
    let v = ctx.velocity();
    if ctx.health() <= 0.0 {
        ctx.set_velocity([0.0, v[1], 0.0]);
        ctx.state_set("objective", "Gefallen – Neustart");
        return;
    }
    let speed = if ctx.input("sprint") > 0.5 { cfg.sprint_speed } else { cfg.move_speed };
    let m = ctx.move_dir();
    let vy = v[1];
    if ctx.input("jump") > 0.5 && ctx.grounded() { vy = cfg.jump_speed; }
    ctx.set_velocity([m[0] * speed, vy, m[2] * speed]);

    let cooldown = ctx.state_get("_cooldown") - dt;
    let reload = ctx.state_get("_reload");
    let ammo = ctx.state_get("ammo");
    let reserve = ctx.state_get("ammo_reserve");
    if reload > 0.0 {
        reload -= dt;
        if reload <= 0.0 {
            let take = 30.0 - ammo;
            if take > reserve { take = reserve; }
            ammo += take;
            reserve -= take;
            reload = 0.0;
        }
    } else if ctx.input("reload") > 0.5 && ammo < 30.0 && reserve > 0.0 {
        reload = 1.6;
    } else if ctx.input("fire") > 0.5 && cooldown <= 0.0 {
        if ammo > 0.0 {
            ammo -= 1.0;
            cooldown = 0.1;
            ctx.state_set("_shot", ctx.state_get("_shot") + 1.0);
            fire(ctx, cfg);
        } else if reserve > 0.0 {
            reload = 1.6;
        }
    }
    ctx.state_set("_cooldown", if cooldown < 0.0 { 0.0 } else { cooldown });
    ctx.state_set("_reload", reload);
    ctx.state_set("ammo", ammo);
    ctx.state_set("ammo_reserve", reserve);
    let left = ctx.find_by_tag("enemy").len();
    if left == 0 && !ctx.state_get("won") {
        ctx.state_set("won", true);
        ctx.state_set("objective", "Gebiet gesichert");
    } else if left > 0 {
        ctx.state_set("objective", "Gegner ausschalten: " + left);
    }
}

fn fire(ctx, cfg) {
    let p = ctx.position();
    let eye = [p[0], p[1] + cfg.eye_height, p[2]];
    let dir = ctx.aim_dir();
    let range = 250.0;
    let ray = ctx.raycast(eye, dir, range);
    ctx.emit_fx("muzzle", eye, dir);
    let distance = if ray.hit { ray.distance } else { range };
    ctx.emit_fx("tracer", eye, [dir[0] * distance, dir[1] * distance, dir[2] * distance]);
    if !ray.hit { return; }
    let left = ctx.damage(ray.id, 34.0);
    if left < 0.0 {
        ctx.emit_fx("impact", ray.point, ray.normal);
        return;
    }
    ctx.emit_fx("blood", ray.point, ray.normal);
    if left == 0.0 {
        ctx.state_set("kills", ctx.state_get("kills") + 1.0);
    }
}
