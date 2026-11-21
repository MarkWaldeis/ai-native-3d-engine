use engine_runtime::Simulation;
use serde_json::{json, Value};
use shared::scene::Scene;
use std::collections::BTreeMap;
use engine_mcp::ToolHandler;

fn scene() -> Scene {
    serde_json::from_value(json!({"entities":[
        {"id":1,"name":"player","kind":"box","size":[1,1,1],"script":{"source":r#"
            fn on_start(ctx) {
                ctx.state_set("starts",1); ctx.state_set("steps",0);
                ctx.flag_set("quest",true); ctx.inventory_add("key",3);
                ctx.state_set("spawn",ctx.spawn_entity(#{kind:"box",size:[1,1,1]}));
                ctx.remove(2);
            }
            fn on_load(ctx) {ctx.state_set("loaded",true);}
            fn on_tick(ctx,dt) {
                ctx.state_set("steps",ctx.state_get("steps")+1);
                let p=ctx.position(); ctx.set_position([p[0]+1.0,p[1],p[2]]);
            }
        "#}},
        {"id":2,"name":"removed","kind":"box","size":[1,1,1]},
        {"id":3,"name":"unchanged","kind":"box","size":[1,1,1]}
    ]})).unwrap()
}

#[test]
fn delta_roundtrip_and_hooks() {
    let base = scene();
    let mut sim = Simulation::from_sources(base.clone(), BTreeMap::new()).unwrap();
    sim.step(5, &[]).unwrap();
    let save = sim.save_game().unwrap();
    println!("SAVE_EXAMPLE={save}");
    assert_eq!(save["delta"]["removed"], json!([2]));
    assert_eq!(save["delta"]["modified"].as_array().unwrap().len(),1);
    assert_eq!(save["delta"]["spawned"].as_array().unwrap().len(),1);
    assert_eq!(save,sim.save_game().unwrap());
    let mut loaded = Simulation::load_game_from_sources(base, BTreeMap::new(), save.clone()).unwrap();
    assert!(!loaded.base_changed);
    assert_eq!(loaded.state()["state"]["loaded"],true);
    assert_eq!(loaded.state()["state"]["starts"],1);
    sim.step(7,&[]).unwrap(); loaded.step(7,&[]).unwrap();
    for key in ["entities","flags","inventory"] { assert_eq!(sim.state()[key],loaded.state()[key]); }
    assert_eq!(sim.state()["state"]["steps"],loaded.state()["state"]["steps"]);
    let mut changed = scene(); changed.entities[2].name="updated".into();
    let updated = Simulation::load_game_from_sources(changed,BTreeMap::new(),save).unwrap();
    assert!(updated.base_changed);
    assert_eq!(updated.scene().entities.iter().find(|e|e.id==3).unwrap().name,"updated");
}

#[test]
fn empty_large_static_world_and_invalid_saves() {
    let entity = scene().entities[2].clone();
    let mut base=Scene::default();
    for id in 1..=2000 { let mut e=entity.clone(); e.id=id; base.entities.push(e); }
    let sim=Simulation::from_sources(base.clone(),BTreeMap::new()).unwrap();
    let save=sim.save_game().unwrap();
    assert_eq!(save["delta"],json!({"modified":[],"removed":[],"spawned":[]}));
    assert!(save.to_string().len()*100 < serde_json::to_string(&base).unwrap().len());
    assert!(Simulation::load_game_from_sources(base.clone(),BTreeMap::new(),save.clone()).is_ok());
    for bad in [json!({}),{let mut s=save.clone();s["version"]=json!(9);s},
        {let mut s=save.clone();s["inventory"]=json!({"key":-1});s},
        {let mut s=save.clone();s["delta"]["modified"]=json!([{}]);s},
        {let mut s=save.clone();s["state"]=json!({"huge":"x".repeat(4*1024*1024)});s}] {
        assert!(Simulation::load_game_from_sources(base.clone(),BTreeMap::new(),bad).is_err());
    }
}

#[test]
fn mcp_roundtrip_and_confinement() {
    use ai_engine::Engine;
    fn call(e:&mut Engine,name:&str,args:Value)->Value {
        let r=engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string(),e).unwrap();
        let result=&r["result"];
        assert_ne!(result["isError"],true,"{r}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let dir=root.join("target/savegame-test");std::fs::create_dir_all(&dir).unwrap();
    let mut e=Engine::new();e.assets_dir=root.join("assets");e.world.scene=scene();
    call(&mut e,"play",json!({}));call(&mut e,"step",json!({"ticks":4}));
    let save=call(&mut e,"save_game",json!({"path":"target/savegame-test/save.json"}));
    for path in ["../escape.json","target/savegame-test/save.txt"] {
        assert!(e.handle("save_game",&json!({"path":path})).is_err());
    }
    assert!(e.handle("load_game",&json!({"save":save})).is_err());
    call(&mut e,"stop",json!({}));
    let loaded=call(&mut e,"load_game",json!({"path":"target/savegame-test/save.json"}));
    assert_eq!(loaded["tick"],4);assert_eq!(loaded["inventory"],save["inventory"]);
    call(&mut e,"stop",json!({}));
    call(&mut e,"load_game",json!({"save":save}));
}

#[test]
fn dynamic_velocities_resume() {
    let base: Scene=serde_json::from_value(json!({"entities":[{"id":1,"name":"falling","kind":"box","size":[1,1,1],"transform":{"position":[0,50,0]},"rigidbody":{"type":"dynamic"},"collider":{"shape":"box","half_extents":[0.5,0.5,0.5]},"script":{"source":"fn on_start(ctx){ctx.set_velocity([2.0,3.0,0.0]);}"}}]})).unwrap();
    let mut sim=Simulation::from_sources(base.clone(),BTreeMap::new()).unwrap();
    sim.step(10,&[]).unwrap();
    let save=sim.save_game().unwrap();
    assert_eq!(save["bodies"].as_array().unwrap().len(),1);
    let mut loaded=Simulation::load_game_from_sources(base,BTreeMap::new(),save).unwrap();
    sim.step(10,&[]).unwrap(); loaded.step(10,&[]).unwrap();
    for (a,b) in sim.scene().entities[0].transform.position.iter().zip(loaded.scene().entities[0].transform.position) { assert!((*a-b).abs()<0.001,"{a} != {b}"); }
}

#[test]
fn inventory_and_flag_empty_and_error_cases() {
    let mut base=scene();base.entities.truncate(1);
    base.entities[0].script=Some(serde_json::from_value(json!({"source":r#"fn on_start(ctx) {
        if ctx.flag("absent") != () {throw "missing flag";}
        if ctx.inventory_count("key") != 0 {throw "missing item";}
        if ctx.inventory_remove("key",1) {throw "insufficient item";}
        ctx.inventory_add("key",2);
        if !ctx.inventory_remove("key",1) {throw "remove failed";}
        ctx.flag_set("done",true);
    }"#})).unwrap());
    let sim=Simulation::from_sources(base.clone(),BTreeMap::new()).unwrap();
    assert_eq!(sim.state()["inventory"]["key"],1);
    for source in ["fn on_start(ctx){ctx.inventory_add(\"key\",-1);}","fn on_start(ctx){ctx.inventory_remove(\"key\",-1);}"] {
        base.entities[0].script=Some(serde_json::from_value(json!({"source":source})).unwrap());
        assert!(Simulation::from_sources(base.clone(),BTreeMap::new()).is_err());
    }
}

#[test]
fn restored_asset_and_script_paths_are_confined() {
    let base = Scene::default();
    let save = Simulation::from_sources(base.clone(), BTreeMap::new()).unwrap().save_game().unwrap();
    for path in ["../outside.glb", "/outside.glb", "C:\\outside.glb"] {
        for field in ["asset", "script"] {
            let mut bad = save.clone();
            let mut entity = json!({"id":1,"name":"bad","kind":"box","size":[1,1,1]});
            if field == "asset" { entity = json!({"id":1,"name":"bad","kind":"gltf","path":path}); }
            else { entity["script"] = json!({"path":path}); }
            bad["delta"]["spawned"] = json!([entity]); bad["next_id"] = json!(2);
            let err = Simulation::load_game_from_sources(base.clone(), BTreeMap::new(), bad).err().unwrap();
            assert!(err.to_string().contains("project-relative"), "{err:#}");
        }
    }
    let mut base = scene(); base.entities.truncate(1); base.entities[0].script = None;
    let mut save = Simulation::from_sources(base.clone(), BTreeMap::new()).unwrap().save_game().unwrap();
    save["delta"]["modified"] = json!([{"id":1,"kind":"gltf","path":"../outside.glb"}]);
    assert!(Simulation::load_game_from_sources(base, BTreeMap::new(), save).is_err());
}

#[test]
fn damaged_enemy_health_and_local_memos_survive_loading() {
    let base: Scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"enemy","kind":"box","size":[1,1,1],"tags":["enemy"],"script":{"source":r#"
            fn on_start(ctx) { ctx.set_memo("route",#{point:3}); }
            fn on_load(ctx) {ctx.state_set("memo",ctx.memo("route"));ctx.state_set("health",ctx.health());}
        "#}},
        {"id":2,"name":"attacker","kind":"box","size":[1,1,1],"script":{"source":"fn on_start(ctx){ctx.damage(1,25.0);}"}}
    ]})).unwrap();
    let sim = Simulation::from_sources(base.clone(), BTreeMap::new()).unwrap();
    let save = sim.save_game().unwrap();
    let loaded = Simulation::load_game_from_sources(base.clone(), BTreeMap::new(), save.clone()).unwrap();
    assert_eq!(loaded.state()["state"]["health"],75.0);
    assert_eq!(loaded.state()["state"]["memo"],json!({"point":3}));
    assert_eq!(loaded.save_game().unwrap()["combat"],save["combat"]);
    let mut legacy=save;legacy.as_object_mut().unwrap().remove("combat");
    assert!(Simulation::load_game_from_sources(base,BTreeMap::new(),legacy).is_ok());
}

#[test]
fn pending_spawned_script_gets_its_start_exactly_once() {
    let base: Scene=serde_json::from_value(json!({"entities":[{"id":1,"name":"spawner","kind":"box","size":[1,1,1],"script":{"source":r#"
        fn on_start(ctx) {ctx.state_set("spawned",false);}
        fn on_load(ctx) {ctx.flag_set("existing_loaded",true);}
        fn on_tick(ctx,dt) {
            if !ctx.state_get("spawned") {
                ctx.spawn_entity(#{kind:"box",size:[1,1,1],script:#{source:"fn on_start(ctx){ctx.inventory_add(\"token\",1);} fn on_load(ctx){ctx.flag_set(\"child_loaded\",true);}"}});
                ctx.state_set("spawned",true);
            }
        }
    "#}}]})).unwrap();
    let mut sim=Simulation::from_sources(base.clone(),BTreeMap::new()).unwrap();
    sim.step(1,&[]).unwrap();
    let save=sim.save_game().unwrap();assert_eq!(save["pending_starts"],json!([2]));
    let mut loaded=Simulation::load_game_from_sources(base.clone(),BTreeMap::new(),save).unwrap();
    assert_eq!(loaded.state()["inventory"]["token"],1);
    assert_eq!(loaded.state()["flags"]["existing_loaded"],true);
    assert!(loaded.state()["flags"].get("child_loaded").is_none());
    loaded.step(1,&[]).unwrap();sim.step(1,&[]).unwrap();
    assert_eq!(loaded.state()["inventory"],sim.state()["inventory"]);
    let loaded=Simulation::load_game_from_sources(base,BTreeMap::new(),loaded.save_game().unwrap()).unwrap();
    assert_eq!(loaded.state()["inventory"]["token"],1);
    assert_eq!(loaded.state()["flags"]["child_loaded"],true);
}

#[test]
fn oversized_runtime_and_mcp_saves_return_errors() {
    let source = format!("fn on_start(ctx) {{let chunk=\"{}\";for n in 0..256 {{ctx.state_set(\"large_\"+n,chunk);}}}}", "x".repeat(16384));
    let base: Scene=serde_json::from_value(json!({"entities":[{"id":1,"name":"large-state","kind":"box","size":[1,1,1],"script":{"source":source}}]})).unwrap();
    let sim=Simulation::from_sources(base.clone(),BTreeMap::new()).unwrap();
    assert!(sim.save_game().unwrap_err().to_string().contains("4 MB"));
    let mut engine=ai_engine::Engine::new();engine.world.scene=base;
    engine.handle("play",&json!({})).unwrap();
    assert!(engine.handle("save_game",&json!({})).unwrap_err().contains("4 MB"));
}
