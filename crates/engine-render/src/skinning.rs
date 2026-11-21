use anyhow::Result;
use engine_geo::skinning::SkinnedAsset;
use shared::{
    animation::Animator,
    mesh::MeshData,
    scene::{PrimitiveKind, Scene},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

struct Entry {
    stamp: (u64, Option<SystemTime>),
    asset: SkinnedAsset,
}

#[derive(Default)]
pub(crate) struct SkinCache {
    assets: HashMap<PathBuf, Entry>,
}

impl SkinCache {
    pub fn retain(&mut self, scene: &Scene) {
        let paths: HashSet<_> = scene
            .entities
            .iter()
            .filter(|e| e.animator.is_some())
            .filter_map(|e| {
                if let PrimitiveKind::Gltf { path } = &e.mesh {
                    Some(Path::new(path))
                } else {
                    None
                }
            })
            .collect();
        self.assets.retain(|path, _| paths.contains(path.as_path()));
    }

    pub fn mesh(&mut self, path: &Path, animator: &Animator) -> Result<MeshData> {
        let metadata = std::fs::metadata(path)?;
        let stamp = (metadata.len(), metadata.modified().ok());
        if self
            .assets
            .get(path)
            .is_none_or(|entry| entry.stamp != stamp)
        {
            self.assets.insert(
                path.to_owned(),
                Entry {
                    stamp,
                    asset: SkinnedAsset::load(path)?,
                },
            );
        }
        Ok(self.assets[path].asset.pose(Some(animator))?.mesh)
    }
}
