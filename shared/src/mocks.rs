//! Dummy data so every crate can be developed and tested in isolation.

use crate::mesh::{MeshData, Vertex};
use crate::scene::{Entity, PrimitiveKind, Scene, Transform};

/// A single upward-facing triangle — enough to smoke-test any renderer path.
pub fn mock_triangle() -> MeshData {
    MeshData {
        vertices: vec![
            Vertex::new([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.5, 1.0]),
            Vertex::new([-1.0, -1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0]),
            Vertex::new([1.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0]),
        ],
        indices: vec![0, 1, 2],
    }
}

/// Small demo scene: floor + box + sphere.
pub fn mock_scene() -> Scene {
    let mut scene = Scene::default();
    scene.name = "mock".to_string();
    scene.entities = vec![
        Entity {
            id: 1,
            name: "floor".to_string(),
            mesh: PrimitiveKind::Plane { size: 12.0 },
            transform: Transform::default(),
            color: [0.45, 0.5, 0.55, 1.0],
            visible: true,
            source_uid: None,
            props: serde_json::json!({}),
            tags: vec![], collider: None, rigidbody: None, script: None, animator: None, nav_agent: None,
        },
        Entity {
            id: 2,
            name: "crate".to_string(),
            mesh: PrimitiveKind::Box { size: [1.5, 1.5, 1.5] },
            transform: Transform::at(-1.5, 0.75, 0.0),
            color: [0.85, 0.55, 0.2, 1.0],
            visible: true,
            source_uid: None,
            props: serde_json::json!({}),
            tags: vec![], collider: None, rigidbody: None, script: None, animator: None, nav_agent: None,
        },
        Entity {
            id: 3,
            name: "ball".to_string(),
            mesh: PrimitiveKind::Sphere {
                radius: 0.8,
                segments: 24,
            },
            transform: Transform::at(1.6, 0.8, 1.0),
            color: [0.25, 0.55, 0.9, 1.0],
            visible: true,
            source_uid: None,
            props: serde_json::json!({}),
            tags: vec![], collider: None, rigidbody: None, script: None, animator: None, nav_agent: None,
        },
    ];
    scene
}
