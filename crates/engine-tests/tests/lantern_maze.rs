use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::Value;
use std::path::Path;

#[test]
fn lantern_maze_requires_nearby_action_and_opens_gate_only_after_four_lights() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lantern_maze");
    let mut engine = Engine::from_project(&root).unwrap();
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("validation.json")).unwrap())
            .unwrap();
    for (name, case) in cases.as_object().unwrap() {
        let result = engine.handle("simulate", case).unwrap();
        let report: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["passed"], true, "{name}: {report}");
    }
}
