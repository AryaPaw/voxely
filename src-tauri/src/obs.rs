use std::path::PathBuf;

use crate::dsp::obs_mapping::{parse_scene_collection, ObsImportPreview};
use crate::error::AppError;

pub fn scene_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("obs-studio")
            .join("basic")
            .join("scenes"),
    )
}

pub fn load_local_previews() -> Result<Vec<ObsImportPreview>, AppError> {
    let dir = scene_dir().ok_or_else(|| AppError::StorageFailed("APPDATA missing".into()))?;
    if !dir.exists() {
        return Err(AppError::StorageFailed(
            "OBS scene collections not found".into(),
        ));
    }
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| AppError::StorageFailed(e.to_string()))? {
        let entry = entry.map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".bak"))
        {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        if let Ok(mut parsed) = parse_scene_collection(&text) {
            all.append(&mut parsed);
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_on_minimal_json() {
        let json = r#"{"sources":[]}"#;
        assert!(parse_scene_collection(json).unwrap().is_empty());
    }
}
