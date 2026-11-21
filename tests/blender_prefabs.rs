use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    let result = engine
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn real_blender_collections_lods_hash_dedup_propagation_and_portable_bundle() {
    if asset_pipeline::find_blender().is_none() {
        eprintln!("SKIP: Blender collection/LOD test requires Blender");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "blender-prefabs-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    ai_engine::project::initialize(&root, "empty").unwrap();
    let project = Project(root.canonicalize().unwrap());
    let mut engine = Engine::from_project(&project.0).unwrap();
    call(&mut engine, "blender_start", json!({}));
    call(
        &mut engine,
        "blender_exec",
        json!({"code":r#"
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
collection=bpy.data.collections.new('Cabin')
for level, size in [(0,2.0),(1,1.5),(2,1.0)]:
    bpy.ops.mesh.primitive_cube_add(size=size)
    obj=bpy.context.object;obj.name='Cabin_LOD'+str(level)
    for parent in list(obj.users_collection):parent.objects.unlink(obj)
    collection.objects.link(obj)
    obj['ai_tags']='["building"]'
fixture=bpy.data.objects.new('Fixture',None);collection.objects.link(fixture)
fixture.parent=bpy.data.objects['Cabin_LOD0'];fixture.location.x=1
bpy.ops.mesh.primitive_cube_add(size=.5)
detail=bpy.context.object;detail.name='Detail'
for parent in list(detail.users_collection):parent.objects.unlink(detail)
collection.objects.link(detail);detail.parent=fixture;detail.location.z=2
collection.instance_offset.x=.5
for name,x in [('A',0),('B',10)]:
    instance=bpy.data.objects.new(name,None);instance.instance_type='COLLECTION';instance.instance_collection=collection
    bpy.context.scene.collection.objects.link(instance);instance.location.x=x
bpy.ops.mesh.primitive_cube_add(size=3)
original=bpy.context.object;original.name='Linked'
copy=original.copy();copy.data=original.data;bpy.context.collection.objects.link(copy);copy.name='LinkedCopy';copy.location.y=10
"#}),
    );
    let sync = call(&mut engine, "blender_sync", json!({}));
    assert_eq!(engine.world.scene.prefabs.len(), 1);
    assert_eq!(engine.world.scene.entities.len(), 10);
    assert_eq!(sync["ids"].as_array().unwrap().len(), 10);
    let mut positions: Vec<_> = engine
        .world
        .scene
        .entities
        .iter()
        .filter(|e| e.name == "Detail")
        .map(|e| e.transform.position)
        .collect();
    positions.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(positions, vec![[0.5, 2., 0.], [10.5, 2., 0.]]);
    let prefab = engine.world.scene.prefabs.keys().next().unwrap().clone();
    let root_id = engine
        .world
        .scene
        .entities
        .iter()
        .find(|e| e.props["_prefab"]["instance"] == e.id && e.source_uid.is_some())
        .unwrap()
        .id;
    let linked: Vec<_> = engine
        .world
        .scene
        .entities
        .iter()
        .filter(|e| e.name.starts_with("Linked"))
        .collect();
    assert_eq!(linked[0].mesh, linked[1].mesh);
    let paths: Vec<_> = std::fs::read_dir(project.0.join("assets/blender"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "glb"))
        .collect();
    assert_eq!(paths.len(), 5);
    assert!(paths.iter().all(|p| p.file_stem().unwrap().len() == 64));
    let first = engine.world.scene.clone();
    call(&mut engine, "blender_sync", json!({}));
    assert_eq!(engine.world.scene, first);
    assert_eq!(
        std::fs::read_dir(project.0.join("assets/blender"))
            .unwrap()
            .count(),
        5
    );
    let members: Vec<_> = engine
        .world
        .scene
        .entities
        .iter()
        .filter(|e| e.props["_prefab"]["instance"] == root_id)
        .map(|e| e.id)
        .collect();
    call(
        &mut engine,
        "claim_entities",
        json!({"ids":members,"agent":"owner"}),
    );
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['Cabin_LOD0'].data.vertices[0].co.x -= .25"}),
    );
    let revision = engine.world.revision;
    assert!(engine
        .handle("blender_sync", &json!({"agent":"other"}))
        .unwrap_err()
        .contains("locked"));
    assert_eq!(engine.world.revision, revision);
    call(&mut engine, "blender_sync", json!({"agent":"owner"}));
    assert_ne!(engine.world.scene.prefabs[&prefab], first.prefabs[&prefab]);
    assert_eq!(engine.world.scene.entities.len(), 10);
    let model = engine
        .world
        .scene
        .entities
        .iter()
        .find(|e| e.props["_prefab"]["name"] == prefab && e.props.get("web_lods").is_some())
        .unwrap();
    assert!(model.props["web_lods"]["1"]
        .as_str()
        .unwrap()
        .ends_with(".glb"));
    assert!(Path::new(model.props["web_lods"]["2"].as_str().unwrap()).is_file());
    let pkg = project.0.join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    std::fs::write(pkg.join("engine_web.js"), "export default function(){}").unwrap();
    std::fs::write(pkg.join("engine_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let out = project.0.join("site");
    let report =
        ai_engine::web::export_bundle(&engine.world.scene, &project.0, &out, &pkg).unwrap();
    assert_eq!(report["assets"], 5);
    let exported: shared::scene::Scene =
        serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    assert!(exported.prefabs.is_empty());
    assert!(exported
        .entities
        .iter()
        .all(|e| e.props.get("_prefab").is_none()));
    for e in &exported.entities {
        if let Some(lods) = e.props["web_lods"].as_object() {
            for path in lods.values() {
                assert!(out.join(path.as_str().unwrap()).is_file());
            }
        }
    }
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects.remove(bpy.data.objects['A'],do_unlink=True)"}),
    );
    call(
        &mut engine,
        "blender_sync",
        json!({"prune":true,"agent":"owner"}),
    );
    assert_eq!(engine.world.scene.entities.len(), 6);
    println!("PASS real Blender: collection prefabs, inherited geometry updates, LOD0/1/2, linked-mesh SHA-256 dedup, locks, prune and portable deduplicated export");
}
