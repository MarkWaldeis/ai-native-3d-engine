use super::*;
use serde_json::json;

fn simulation() -> Simulation {
    let scene = json!({"entities":[
        {"id":1,"name":"near","kind":"box","size":[1,1,1],"script":{"source":"fn on_tick(ctx,dt) {ctx.state_set(\"near\",true);}"}},
        {"id":2,"name":"far","kind":"box","size":[1,1,1],"transform":{"position":[1000,20,0]},"rigidbody":{"type":"dynamic"},"script":{"source":"fn on_start(ctx) {ctx.set_velocity([3.0,0.0,0.0]);ctx.state_set(\"far\",0);} fn on_tick(ctx,dt) {ctx.state_set(\"far\",ctx.state_get(\"far\")+1);}"}},
        {"id":3,"name":"always","kind":"box","size":[1,1,1],"transform":{"position":[1000,20,0]},"tags":["always_active"],"script":{"source":"fn on_tick(ctx,dt) {ctx.state_set(\"always\",true);}"}},
        {"id":4,"name":"player","kind":"box","size":[1,1,1],"transform":{"position":[1000,20,0]},"tags":["player"],"script":{"source":"fn on_tick(ctx,dt) {ctx.state_set(\"player\",true);}"}},
        {"id":5,"name":"kinematic","kind":"box","size":[1,1,1],"transform":{"position":[1000,30,0]},"rigidbody":{"type":"kinematic"},"script":{"source":"fn on_start(ctx) {ctx.set_velocity([2.0,0.0,0.0]);}"}},
        {"id":6,"name":"ground","kind":"box","size":[100,1,100],"transform":{"position":[1000,-0.5,0]},"collider":{"shape":"box"}}
    ]});
    Simulation::from_sources(serde_json::from_value(scene).unwrap(),BTreeMap::new()).unwrap()
}

#[test]
fn region_freezes_far_hooks_and_bodies_then_resumes_with_velocity() {
    let mut simulation = simulation();
    simulation.set_active_region(0.,0.,100.,10.).unwrap();
    let initial = simulation.scene();
    simulation.step(5,&[]).unwrap();
    let state = simulation.state();
    assert_eq!(state["state"]["far"],0);
    for key in ["near","always","player"] { assert_eq!(state["state"][key],true); }
    for id in [2,5] {
        assert!(!simulation.entity_active(id));
        let now = simulation.scene().entities.into_iter().find(|e| e.id == id).unwrap();
        assert_eq!(now.transform,initial.entities.iter().find(|e| e.id == id).unwrap().transform);
    }
    assert!(simulation.entity_active(6));
    simulation.set_active_region(1000.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert_eq!(simulation.state()["state"]["far"],1);
    for id in [2,5] {
        assert!(simulation.entity_active(id));
        assert!(simulation.scene().entities.iter().find(|e| e.id == id).unwrap().transform.position[0] > 1000.);
    }
}

#[test]
fn default_and_cleared_regions_simulate_everything() {
    let mut simulation = simulation();
    simulation.step(1,&[]).unwrap();
    assert_eq!(simulation.state()["state"]["far"],1);
    simulation.set_active_region(0.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert_eq!(simulation.state()["state"]["far"],1);
    simulation.clear_active_region();
    simulation.step(1,&[]).unwrap();
    assert_eq!(simulation.state()["state"]["far"],2);
}

#[test]
fn region_hysteresis_and_invalid_arguments_are_transactional() {
    let mut simulation = simulation();
    assert!(simulation.set_active_region(f32::NAN,0.,100.,10.).is_err());
    assert!(simulation.set_active_region(0.,0.,0.,10.).is_err());
    assert!(simulation.set_active_region(0.,0.,100.,-1.).is_err());
    simulation.set_active_region(0.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert!(!simulation.entity_active(2));
    simulation.set_active_region(899.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert!(!simulation.entity_active(2));
    simulation.set_active_region(905.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert!(simulation.entity_active(2));
    simulation.set_active_region(899.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert!(simulation.entity_active(2));
}

#[test]
fn empty_region_scene_ticks() {
    let mut simulation = Simulation::from_sources(Scene::default(),BTreeMap::new()).unwrap();
    simulation.set_active_region(0.,0.,100.,10.).unwrap();
    simulation.step(1,&[]).unwrap();
    assert_eq!(simulation.tick,1);
}

#[test]
fn region_activation_is_deterministic() {
    let run = || {
        let mut simulation = simulation();
        for x in [0.,1000.,0.,1000.] {
            simulation.set_active_region(x,0.,100.,10.).unwrap();
            simulation.step(3,&[]).unwrap();
        }
        simulation.state()
    };
    assert_eq!(run(),run());
}
