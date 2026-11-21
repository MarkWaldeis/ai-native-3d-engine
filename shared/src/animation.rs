use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn one() -> f64 {
    1.0
}
fn blend() -> f64 {
    0.15
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationClip {
    pub name: String,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationState {
    pub clip: String,
    #[serde(default = "one")]
    pub speed: f64,
    #[serde(default = "yes")]
    pub looping: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnimationOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationCondition {
    pub parameter: String,
    pub op: AnimationOp,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationTransition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub conditions: Vec<AnimationCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationMachine {
    pub initial: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, f64>,
    pub states: BTreeMap<String, AnimationState>,
    #[serde(default)]
    pub transitions: Vec<AnimationTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Animator {
    pub clips: Vec<AnimationClip>,
    pub machine: AnimationMachine,
    #[serde(default = "one")]
    pub speed: f64,
    #[serde(default = "blend")]
    pub blend_time: f64,
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub time: f64,
    #[serde(default)]
    pub values: BTreeMap<String, f64>,
    #[serde(default)]
    pub previous_state: Option<String>,
    #[serde(default)]
    pub previous_time: f64,
    #[serde(default)]
    pub blend_elapsed: f64,
    #[serde(default)]
    pub sequence: u64,
}
