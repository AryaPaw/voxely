use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    #[serde(default)]
    pub available: bool,
}

pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut devices = Vec::new();
    let inputs = host.input_devices().map_err(|e| e.to_string())?;
    for device in inputs {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());
        let count = seen.entry(name.clone()).or_insert(0);
        *count += 1;
        let id = if *count == 1 {
            name.clone()
        } else {
            format!("{name} #{count}")
        };
        let is_default = default_name.as_ref() == Some(&name) && *count == 1;
        let (sample_rate, channels) = device
            .default_input_config()
            .ok()
            .map(|c| (Some(c.sample_rate().0), Some(c.channels())))
            .unwrap_or((None, None));
        devices.push(InputDeviceInfo {
            id,
            name,
            is_default,
            sample_rate,
            channels,
            available: true,
        });
    }
    Ok(devices)
}

pub fn resolve_device(preferred: Option<&str>) -> Result<cpal::Device, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(name) = preferred {
        if name != "default" {
            let listed = list_input_devices().unwrap_or_default();
            let exact = listed.iter().find(|d| d.id == name);
            let name_matches: Vec<_> = listed.iter().filter(|d| d.name == name).collect();
            let resolved_id = if let Some(device) = exact {
                Some(device.id.as_str())
            } else if name_matches.len() == 1 {
                Some(name_matches[0].id.as_str())
            } else if name_matches.len() > 1 {
                return Err("Microphone selection is ambiguous".into());
            } else {
                return Err("Selected microphone is unavailable".into());
            };
            if let Some(id) = resolved_id {
                let match_name = listed
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| name.to_string());
                let occurrence = listed.iter().position(|d| d.id == id).unwrap_or(0);
                if let Ok(mut devices) = host.input_devices() {
                    let mut seen = 0usize;
                    if let Some(found) = devices.find(|d| {
                        if d.name().ok().as_deref() == Some(match_name.as_str()) {
                            if seen == occurrence {
                                return true;
                            }
                            seen += 1;
                        }
                        false
                    }) {
                        return Ok(found);
                    }
                }
                return Err("Selected microphone is unavailable".into());
            }
        }
    }
    host.default_input_device()
        .ok_or_else(|| "Microphone is unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_get_stable_suffix_ids() {
        let mut seen: HashMap<String, u32> = HashMap::new();
        let names = ["Mic", "Mic", "Other"];
        let mut ids = Vec::new();
        for name in names {
            let count = seen.entry(name.into()).or_insert(0);
            *count += 1;
            ids.push(if *count == 1 {
                name.to_string()
            } else {
                format!("{name} #{count}")
            });
        }
        assert_eq!(ids, ["Mic", "Mic #2", "Other"]);
    }
}
