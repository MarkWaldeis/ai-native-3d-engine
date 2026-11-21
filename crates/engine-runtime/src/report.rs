use crate::Simulation;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use shared::scene::Scene;

fn vec3(value: &Value) -> Result<[f32; 3]> {
    let vector: [f32; 3] = serde_json::from_value(value.clone())?;
    if vector.iter().any(|v| !v.is_finite()) {
        bail!("expected finite three-number vector");
    }
    Ok(vector)
}

pub fn validate(assertions: &[Value], ticks: u64) -> Result<()> {
    if assertions.len() > 1024 {
        bail!("at most 1024 assertions");
    }
    for assertion in assertions {
        let fields = assertion
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("assert must be an object"))?;
        if fields.keys().any(|k| {
            ![
                "entity",
                "position_near",
                "tolerance",
                "at_tick",
                "y_greater_than",
                "state",
                "equals",
                "entities_with_tag",
                "count",
            ]
            .contains(&k.as_str())
        }) {
            bail!("unknown assert field; valid forms: entity+position_near, entity+y_greater_than, state+equals, entities_with_tag+count");
        }
        if let Some(tick) = assertion.get("at_tick") {
            if tick.as_u64().is_none_or(|n| n > ticks) {
                bail!("at_tick must be an integer in 0..={ticks}");
            }
        }
        let selectors = ["entity", "state", "entities_with_tag"]
            .iter()
            .filter(|key| assertion.get(**key).is_some())
            .count();
        if selectors != 1 {
            bail!("assert needs exactly one of entity, state, entities_with_tag");
        }
        if let Some(entity) = assertion.get("entity") {
            if !entity.is_string() {
                bail!("assert entity must be a name string");
            }
            let near = assertion.get("position_near");
            let above = assertion.get("y_greater_than");
            if near.is_some() == above.is_some() {
                bail!("entity assert needs exactly one of position_near or y_greater_than");
            }
            if let Some(near) = near {
                vec3(near)?;
            }
            if above.is_some_and(|v| v.as_f64().is_none_or(|n| !n.is_finite())) {
                bail!("y_greater_than must be finite");
            }
            if assertion
                .get("tolerance")
                .is_some_and(|v| v.as_f64().is_none_or(|n| !n.is_finite() || n < 0.0))
            {
                bail!("tolerance must be nonnegative");
            }
        } else if let Some(state) = assertion.get("state") {
            if !state.is_string() || !fields.contains_key("equals") {
                bail!("state assert requires string state and equals value");
            }
        } else if !assertion["entities_with_tag"].is_string()
            || assertion["count"].as_u64().is_none()
        {
            bail!("tag assert requires string entities_with_tag and nonnegative count");
        }
    }
    Ok(())
}

pub fn evaluate(simulation: &Simulation, assertion: &Value) -> Value {
    let scene = simulation.scene();
    if let Some(name) = assertion["entity"].as_str() {
        if !scene.entities.iter().any(|e| e.name == name) {
            let hint = shared::suggestions::hint(name, scene.entities.iter().map(|e| e.name.as_str()));
            return json!({"assert":assertion,"at_tick":simulation.tick,"passed":false,"actual":null,"message":format!("unknown entity name '{name}'{hint}")});
        }
    }
    let (passed, actual) = if let Some(name) = assertion["entity"].as_str() {
        if let Some(entity) = scene.entities.iter().find(|e| e.name == name) {
            if let Some(target) = assertion.get("position_near") {
                let target = vec3(target).expect("validated assertion");
                let distance = entity
                    .transform
                    .position
                    .iter()
                    .zip(target)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt();
                (
                    distance as f64 <= assertion["tolerance"].as_f64().unwrap_or(0.5),
                    json!(entity.transform.position),
                )
            } else {
                (
                    entity.transform.position[1] as f64
                        > assertion["y_greater_than"].as_f64().unwrap(),
                    json!(entity.transform.position[1]),
                )
            }
        } else {
            (false, Value::Null)
        }
    } else if let Some(key) = assertion["state"].as_str() {
        let actual = simulation.state()["state"]
            .get(key)
            .cloned()
            .unwrap_or(Value::Null);
        let expected = &assertion["equals"];
        let equal = actual == *expected
            || actual
                .as_f64()
                .zip(expected.as_f64())
                .is_some_and(|(a, b)| a == b);
        (equal, actual)
    } else {
        let tag = assertion["entities_with_tag"].as_str().unwrap();
        let count = scene
            .entities
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .count();
        (
            Some(count as u64) == assertion["count"].as_u64(),
            json!(count),
        )
    };
    json!({"assert":assertion, "at_tick":simulation.tick, "passed":passed, "actual":actual})
}

pub fn query(scene: &Scene, args: &Value) -> Result<Value> {
    if let Some(name) = args["name"].as_str() {
        if !scene.entities.iter().any(|e| e.name == name) {
            bail!("unknown entity name '{name}'{}", shared::suggestions::hint(name, scene.entities.iter().map(|e| e.name.as_str())));
        }
    }
    for key in ["tag", "name"] {
        if args.get(key).is_some_and(|v| !v.is_string()) {
            bail!("{key} must be a string");
        }
    }
    let near = args
        .get("near")
        .map(|near| -> Result<_> {
            let point = vec3(&near["point"])?;
            let radius = near["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && *r >= 0.0)
                .ok_or_else(|| anyhow::anyhow!("near.radius must be nonnegative"))?;
            Ok((point, radius))
        })
        .transpose()?;
    Ok(json!(scene
        .entities
        .iter()
        .filter(|entity| {
            args["name"].as_str().is_none_or(|name| entity.name == name)
                && args["tag"]
                    .as_str()
                    .is_none_or(|tag| entity.tags.iter().any(|t| t == tag))
                && near.is_none_or(|(point, radius)| {
                    entity
                        .transform
                        .position
                        .iter()
                        .zip(point)
                        .map(|(a, b)| (*a as f64 - b as f64).powi(2))
                        .sum::<f64>()
                        <= radius * radius
                })
        })
        .map(|e| json!({"id":e.id, "name":e.name, "tags":e.tags, "position":e.transform.position}))
        .collect::<Vec<_>>()))
}
