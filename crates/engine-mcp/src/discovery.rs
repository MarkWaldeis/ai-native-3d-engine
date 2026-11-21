use crate::ToolHandler;
use serde_json::{json, Value};
use shared::mcp_protocol::builtin_tools;

pub(crate) fn tools() -> Value {
    let mut tools = serde_json::to_value(builtin_tools()).unwrap();
    tools.as_array_mut().unwrap().extend(crate::terrain::tools());
    tools.as_array_mut().unwrap().extend(crate::animation::tools());
    tools.as_array_mut().unwrap().extend(crate::navigation::tools());
    tools.as_array_mut().unwrap().extend(crate::savegame::tools());
    tools.as_array_mut().unwrap().push(crate::netcode::tool());
    for tool in tools.as_array_mut().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let read_only = matches!(
            name,
            "validate_scene"
                | "nav_query_path" | "nav_lint"
                | "animation_clips" | "animator_validate"
                | "get_sim_state"
                | "simulate"
                | "net_simulate"
                | "query"
                | "query_entities" | "get_scene_summary"
                | "blender_status"
                | "blender_scene_info"
                | "list_locks"
                | "get_changes_since"
                | "get_task_changes_since"
                | "task_list"
                | "get_scene_state"
                | "get_revision"
                | "get_screenshot"
                | "list_assets"
                | "terrain_info"
                | "terrain_sample"
        );
        let additive = matches!(
            name,
            "spawn_primitive"
                | "prefab_create" | "prefab_instantiate"
                | "export_web"
                | "task_create"
                | "task_claim"
                | "task_complete"
                | "claim_entities"
                | "release_entities"
                | "blender_start"
                | "play"
                | "load_game" | "save_game"
                | "terrain_generate"
                | "terrain_import"
        );
        tool["annotations"] =
            json!({"readOnlyHint":read_only,"destructiveHint":!read_only && !additive});
    }
    tools
}

/// Workflow reference plus a catalogue generated from the authoritative tools.
pub fn agent_guide() -> String {
    let mut guide = include_str!("guide.md").to_owned();
    for tool in crate::savegame::tools() {
        guide.push_str(&format!("\n- **{}**: {}\n", tool["name"].as_str().unwrap(), tool["description"].as_str().unwrap()));
    }
    guide.push_str("\n## Tool catalogue / Werkzeugkatalog (generated)\n\n");
    for tool in builtin_tools() {
        guide.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
    }
    for tool in crate::terrain::tools() {
        guide.push_str(&format!(
            "- **{}**: {}\n",
            tool["name"].as_str().unwrap(),
            tool["description"].as_str().unwrap()
        ));
    }
    for tool in crate::animation::tools() {
        guide.push_str(&format!("- **{}**: {}\n", tool["name"].as_str().unwrap(), tool["description"].as_str().unwrap()));
    }
    for tool in crate::navigation::tools() {
        guide.push_str(&format!("- **{}**: {}\n", tool["name"].as_str().unwrap(), tool["description"].as_str().unwrap()));
    }
    let tool = crate::netcode::tool();
    guide.push_str(&format!("- **net_simulate**: {}\n", tool["description"].as_str().unwrap()));
    guide
}

pub(crate) fn resources() -> Value {
    json!({"resources":[
        {"uri":"engine://guide","name":"agent_guide","description":"Deutsch/English workflow, components, Rhai API and generated tool catalogue","mimeType":"text/markdown"},
        {"uri":"engine://scene","name":"scene","description":"Current authoring scene JSON; use get_sim_state for live gameplay","mimeType":"application/json"}
    ]})
}

pub(crate) fn read_resource(
    params: &Value,
    handler: &dyn ToolHandler,
) -> Result<Value, (i64, String)> {
    let uri = params["uri"]
        .as_str()
        .ok_or((-32602, "missing resource uri".into()))?;
    let (mime, text) = match uri {
        "engine://guide" => ("text/markdown", agent_guide()),
        "engine://scene" => (
            "application/json",
            serde_json::to_string_pretty(&handler.scene().map_err(|error| (-32603, error))?)
                .unwrap(),
        ),
        _ => return Err((-32002, format!("resource not found: {uri}"))),
    };
    Ok(json!({"contents":[{"uri":uri,"mimeType":mime,"text":text}]}))
}

pub(crate) fn prompts() -> Value {
    json!({"prompts":[
        {"name":"build_game","description":"Blender-first game with deterministic validation / Spiel erstellen","arguments":[{"name":"idea","description":"Game idea / Spielidee","required":true}]},
        {"name":"qa_pass","description":"Validate, simulate and visually inspect the current scene / QA-Prüfung"},
        {"name":"swarm_lead","description":"Coordinate model, gameplay and QA agents / Agententeam koordinieren","arguments":[{"name":"goal","description":"Team goal / Teamziel","required":true}]}
    ]})
}

pub(crate) fn get_prompt(params: &Value) -> Result<Value, String> {
    let name = params["name"].as_str().ok_or("missing prompt name")?;
    let argument = |key: &str| {
        params["arguments"][key]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| format!("missing prompt argument: {key}"))
    };
    let task = match name {
        "build_game" => format!("Build / Erstelle: {}. Model in Blender with materials and gameplay props; sync, write Rhai scripts, simulate with win/fall assertions, validate, inspect screenshots, save a portable scene and export_web. Keep German player-facing UI.", argument("idea")?),
        "qa_pass" => "Run validate_scene, simulate success and failure/respawn paths, inspect get_screenshot with views and overlay_ids, and report actual results. Do not claim visual checks unless images were inspected. / Prüfe Szene, Simulation und Bilder; berichte ehrlich.".into(),
        "swarm_lead" => format!("Coordinate / Koordiniere: {}. Create task dependencies for Blender artist, gameplay scripter and QA. Each agent uses its own identity, claims tasks and entities before edits, checks revisions/get_changes_since, releases claims and completes tasks only after verification. Local trusted agents only.", argument("goal")?),
        _ => return Err(format!("unknown prompt: {name}")),
    };
    Ok(
        json!({"description":name,"messages":[{"role":"user","content":{"type":"text","text":format!("Read engine://guide and engine://scene first. / Lies zuerst die Ressourcen.\n{task}")}}]}),
    )
}
