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

fn blast(ctx, p) {
    fx(ctx,"explosion",p,[0.0,1.0,0.0]);
    for id in ctx.find_by_tag("blast_prop") {
        if id == ctx.id() { continue; }
        let q=ctx.entity_position(id);
        let dx=q[0]-p[0]; let dy=q[1]-p[1]+0.75; let dz=q[2]-p[2];
        let d=sqrt(dx*dx+dy*dy+dz*dz);
        if d > 0.01 && d < 14.0 {
            let strength=120.0*(1.0-d/14.0)/d;
            ctx.apply_impulse(id,[dx*strength,dy*strength,dz*strength]);
        }
    }
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
    let home=ctx.position(); let cover=(); let best=45.0*45.0;
    for id in ctx.find_by_tag("cover") {
        let p=ctx.entity_position(id);
        let dx=p[0]-home[0]; let dz=p[2]-home[2];
        let d=dx*dx+dz*dz;
        if d < best { best=d; cover=p; }
    }
    let commander=ctx.find_by_name("commander") == ctx.id();
    ctx.set_health(if commander {250.0} else {100.0});
    ctx.set_memo("ai",#{hp:if commander {250} else {100},home:home,group:group(ctx),
        commander:commander,cover:cover,mode:"patrol",sight:false,last_seen:-1000,
        waypoint:0,cooldown:0.7+rnd(ctx.id())*0.6,cover_time:0.0,cover_cd:0.0,dead_time:0.0});
}

fn on_tick(ctx,dt) {
    let m=ctx.memo("ai");
    let damage=ctx.hits().len;
    m.hp=ctx.health();
    if m.hp <= 0 {
        if m.mode != "dead" {
            m.mode="dead"; record_death(ctx,m);
            let cfg=ctx.prop("ai_physics");
            if type_of(cfg) != "map" || cfg.ragdoll == () {
                let p=ctx.position(); p[1]-=0.5; ctx.set_position(p);
                ctx.set_rotation([90.0,0.0,0.0]);
                fx(ctx,"blood",p,[0.0,1.0,0.0]);
            }
        }
        m.dead_time+=dt;
        ctx.set_memo("ai",m);
        if m.dead_time >= 5.0 { ctx.remove(ctx.id()); }
        return;
    }
    if ctx.state_get("won") || ctx.state_get("lost") { return; }
    let target=player_position(ctx);
    if target == () { ctx.set_memo("ai",m); return; }
    let p=ctx.position();
    let dx=target[0]-p[0]; let dz=target[2]-p[2];
    let d=sqrt(dx*dx+dz*dz);
    let tick=ctx.state_get("_tick");
    if tick == () { tick=0; }
    let eye=[p[0],p[1]+1.5,p[2]];
    let chest=[target[0],target[1]+0.3,target[2]];
    if tick % 8 == ctx.id() % 8 {
        m.sight=d < 130.0 && line_of_sight(ctx,eye,chest,ctx.find_by_name("player"));
        if m.sight { m.last_seen=tick; }
    }
    let shot=ctx.state_get("_shot");
    let noise=false;
    if shot != () {
        let sx=p[0]-shot[0]; let sz=p[2]-shot[1];
        noise=tick-shot[2] < 60 && sx*sx+sz*sz < 8100.0;
    }
    if damage > 0 || noise || m.sight {
        if m.mode == "patrol" { m.mode="alert"; }
        if damage > 0 || noise { m.last_seen=tick; }
    } else if tick-m.last_seen > 360 && m.mode != "cover" { m.mode="patrol"; }
    m.cover_cd=max(0.0,m.cover_cd-dt);
    if m.hp < 55 && m.cover != () && m.mode == "alert" && m.cover_cd <= 0.0 {
        m.mode="cover"; m.cover_time=0.0; m.cover_cd=8.0;
    }
    if m.mode == "cover" {
        p=walk(ctx,p,m.cover,4.5,dt);
        let cx=p[0]-m.cover[0]; let cz=p[2]-m.cover[2];
        if cx*cx+cz*cz < 1.0 { m.cover_time+=dt; }
        if m.cover_time >= 3.0 { m.mode="alert"; }
    } else if m.mode == "patrol" {
        let angle=m.waypoint.to_float()*1.57079632679+rnd(ctx.id())*6.28318530718;
        let wp=[m.home[0]+cos(angle)*15.0,m.home[1],m.home[2]+sin(angle)*15.0];
        p=walk(ctx,p,wp,1.6,dt);
        let wx=p[0]-wp[0]; let wz=p[2]-wp[2];
        if wx*wx+wz*wz < 1.0 { m.waypoint=(m.waypoint+1)%4; }
    } else {
        if d > 30.0 { p=walk(ctx,p,target,3.8,dt); }
        else if d > 0.1 {
            let side=if (tick/120+ctx.id())%2 == 0 {1.0} else {-1.0};
            p=walk(ctx,p,[p[0]-dz/d*side,p[1],p[2]+dx/d*side],0.8,dt);
        }
        ctx.set_rotation([0.0,atan(-dx,-dz).to_degrees(),0.0]);
    }
    ctx.set_position(p);
    m.cooldown-=dt;
    if m.mode != "patrol" && m.cooldown <= 0.0 && m.sight && d < 130.0 {
        if line_of_sight(ctx,[p[0],p[1]+1.5,p[2]],chest,ctx.find_by_name("player")) {
            if rnd(tick*97+ctx.id()) < clamp(0.8-d/160.0,0.12,0.7) { ctx.damage(ctx.player_id(),7.0); }
            fx(ctx,"muzzle",eye,[dx,0.0,dz]); fx(ctx,"tracer",eye,[chest[0]-eye[0],chest[1]-eye[1],chest[2]-eye[2]]);
        }
        m.cooldown=0.7+rnd(tick+ctx.id()*31)*0.6;
    }
    ctx.set_memo("ai",m);
}
