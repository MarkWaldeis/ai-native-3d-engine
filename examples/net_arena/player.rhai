fn on_start(ctx) {
    ctx.state_set("score_" + ctx.id().to_string(), 0);
}

fn on_tick(ctx, dt) {
    let velocity = ctx.velocity();
    ctx.set_velocity([ctx.input("move_x") * 2.0, velocity[1], ctx.input("move_z") * 2.0]);
    let position = ctx.position();
    if position[0].abs() < 1.5 {
        ctx.state_set("score_" + ctx.id().to_string(), 1);
    }
    if ctx.id() == 2 {
        let ticks = ctx.state_get("_spawn_ticks");
        if ticks == () { ticks = 0; }
        if ticks % 180 == 0 {
            let old = ctx.state_get("_spawn_box");
            if old != () { ctx.remove(old); }
            let id = ctx.spawn_entity(#{name:"Netz-Bonus",kind:"box",size:[0.6,0.6,0.6],
                tags:["net_spawn"],transform:#{position:[0.0,4.0,6.0]},
                color:[1.0,0.75,0.1,1.0],collider:#{shape:"box"},rigidbody:#{type:"dynamic",mass:0.2}});
            ctx.state_set("_spawn_box",id);
        }
        ctx.state_set("_spawn_ticks",ticks+1);
    }
}
