use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsp::dynamics::{DynamicsConfig, GateConfig, LimiterConfig};
use crate::dsp::high_pass::GainConfig;
use crate::dsp::pipeline::{DspPreset, FilterKind, FilterSlot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObsFilterView {
    pub name: String,
    pub filter_type: String,
    pub enabled: bool,
    pub settings: Value,
    pub mapped_kind: Option<FilterKind>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObsImportPreview {
    pub source_name: String,
    pub device_id: Option<String>,
    pub filters: Vec<ObsFilterView>,
    pub unsupported: Vec<String>,
}

pub fn parse_scene_collection(json: &str) -> Result<Vec<ObsImportPreview>, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut sources = Vec::new();
    if let Some(array) = root.get("sources").and_then(Value::as_array) {
        for source in array {
            if let Some(preview) = preview_source(source) {
                sources.push(preview);
            }
        }
    }
    if let Some(array) = root.get("aux-audio").and_then(Value::as_array) {
        for source in array {
            if let Some(preview) = preview_source(source) {
                sources.push(preview);
            }
        }
    }
    Ok(sources)
}

fn preview_source(source: &Value) -> Option<ObsImportPreview> {
    let id = source.get("id")?.as_str()?;
    if !id.contains("wasapi_input")
        && !id.contains("pulse_input")
        && id != "wasapi_input_capture"
        && !id.contains("input_capture")
    {
        return None;
    }
    let name = source.get("name")?.as_str()?.to_string();
    let device_id = source
        .get("settings")
        .and_then(|s| s.get("device_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut filters = Vec::new();
    let mut unsupported = Vec::new();
    if let Some(list) = source.get("filters").and_then(Value::as_array) {
        for filter in list {
            let view = map_filter(filter);
            if let Some(reason) = &view.unsupported_reason {
                unsupported.push(format!("{} ({reason})", view.name));
            }
            filters.push(view);
        }
    }
    Some(ObsImportPreview {
        source_name: name,
        device_id,
        filters,
        unsupported,
    })
}

fn map_filter(filter: &Value) -> ObsFilterView {
    let name = filter
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Unnamed")
        .to_string();
    let filter_type = filter
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let enabled = filter
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let settings = filter
        .get("settings")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let (mapped_kind, unsupported_reason) = match filter_type.as_str() {
        "noise_suppress_filter" | "noise_suppress_filter_v2" => {
            let method = settings
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("rnnoise");
            if method == "rnnoise" {
                (Some(FilterKind::Rnnoise), None)
            } else {
                (
                    Some(FilterKind::Rnnoise),
                    Some(format!(
                        "OBS method '{method}' is not RNNoise; mapped approximately"
                    )),
                )
            }
        }
        "gain_filter" => (Some(FilterKind::Gain), None),
        "compressor_filter" => (Some(FilterKind::Compressor), None),
        "expander_filter" => (Some(FilterKind::Expander), None),
        "noise_gate_filter" => (Some(FilterKind::Gate), None),
        "limiter_filter" => (Some(FilterKind::Limiter), None),
        other => (None, Some(format!("Unsupported: {other}"))),
    };
    ObsFilterView {
        name,
        filter_type,
        enabled,
        settings,
        mapped_kind,
        unsupported_reason,
    }
}

pub fn preset_from_preview(preview: &ObsImportPreview, name: String) -> DspPreset {
    let mut preset = DspPreset::obs_imported();
    preset.id = format!("obs-{}", uuid::Uuid::new_v4());
    preset.name = name;
    preset.order.clear();
    for filter in &preview.filters {
        let Some(kind) = filter.mapped_kind else {
            continue;
        };
        match kind {
            FilterKind::Gain => {
                if let Some(db) = filter.settings.get("db").and_then(Value::as_f64) {
                    preset.gain = GainConfig { db: db as f32 };
                }
            }
            FilterKind::Compressor => {
                preset.compressor = DynamicsConfig {
                    threshold_db: num(&filter.settings, "threshold", -18.0),
                    ratio: num(&filter.settings, "ratio", 3.0),
                    attack_ms: num(&filter.settings, "attack_time", 6.0),
                    release_ms: num(&filter.settings, "release_time", 60.0),
                    makeup_db: num(&filter.settings, "output_gain", 0.0),
                    sample_rate: 48_000.0,
                };
            }
            FilterKind::Expander => {
                preset.expander = DynamicsConfig {
                    threshold_db: num(&filter.settings, "threshold", -40.0),
                    ratio: num(&filter.settings, "ratio", 2.0),
                    attack_ms: num(&filter.settings, "attack_time", 10.0),
                    release_ms: num(&filter.settings, "release_time", 125.0),
                    makeup_db: num(&filter.settings, "output_gain", 0.0),
                    sample_rate: 48_000.0,
                };
            }
            FilterKind::Gate => {
                preset.gate = GateConfig {
                    open_threshold_db: num(&filter.settings, "open_threshold", -26.0),
                    close_threshold_db: num(&filter.settings, "close_threshold", -32.0),
                    hold_ms: num(&filter.settings, "hold_time", 200.0),
                    release_ms: num(&filter.settings, "release_time", 150.0),
                    sample_rate: 48_000.0,
                };
            }
            FilterKind::Limiter => {
                preset.limiter = LimiterConfig {
                    threshold_db: num(&filter.settings, "threshold", -6.0),
                    release_ms: num(&filter.settings, "release_time", 60.0),
                    sample_rate: 48_000.0,
                };
            }
            FilterKind::Rnnoise | FilterKind::HighPass => {}
        }
        preset.order.push(FilterSlot {
            id: filter.name.clone(),
            kind,
            enabled: filter.enabled,
        });
    }
    if !preset.order.iter().any(|s| s.kind == FilterKind::Limiter) {
        preset.order.push(FilterSlot {
            id: "limiter".into(),
            kind: FilterKind::Limiter,
            enabled: true,
        });
    }
    preset
}

fn num(settings: &Value, key: &str, default: f32) -> f32 {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wasapi_mic() {
        let json = r#"{
          "aux-audio": [{
            "name": "Mic/Aux",
            "id": "wasapi_input_capture",
            "settings": { "device_id": "default" },
            "filters": [
              { "name": "EQ", "id": "basic_eq_filter", "enabled": true, "settings": {} },
              { "name": "Gain", "id": "gain_filter", "enabled": true, "settings": { "db": 2.0 } }
            ]
          }]
        }"#;
        let sources = parse_scene_collection(json).unwrap();
        assert_eq!(sources[0].source_name, "Mic/Aux");
        assert_eq!(sources[0].unsupported.len(), 1);
        let preset = preset_from_preview(&sources[0], "Imported".into());
        assert!(preset.order.iter().any(|s| s.kind == FilterKind::Gain));
        assert_eq!(preset.gain.db, 2.0);
    }
}
