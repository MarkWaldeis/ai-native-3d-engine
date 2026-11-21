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
    let p = ();
    try { p = ctx.player_pos(); } catch { p = (); }
    if p == () {
        let id = ctx.find_by_name("player");
        if id < 0 { return (); }
        p = ctx.entity_position(id);
    }
    p
}

fn rnd(seed) {
    let h = abs((seed * 1103515245 + 12345) % 2147483647);
    h = abs((h * 48271) % 2147483647);
    h.to_float() / 2147483647.0
}

fn fx(ctx, kind, pos, dir) {
    try { ctx.emit_fx(kind, pos, dir); } catch { }
}

// True when nothing blocks the segment a->b (raycast if available, else terrain samples).
fn line_of_sight(ctx, a, b, target_id) {
    let dx = b[0] - a[0]; let dy = b[1] - a[1]; let dz = b[2] - a[2];
    let dist = sqrt(dx * dx + dy * dy + dz * dz);
    if dist < 0.01 { return true; }
    let dir = [dx / dist, dy / dist, dz / dist];
    let hit = ();
    try { hit = ctx.raycast(a, dir, dist + 1.0); } catch { hit = (); }
    if hit != () {
        if !hit.hit { return true; }
        return hit.id == target_id || hit.distance > dist - 0.8;
    }
    for i in 1..6 {
        let t = i.to_float() / 6.0;
        if analytic_height(a[0] + dx * t, a[2] + dz * t) > a[1] + dy * t - 0.3 { return false; }
    }
    true
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
                 ["_tick",0],["_stage",0],["_tower_hp",300],["_pdmg",0]] {
        ctx.state_set(pair[0],pair[1]);
    }
    ctx.state_set("won",false);
    ctx.state_set("lost",false);
    ctx.state_set("_hurt",false);
    ctx.state_set("_commander_dead",false);
    ctx.state_set("_vy",0.0);
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
}

