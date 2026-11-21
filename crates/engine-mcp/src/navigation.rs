use serde_json::{json, Value};
pub fn tools() -> Vec<Value> {
    [("nav_bake","Bake bounded, radius-eroded tiled navmesh from static collision geometry; incremental tile reuse after edits. Params: radius,height,step,max_slope_deg,cell_size,tile_size,bounds [min_x,min_z,max_x,max_z]. Stores immutable project asset and scene snapshot. Honors global locks and CAS.",vec![]),
     ("nav_query_path","Deterministic A* polygon path with corridor string-pulling. start/goal are foot positions; unreachable/outside points fail, never snap across walls.",vec!["start","goal"]),
     ("nav_lint","Report goals and collectibles unreachable from player or explicit start using baked navigation.",vec![])]
    .into_iter().map(|(name,description,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","required":required,"properties":{
        "params":{"type":"object"},"start":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"goal":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"agent":{"type":"string"},"expected_revision":{"type":"integer"}
    }}})).collect()
}
