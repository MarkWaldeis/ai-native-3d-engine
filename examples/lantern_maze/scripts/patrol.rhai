fn on_start(ctx) {
    ctx.state_set("_nav_patrol_stop", 0);
    ctx.state_set("_nav_patrol_visits", 0);
    ctx.nav_set_target([-4.9, 0.0, -3.3]);
}

fn on_tick(ctx, dt) {
    if ctx.nav_path().len == 0 {
        let points = [[-4.9, 0.0, -3.3], [-0.8, 0.0, -3.3], [-0.8, 0.0, 3.3], [2.5, 0.0, 3.3]];
        let stop = (ctx.state_get("_nav_patrol_stop") + 1) % 4;
        if ctx.nav_set_target(points[stop]) {
            ctx.state_set("_nav_patrol_stop", stop);
            ctx.state_set("_nav_patrol_visits", ctx.state_get("_nav_patrol_visits") + 1);
        }
    }
}