fn fire(ctx, eye, dir, tick) {
    ctx.state_set("ammo",ctx.state_get("ammo")-1);
    ctx.state_set("_shot",[eye[0],eye[2],tick]);
    ctx.state_set("_fire_cd",0.1);
    let enemies=ctx.state_get("_enemies");
    let tower=ctx.find_by_name("radio-tower");
    let hit=();
    try { hit=ctx.raycast(eye,dir,400.0); } catch { }
    let target=-1; let distance=400.0;
    if hit != () {
        if hit.hit {
            distance=hit.distance;
            if hit.id == tower || enemies.contains(hit.id) { target=hit.id; }
        }
    } else {
        let candidates=enemies;
        if tower >= 0 { candidates.push(tower); }
        for id in candidates {
            let m=ctx.state_get("_e"+id);
            if m != () && m.hp <= 0 { continue; }
            let p=();
            try { p=ctx.entity_position(id); } catch { continue; }
            let radius=0.6;
            if id == tower { p[1]+=8.0; radius=3.5; }
            else if ctx.state_get("_drones").contains(id) { radius=0.8; }
            else { p[1]+=1.0; }
            let dx=p[0]-eye[0]; let dy=p[1]-eye[1]; let dz=p[2]-eye[2];
            let along=dx*dir[0]+dy*dir[1]+dz*dir[2];
            let discriminant=radius*radius-(dx*dx+dy*dy+dz*dz-along*along);
            if along <= 0.0 || discriminant < 0.0 { continue; }
            let entry=max(0.0,along-sqrt(discriminant));
            if entry < distance {
                target=id; distance=entry;
            }
        }
        if target >= 0 {
            let end=[eye[0]+dir[0]*distance,eye[1]+dir[1]*distance,eye[2]+dir[2]*distance];
            if !line_of_sight(ctx,eye,end,target) { target=-1; }
        }
    }
    let end=[eye[0]+dir[0]*distance,eye[1]+dir[1]*distance,eye[2]+dir[2]*distance];
    fx(ctx,"muzzle",eye,dir);
    fx(ctx,"tracer",eye,end);
    if target == tower && tower >= 0 {
        ctx.state_set("_tower_hp",ctx.state_get("_tower_hp")-34);
        fx(ctx,"impact",end,dir);
    } else if target >= 0 {
        let key="_d"+target;
        ctx.state_set(key,number(ctx,key)+34);
        fx(ctx,"blood",end,dir);
    } else if distance < 400.0 { fx(ctx,"impact",end,dir); }
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
            let m=ctx.state_get("_e"+enemy);
            if m != () && m.hp > 0 { ctx.state_set("_wp",ctx.entity_position(enemy)); break; }
        }
        if left == 0 { stage=2; }
    } else if stage == 2 {
        ctx.state_set("objective","Sichere die Akte in San Aurelio");
        if id < 0 { stage=3; }
        else if near_xz(p,ctx.entity_position(id),3.0) { ctx.remove(id); stage=3; }
    } else if stage == 3 {
        ctx.state_set("objective","Zerstöre den Funkturm ("+max(0,ctx.state_get("_tower_hp"))+" LP)");
        if id < 0 { stage=4; }
        else if ctx.state_get("_tower_hp") <= 0 {
            fx(ctx,"explosion",ctx.entity_position(id),[0.0,1.0,0.0]);
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
    if ctx.state_get("won") || ctx.state_get("lost") { return; }
    let tick=ctx.state_get("_tick")+1;
    ctx.state_set("_tick",tick);
    let damage=ctx.state_get("_pdmg");
    ctx.state_set("_pdmg",0);
    if damage > 0 {
        ctx.state_set("health",max(0,ctx.state_get("health")-damage));
        ctx.state_set("_hurt",true);
    }
    if ctx.state_get("health") <= 0 {
        ctx.state_set("lost",true);
        ctx.state_set("objective","Gefallen – Neustart");
        return;
    }
    let p=ctx.position();
    let yaw=ctx.input("look_yaw"); let pitch=ctx.input("look_pitch");
    let mx=ctx.input("move_x"); let mz=ctx.input("move_z");
    let length=max(1.0,sqrt(mx*mx+mz*mz));
    let speed=5.5;
    if ctx.input("sprint") > 0.0 { speed=9.0; }
    p[0]=clamp(p[0]+(cos(yaw)*mx+sin(yaw)*mz)*speed*dt/length,-3260.0,3260.0);
    p[2]=clamp(p[2]+(-sin(yaw)*mx+cos(yaw)*mz)*speed*dt/length,-3260.0,3260.0);
    let floor=ground_height(ctx,p[0],p[2])+0.9;
    let vy=ctx.state_get("_vy");
    if p[1] <= floor+0.03 && vy <= 0.0 {
        p[1]=floor; vy=0.0;
        if ctx.input("jump") > 0.0 { vy=5.0; }
    }
    vy-=9.81*dt; p[1]=max(floor,p[1]+vy*dt);
    if p[1] <= floor { vy=0.0; }
    ctx.state_set("_vy",vy);
    ctx.set_position(p);
    ctx.set_rotation([0.0,yaw.to_degrees(),0.0]);
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
        fire(ctx,[p[0],p[1]+0.75,p[2]],[-sin(yaw)*cos(pitch),sin(pitch),-cos(yaw)*cos(pitch)],tick);
    }
    if tick % 6 == 0 {
        for tag in ["pickup_health","pickup_ammo"] {
            for id in ctx.find_by_tag(tag) {
                if near_xz(p,ctx.entity_position(id),2.2) {
                    if tag == "pickup_health" { ctx.state_set("health",min(100,ctx.state_get("health")+40)); }
                    else { ctx.state_set("ammo_reserve",ctx.state_get("ammo_reserve")+60); }
                    ctx.remove(id);
                }
            }
        }
    }
    if tick % 10 == 0 { missions(ctx,p); }
}
