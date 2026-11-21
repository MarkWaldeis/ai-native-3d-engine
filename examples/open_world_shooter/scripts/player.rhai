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

fn number(ctx, key) {
    let n = ctx.state_get(key);
    if n == () { return 0; }
    n
}

fn near_xz(a, b, radius) {
    let dx = a[0] - b[0]; let dz = a[2] - b[2];
    dx * dx + dz * dz <= radius * radius
}

fn clamp(value, lo, hi) {
    min(hi, max(lo, value))
}

fn on_start(ctx) {
    for pair in [["health",100],["ammo",30],["ammo_reserve",150],["kills",0],
                 ["_tick",0],["_stage",0],["_tower_hp",300]] {
        ctx.state_set(pair[0],pair[1]);
    }
    ctx.state_set("won",false);
    ctx.state_set("lost",false);
    ctx.state_set("_hurt",false);
    ctx.state_set("_commander_dead",false);
    ctx.set_health(100.0);
    let tower=ctx.find_by_name("radio-tower");
    if tower >= 0 { ctx.set_health(tower,300.0); }
    ctx.state_set("_fire_cd",0.0);
    ctx.state_set("_reload",0.0);
    ctx.state_set("_shot",[0.0,0.0,-1000]);
    ctx.state_set("_enemies",ctx.find_by_tag("enemy"));
    ctx.state_set("_drones",ctx.find_by_tag("drone"));
    for g in ["alpha","bravo","charlie","town","citadel","patrol"] {
        ctx.state_set("_tot_"+g,ctx.find_by_tag("grp_"+g).len);
        ctx.state_set("_dead_"+g,0);
    }
    ctx.state_set("objective","Erreiche die Brücke über den Rio Verde");
    ctx.state_set("_wp",ctx.position());
    ctx.set_memo("has_terrain",ctx.find_by_tag("terrain").len > 0);
    let safe=ctx.position();
    safe[0]=clamp(safe[0],-3260.0,3260.0);
    safe[2]=clamp(safe[2],-3260.0,3260.0);
    safe[1]=max(safe[1],terrain_floor(ctx,safe)+0.9);
    ctx.set_memo("last_safe",safe);
}

fn terrain_floor(ctx,p) {
    if ctx.memo("has_terrain") { ctx.terrain_height(p[0],p[2]) }
    else { analytic_height(p[0],p[2]) }
}

fn fire(ctx, eye, dir, tick) {
    ctx.state_set("ammo",ctx.state_get("ammo")-1);
    ctx.state_set("_shot",[eye[0],eye[2],tick]);
    ctx.state_set("_fire_cd",0.1);
    let hit=ctx.raycast(eye,dir,400.0);
    let distance=if hit.hit { hit.distance } else { 400.0 };
    ctx.emit_fx("muzzle",eye,dir);
    ctx.emit_fx("tracer",eye,[dir[0]*distance,dir[1]*distance,dir[2]*distance]);
    if hit.hit {
        let health=ctx.damage(hit,34.0,[dir[0]*16.0,dir[1]*16.0,dir[2]*16.0]);
        ctx.emit_fx(if health >= 0.0 { "blood" } else { "impact" },hit.point,hit.normal);
    }
}

fn setting(cfg,key,fallback) {
    if type_of(cfg) != "map" { return fallback; }
    let value=cfg[key];
    if type_of(value) == "f64" || type_of(value) == "i64" { value.to_float() } else { fallback }
}

fn missions(ctx,p) {
    let stage=ctx.state_get("_stage");
    let name="";
    if stage == 0 { name="bridge-marker"; }
    if stage == 2 { name="intel"; }
    if stage == 3 { name="radio-tower"; }
    if stage == 4 { name="commander"; }
    let id=ctx.find_by_name(name);
    if id >= 0 { ctx.state_set("_wp",ctx.entity_position(id)); }
    if stage == 0 {
        ctx.state_set("objective","Erreiche die Brücke über den Rio Verde");
        if id < 0 || near_xz(p,ctx.entity_position(id),40.0) { stage=1; }
    } else if stage == 1 {
        let left=max(0,ctx.state_get("_tot_alpha")-ctx.state_get("_dead_alpha"));
        ctx.state_set("objective","Säubere Außenposten Alpha ("+left+" übrig)");
        for enemy in ctx.find_by_tag("grp_alpha") {
            if ctx.health_of(enemy) > 0.0 { ctx.state_set("_wp",ctx.entity_position(enemy)); break; }
        }
        if left == 0 { stage=2; }
    } else if stage == 2 {
        ctx.state_set("objective","Sichere die Akte in San Aurelio");
        if id < 0 { stage=3; }
        else if near_xz(p,ctx.entity_position(id),3.0) { ctx.remove(id); stage=3; }
    } else if stage == 3 {
        if id >= 0 { ctx.state_set("_tower_hp",ctx.health_of(id)); }
        ctx.state_set("objective","Zerstöre den Funkturm ("+max(0,ctx.state_get("_tower_hp"))+" LP)");
        if id < 0 { stage=4; }
        else if ctx.state_get("_tower_hp") <= 0 {
            blast(ctx,ctx.entity_position(id));
            ctx.remove(id); stage=4;
        }
    } else if stage == 4 {
        ctx.state_set("objective","Eliminiere den Kommandanten der Zitadelle");
        if id < 0 || ctx.state_get("_commander_dead") { stage=5; }
    }
    if stage == 5 {
        ctx.state_set("won",true);
        ctx.state_set("objective","Zitadelle eingenommen!");
    }
    ctx.state_set("_stage",stage);
}

