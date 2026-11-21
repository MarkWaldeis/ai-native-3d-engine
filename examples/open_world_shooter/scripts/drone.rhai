// Mirror of tools/open_world_shooter/terrain_fn.py; used when ctx.terrain_height is unavailable.
fn smooth(e0, e1, x) {
    let t = (x - e0) / (e1 - e0);
    if t < 0.0 { t = 0.0; }
    if t > 1.0 { t = 1.0; }
    t * t * (3.0 - 2.0 * t)
}

fn analytic_height(x, z) {
    let h = 32.0 + 16.0 * sin(x / 760.0 + 0.7) * cos(z / 910.0 - 0.3)
        + 8.0 * sin((x - z) / 430.0 + 2.1)
        + 3.5 * sin(x / 131.0 + 1.0) * sin(z / 157.0 + 0.4)
        + 1.2 * sin(x / 37.0) * sin(z / 41.0);
    for p in [[-2700.0, -2700.0, 70.0, 30.0, 160.0], [-300.0, -2100.0, 90.0, 26.0, 180.0],
              [900.0, -400.0, 380.0, 24.0, 260.0], [-1700.0, 1200.0, 90.0, 40.0, 200.0],
              [2200.0, -1900.0, 90.0, 34.0, 180.0], [2200.0, 2200.0, 210.0, 78.0, 300.0]] {
        let d = hypot(x - p[0], z - p[1]);
        if d < p[2] + p[4] {
            let w = smooth(p[2] + p[4], p[2], d);
            h = h + (p[3] - h) * w;
        }
    }
    let rx = -900.0 + 420.0 * sin(z / 900.0) + 160.0 * sin(z / 370.0 + 1.0);
    let d = abs(x - rx);
    if d < 200.0 {
        let w = smooth(200.0, 22.0, d);
        h = h + (-3.0 - h) * w;
    }
    let edge = max(abs(x), abs(z));
    if edge > 3316.625 - 520.0 {
        h += 110.0 * smooth(3316.625 - 520.0, 3316.625, edge);
    }
    h
}

fn ground_height(ctx, x, z) {
    let h = ();
    let available = ctx.state_get("_has_terrain");
    if available == () {
        available = ctx.find_by_tag("terrain").len > 0;
        ctx.state_set("_has_terrain", available);
    }
    if available { try { h = ctx.terrain_height(x, z); } catch { h = (); } }
    if h == () { h = analytic_height(x, z); }
    // Bridge decks and roads over the river sit at water + 3.6.
    let rx = -900.0 + 420.0 * sin(z / 900.0) + 160.0 * sin(z / 370.0 + 1.0);
    if abs(x - rx) < 62.0 && (abs(z + 1700.0) < 4.5 || abs(z - 900.0) < 4.5) {
        h = max(h, 4.2);
    }
    h
}

fn player_position(ctx) {
    ctx.player_pos()
}

fn rnd(seed) {
    let h = abs((seed * 1103515245 + 12345) % 2147483647);
    h = abs((h * 48271) % 2147483647);
    h.to_float() / 2147483647.0
}

fn fx(ctx, kind, pos, dir) {
    ctx.emit_fx(kind, pos, dir);
}

fn line_of_sight(ctx, a, b, target_id) {
    ctx.line_of_sight(a, b, target_id)
}

fn group(ctx) {
    for g in ["alpha","bravo","charlie","town","citadel","patrol"] {
        if ctx.find_by_tag("grp_"+g).contains(ctx.id()) { return g; }
    }
    "patrol"
}

fn add(ctx,key,amount) {
    let value=ctx.state_get(key);
    if value == () { value=0; }
    ctx.state_set(key,value+amount);
}

fn record_death(ctx,m) {
    add(ctx,"kills",1);
    add(ctx,"_dead_"+m.group,1);
    if m.commander { ctx.state_set("_commander_dead",true); }
}

fn walk(ctx,p,target,speed,dt) {
    let dx=target[0]-p[0]; let dz=target[2]-p[2];
    let d=sqrt(dx*dx+dz*dz);
    if d > 0.05 {
        let step=min(d,speed*dt);
        p[0]+=dx/d*step; p[2]+=dz/d*step;
        ctx.set_rotation([0.0,atan(-dx,-dz).to_degrees(),0.0]);
    }
    p[1]=ground_height(ctx,p[0],p[2]);
    p
}

fn clamp(value, lo, hi) {
    min(hi, max(lo, value))
}

fn on_start(ctx) {
    ctx.set_health(60.0);
    ctx.set_memo("ai",#{hp:60,home:ctx.position(),group:group(ctx),commander:false,
        dead:false,angle:rnd(ctx.id())*6.28318530718,cooldown:1.5,burst:0,alert:false});
}

fn on_tick(ctx,dt) {
    let m=ctx.memo("ai");
    m.hp=ctx.health();
    let p=ctx.position();
    if m.hp <= 0 {
        if !m.dead { m.dead=true; record_death(ctx,m); fx(ctx,"explosion",p,[0.0,1.0,0.0]); }
        p[1]-=8.0*dt;
        ctx.set_memo("ai",m);
        if p[1] <= ground_height(ctx,p[0],p[2]) { ctx.remove(ctx.id()); }
        else { ctx.set_position(p); }
        return;
    }
    if ctx.state_get("won") || ctx.state_get("lost") { return; }
    let player=player_position(ctx);
    if player == () { ctx.set_memo("ai",m); return; }
    let dx=player[0]-p[0]; let dz=player[2]-p[2];
    let dist=sqrt(dx*dx+dz*dz);
    let tick=ctx.state_get("_tick");
    if tick == () { tick=0; }
    if tick%8 == ctx.id()%8 {
        m.alert=dist < 150.0 && line_of_sight(ctx,p,player,ctx.find_by_name("player"));
    }
    if m.alert {
        if dist > 25.0 { p=walk(ctx,p,player,6.0,dt); }
        p[1]=ground_height(ctx,p[0],p[2])+10.0;
        ctx.set_rotation([0.0,atan(-dx,-dz).to_degrees(),0.0]);
    } else {
        m.angle+=6.0/40.0*dt;
        p[0]=m.home[0]+cos(m.angle)*40.0; p[2]=m.home[2]+sin(m.angle)*40.0;
        p[1]=ground_height(ctx,p[0],p[2])+14.0;
    }
    ctx.set_position(p);
    m.cooldown-=dt;
    if m.alert && m.cooldown <= 0.0 {
        if line_of_sight(ctx,p,player,ctx.find_by_name("player")) {
            if rnd(tick*83+ctx.id()) < clamp(0.7-dist/200.0,0.1,0.6) { ctx.damage(ctx.player_id(),5.0); }
            fx(ctx,"muzzle",p,[dx,0.0,dz]); fx(ctx,"tracer",p,[player[0]-p[0],player[1]-p[1],player[2]-p[2]]);
        }
        m.burst+=1;
        if m.burst >= 3 { m.burst=0; m.cooldown=1.5; } else { m.cooldown=0.12; }
    }
    ctx.set_memo("ai",m);
}
