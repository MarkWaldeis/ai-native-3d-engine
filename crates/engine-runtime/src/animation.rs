use anyhow::{ensure, Result};
use shared::animation::{AnimationOp, Animator};
use std::collections::BTreeSet;

pub fn validate(animator: &Animator) -> Result<()> {
    let machine = &animator.machine;
    ensure!(
        !animator.clips.is_empty() && animator.clips.len() <= 256,
        "animator requires 1..256 clips"
    );
    let mut names = BTreeSet::new();
    for clip in &animator.clips {
        ensure!(
            !clip.name.is_empty() && names.insert(clip.name.as_str()),
            "duplicate/empty animation clip {}",
            clip.name
        );
        ensure!(
            clip.duration.is_finite() && clip.duration > 0.0 && clip.duration <= 86400.0,
            "invalid duration for clip {}",
            clip.name
        );
    }
    ensure!(
        animator.speed.is_finite() && (0.0..=100.0).contains(&animator.speed),
        "animator speed must be 0..100"
    );
    ensure!(
        animator.blend_time.is_finite() && (0.0..=60.0).contains(&animator.blend_time),
        "blend_time must be 0..60 seconds"
    );
    ensure!(
        !machine.states.is_empty()
            && machine.states.len() <= 256
            && machine.transitions.len() <= 1024
            && machine.parameters.len() <= 64,
        "animation machine exceeds limits or has no states"
    );
    ensure!(
        machine.states.contains_key(&machine.initial),
        "unknown initial animation state {}",
        machine.initial
    );
    for (name, state) in &machine.states {
        ensure!(
            !name.is_empty() && names.contains(state.clip.as_str()),
            "state {name}: unknown clip {}",
            state.clip
        );
        ensure!(
            state.speed.is_finite() && (0.0..=100.0).contains(&state.speed),
            "state {name}: speed must be 0..100"
        );
    }
    for (name, value) in &machine.parameters {
        ensure!(
            !name.is_empty() && value.is_finite(),
            "invalid animation parameter {name}"
        );
    }
    for transition in &machine.transitions {
        ensure!(
            machine.states.contains_key(&transition.from)
                && machine.states.contains_key(&transition.to),
            "transition references unknown animation state"
        );
        ensure!(
            transition.conditions.len() <= 64,
            "too many animation conditions"
        );
        for condition in &transition.conditions {
            ensure!(
                machine.parameters.contains_key(&condition.parameter),
                "unknown animation parameter {}",
                condition.parameter
            );
            ensure!(condition.value.is_finite(), "nonfinite animation condition");
        }
    }
    let mut reachable = BTreeSet::from([machine.initial.as_str()]);
    loop {
        let before = reachable.len();
        for t in &machine.transitions {
            if reachable.contains(t.from.as_str()) {
                reachable.insert(t.to.as_str());
            }
        }
        if before == reachable.len() {
            break;
        }
    }
    let unreachable: Vec<_> = machine
        .states
        .keys()
        .filter(|s| !reachable.contains(s.as_str()))
        .collect();
    ensure!(
        unreachable.is_empty(),
        "unreachable animation states: {unreachable:?}"
    );
    Ok(())
}

pub fn reset(animator: &mut Animator) {
    animator.current_state = animator.machine.initial.clone();
    animator.values = animator.machine.parameters.clone();
    animator.time = 0.0;
    animator.previous_state = None;
    animator.previous_time = 0.0;
    animator.blend_elapsed = 0.0;
    animator.sequence = 0;
}

fn advance(animator: &Animator, name: &str, time: f64, dt: f64) -> f64 {
    let state = &animator.machine.states[name];
    let duration = animator
        .clips
        .iter()
        .find(|c| c.name == state.clip)
        .unwrap()
        .duration;
    let time = time + dt * animator.speed * state.speed;
    if state.looping {
        time % duration
    } else {
        time.min(duration)
    }
}

pub fn step(animator: &mut Animator, dt: f64) {
    let target = animator
        .machine
        .transitions
        .iter()
        .find(|transition| {
            transition.from == animator.current_state
                && transition.conditions.iter().all(|condition| {
                    let value = animator.values[&condition.parameter];
                    match condition.op {
                        AnimationOp::Eq => value == condition.value,
                        AnimationOp::Ne => value != condition.value,
                        AnimationOp::Gt => value > condition.value,
                        AnimationOp::Ge => value >= condition.value,
                        AnimationOp::Lt => value < condition.value,
                        AnimationOp::Le => value <= condition.value,
                    }
                })
        })
        .map(|t| t.to.clone());
    if let Some(target) = target {
        if target != animator.current_state {
            animator.previous_state = Some(std::mem::replace(&mut animator.current_state, target));
            animator.previous_time = animator.time;
            animator.time = 0.0;
            animator.blend_elapsed = 0.0;
            animator.sequence += 1;
        }
    }
    animator.time = advance(animator, &animator.current_state, animator.time, dt);
    if let Some(previous) = &animator.previous_state {
        animator.previous_time = advance(animator, previous, animator.previous_time, dt);
        animator.blend_elapsed = (animator.blend_elapsed + dt).min(animator.blend_time);
        if animator.blend_elapsed >= animator.blend_time {
            animator.previous_state = None;
        }
    }
}
