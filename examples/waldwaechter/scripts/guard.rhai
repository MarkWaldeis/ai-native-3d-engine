fn on_start(ctx) {
    let p = ctx.position();
    ctx.nav_set_target([6.0, 0.0, p[2]]);
}

fn on_tick(ctx, dt) {
    if ctx.nav_path().len == 0 {
        let p = ctx.position();
        let x = if p[0] > 0.0 { -6.0 } else { 46.0 };
        if ctx.nav_set_target([x, 0.0, p[2]]) {
            ctx.state_set("_patrol_visits", ctx.state_get("_patrol_visits") + 1);
        }
    }
    if ctx.has_animator() {
        ctx.animator_set("speed", 1.2);
        ctx.animator_set("grounded", true);
    }
}