fn on_tick(ctx,dt) {
    let p=ctx.position();
    let x=clamp(p[0],-3260.0,3260.0);
    let z=clamp(p[2],-3260.0,3260.0);
    if p[0] != x || p[2] != z {
        p=[x,p[1],z];
        ctx.set_position(p);
    }
    let floor=terrain_floor(ctx,p);
    if p[1] < floor-20.0 {
        ctx.set_position(ctx.memo("last_safe"));
        ctx.set_velocity([0.0,0.0,0.0]);
        return;
    }
    if ctx.grounded() && p[1] >= floor { ctx.set_memo("last_safe",p); }
    if ctx.state_get("won") || ctx.state_get("lost") {
        ctx.set_velocity([0.0,ctx.velocity()[1],0.0]);
        return;
    }
    let tick=ctx.state_get("_tick")+1;
    ctx.state_set("_tick",tick);
    if ctx.hits().len > 0 { ctx.state_set("_hurt",true); }
    if ctx.health() <= 0.0 {
        ctx.state_set("lost",true);
        ctx.state_set("objective","Gefallen – Neustart");
        ctx.set_velocity([0.0,ctx.velocity()[1],0.0]);
        return;
    }
    let cfg=ctx.prop("ai_fps");
    let velocity=ctx.velocity();
    let vy=velocity[1];
    if ctx.input("jump") > 0.5 && ctx.grounded() { vy=setting(cfg,"jump_speed",5.0); }
    let speed=if ctx.input("sprint") > 0.5 { setting(cfg,"sprint_speed",9.0) } else { setting(cfg,"move_speed",5.5) };
    let move=ctx.move_dir();
    let vx=clamp(move[0]*speed,(-3260.0-p[0])/dt,(3260.0-p[0])/dt);
    let vz=clamp(move[2]*speed,(-3260.0-p[2])/dt,(3260.0-p[2])/dt);
    ctx.set_velocity([vx,vy,vz]);
    let reload=ctx.state_get("_reload");
    if reload > 0.0 {
        reload=max(0.0,reload-dt);
        if reload < 0.000001 { reload=0.0; }
        if reload == 0.0 {
            let count=min(30-ctx.state_get("ammo"),ctx.state_get("ammo_reserve"));
            ctx.state_set("ammo",ctx.state_get("ammo")+count);
            ctx.state_set("ammo_reserve",ctx.state_get("ammo_reserve")-count);
        }
    } else if (ctx.input("reload") > 0.0 || ctx.state_get("ammo") == 0)
              && ctx.state_get("ammo") < 30 && ctx.state_get("ammo_reserve") > 0 {
        reload=2.0;
    }
    ctx.state_set("_reload",reload);
    ctx.state_set("_fire_cd",max(0.0,ctx.state_get("_fire_cd")-dt));
    if ctx.input("fire") > 0.0 && reload == 0.0 && ctx.state_get("_fire_cd") < 0.000001 && ctx.state_get("ammo") > 0 {
        fire(ctx,[p[0],p[1]+setting(cfg,"eye_height",0.75),p[2]],ctx.aim_dir(),tick);
    }
    if tick % 6 == 0 {
        for tag in ["pickup_health","pickup_ammo"] {
            for id in ctx.find_by_tag(tag) {
                if near_xz(p,ctx.entity_position(id),2.2) {
                    if tag == "pickup_health" { ctx.set_health(min(100.0,ctx.health()+40.0)); }
                    else { ctx.state_set("ammo_reserve",ctx.state_get("ammo_reserve")+60); }
                    ctx.remove(id);
                }
            }
        }
    }
    if tick % 10 == 0 { missions(ctx,p); }
}
