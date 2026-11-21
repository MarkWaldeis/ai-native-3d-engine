use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

pub(crate) fn content_asset(
    bytes: Vec<u8>,
    files: &mut BTreeMap<String, Vec<u8>>,
    content: &mut BTreeMap<u64, Vec<String>>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let bucket = content.entry(hasher.finish()).or_default();
    if let Some(path) = bucket.iter().find(|path| files[*path] == bytes) {
        return path.clone();
    }
    let path = format!(
        "assets/{}.glb",
        files.keys().filter(|p| p.starts_with("assets/")).count()
    );
    files.insert(path.clone(), bytes);
    bucket.push(path.clone());
    path
}
