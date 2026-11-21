use ai_engine::Engine;
use engine_core::{
    query_index::{components, COMPONENTS},
    Command, World,
};
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use shared::scene::{Scene, Transform};
use std::{path::Path, sync::Arc};

fn scene() -> Scene {
    serde_json::from_value(json!({"entities":(1..=1000).rev().map(|id| {
        let mut e=json!({"id":id,"name":format!("entity-{id}"),"kind":"box","size":[1,1,1],"tags":[if id%2==0 {"even"} else {"odd"},"all","all"],"transform":{"position":[id as f32-500.0,0.0,id as f32%41.0-20.0]}});
        if id%3==0 {e["collider"]=json!({"shape":"box"});}
        if id%5==0 {e["props"]=json!({"_prefab":{"name":"house","instance":id}});}
        e
    }).collect::<Vec<_>>()})).unwrap()
}
fn query(engine: &mut Engine, args: Value) -> Value {
    let result = engine.handle("query_entities", &args).unwrap();
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn posting_intersections_match_exhaustive_selection_and_pages_keep_their_contract() {
    let mut e = Engine::new();
    e.world.replace_authoring_scene(scene()).unwrap();
    for tag in [None, Some("all"), Some("even"), Some("missing")] {
        for component in [None, Some("mesh"), Some("collider")] {
            for prefab in [None, Some("house"), Some("missing")] {
                for region in [None, Some((-1, 0)), Some((0, -1)), Some((100, 100))] {
                    let index = e.world.authoring_index();
                    let actual: Vec<_> = index
                        .candidates(tag, component, prefab, region)
                        .into_iter()
                        .map(|i| e.world.scene.entities[i].id)
                        .collect();
                    let mut expected: Vec<_> = e
                        .world
                        .scene
                        .entities
                        .iter()
                        .filter(|entity| {
                            let p = entity.transform.position;
                            tag.is_none_or(|t| entity.tags.iter().any(|v| v == t))
                                && component.is_none_or(|c| components(entity).contains(&c))
                                && prefab.is_none_or(|name| {
                                    entity.props["_prefab"]["name"].as_str() == Some(name)
                                })
                                && region.is_none_or(|r| {
                                    r == (
                                        (f64::from(p[0]) / 128.0).floor() as i64,
                                        (f64::from(p[2]) / 128.0).floor() as i64,
                                    )
                                })
                        })
                        .map(|entity| entity.id)
                        .collect();
                    expected.sort_unstable();
                    assert_eq!(actual, expected);
                }
            }
        }
    }
    let first = query(
        &mut e,
        json!({"tag":"even","component":"collider","prefab":"house","limit":2}),
    );
    assert_eq!(first["total"], 33);
    assert_eq!(first["entities"][0]["id"], 30);
    assert_eq!(first["entities"][1]["id"], 60);
    let second = query(
        &mut e,
        json!({"tag":"even","component":"collider","prefab":"house","limit":2,"cursor":first["next_cursor"]}),
    );
    assert_eq!(second["total"], 33);
    assert_eq!(second["entities"][0]["id"], 90);
    let bounded = query(
        &mut e,
        json!({"tag":"even","bbox":{"min":[-10,-1,-30],"max":[10,1,30]},"limit":100}),
    );
    assert_eq!(bounded["total"], 11);
    let custom = query(
        &mut e,
        json!({"region":{"x":-1,"z":0,"chunk_size":100},"limit":500}),
    );
    assert_eq!(
        custom["total"].as_u64().unwrap(),
        e.world
            .scene
            .entities
            .iter()
            .filter(|entity| {
                let p = entity.transform.position;
                (p[0] / 100.0).floor() == -1.0 && (p[2] / 100.0).floor() == 0.0
            })
            .count() as u64
    );
    let empty = query(&mut e, json!({"tag":"missing"}));
    assert_eq!(empty["total"], 0);
    assert_eq!(empty["entities"], json!([]));
    assert!(empty["next_cursor"].is_null());
    assert!(e
        .handle("query_entities", &json!({"component":"unknown"}))
        .is_err());
    assert!(e.handle("query_entities", &json!({"limit":0})).is_err());
    let revision = e.world.revision;
    e.world
        .apply(Command::SetColor {
            id: 1,
            color: [1.0; 4],
        })
        .unwrap();
    assert_eq!(e.world.revision, revision + 1);
    assert!(e.handle("query_entities",&json!({"tag":"even","component":"collider","prefab":"house","cursor":first["next_cursor"]})).unwrap_err().contains("stale"));
}

#[test]
fn cache_detects_direct_edits_without_revision_changes_and_reads_live_display_fields() {
    let mut e = Engine::new();
    e.world.replace_authoring_scene(scene()).unwrap();
    let revision = e.world.revision;
    let first = e.world.authoring_index();
    assert!(Arc::ptr_eq(&first, &e.world.authoring_index()));
    e.world.scene.entities[0].name = "renamed".into();
    assert!(Arc::ptr_eq(&first, &e.world.authoring_index()));
    let last = query(
        &mut e,
        json!({"tag":"even","prefab":"house","near":{"point":[500,0,-4],"radius":10}}),
    );
    assert!(last["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == "renamed"));
    e.world.scene.entities[0].tags.push("new".into());
    assert!(!Arc::ptr_eq(&first, &e.world.authoring_index()));
    assert_eq!(query(&mut e, json!({"tag":"new"}))["total"], 1);
    e.world.scene.entities[0].transform.position = [-1.0, 0.0, -1.0];
    assert_eq!(
        query(&mut e, json!({"tag":"new","region":{"x":-1,"z":-1}}))["total"],
        1
    );
    e.world.scene.entities[0].props["_prefab"]["name"] = json!("new-prefab");
    assert_eq!(query(&mut e, json!({"prefab":"new-prefab"}))["total"], 1);
    e.world.scene.entities[0].collider =
        Some(serde_json::from_value(json!({"shape":"box"})).unwrap());
    assert_eq!(
        query(&mut e, json!({"tag":"new","component":"collider"}))["total"],
        1
    );
    e.world.scene.entities.reverse();
    assert_eq!(
        query(&mut e, json!({"tag":"new"}))["entities"][0]["id"],
        1000
    );
    e.world.scene.entities.retain(|entity| entity.id != 1000);
    assert_eq!(query(&mut e, json!({"tag":"new"}))["total"], 0);
    assert_eq!(
        e.world.revision, revision,
        "queries and direct embedding edits must not invent authoring revisions"
    );
    let mut empty = Engine::new();
    assert_eq!(query(&mut empty, json!({}))["total"], 0);
}

#[test]
fn cloned_candidates_rollback_and_restart_never_restore_stale_postings() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<World>();
    let mut world = World::new();
    world.replace_authoring_scene(scene()).unwrap();
    let revision = world.revision;
    let index = world.authoring_index();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| world.authoring_index());
        let b = scope.spawn(|| world.authoring_index());
        assert!(Arc::ptr_eq(&a.join().unwrap(), &b.join().unwrap()));
    });
    let mut candidate = world.clone();
    assert!(Arc::ptr_eq(&index, &candidate.authoring_index()));
    let mut transform = Transform::default();
    transform.position = [9999.0, 0.0, 9999.0];
    candidate
        .apply(Command::SetTransform { id: 1, transform })
        .unwrap();
    assert!(!Arc::ptr_eq(&index, &candidate.authoring_index()));
    assert!(Arc::ptr_eq(&index, &world.authoring_index()));
    world.commit_atomic(candidate);
    let changed = world.authoring_index();
    assert_eq!(
        changed
            .candidates(Some("odd"), None, None, Some((78, 78)))
            .len(),
        1
    );
    world.rollback_to(revision).unwrap();
    assert!(!Arc::ptr_eq(&changed, &world.authoring_index()));
    assert!(world
        .authoring_index()
        .candidates(None, None, None, Some((78, 78)))
        .is_empty());
    let restored =
        World::from_checkpoint(&world.checkpoint(), Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert_eq!(
        restored
            .authoring_index()
            .candidates(Some("even"), Some("collider"), Some("house"), None)
            .len(),
        33
    );
    let mut entity = world.scene.entities[0].clone();
    entity.collider = Some(serde_json::from_value(json!({"shape":"box"})).unwrap());
    entity.rigidbody = Some(Default::default());
    entity.script =
        Some(serde_json::from_value(json!({"source":"fn on_tick(ctx,dt) {}"})).unwrap());
    entity.nav_agent = Some(Default::default());
    let sky: Scene = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sky_garden/scene.json"),
        )
        .unwrap(),
    )
    .unwrap();
    entity.animator = sky.entities.iter().find_map(|e| e.animator.clone());
    assert_eq!(components(&entity), COMPONENTS);
    assert!(index.payload_bytes() > 0);
}
