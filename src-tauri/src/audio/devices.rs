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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceResolvePlan {
    Exact,
    Default,
    FallbackDefault,
    Ambiguous,
    Unavailable,
}

pub fn plan_device_resolve(
    preferred: Option<&str>,
    listed_ids: &[String],
    listed_names: &[String],
    has_default: bool,
) -> DeviceResolvePlan {
    let Some(name) = preferred.filter(|value| *value != "default") else {
        return if has_default {
            DeviceResolvePlan::Default
        } else {
            DeviceResolvePlan::Unavailable
        };
    };
    if listed_ids.iter().any(|id| id == name) {
        return DeviceResolvePlan::Exact;
    }
    let name_matches = listed_names.iter().filter(|item| *item == name).count();
    if name_matches > 1 {
        return DeviceResolvePlan::Ambiguous;
    }
    if name_matches == 1 {
        return DeviceResolvePlan::Exact;
    }
    if has_default {
        DeviceResolvePlan::FallbackDefault
    } else {
        DeviceResolvePlan::Unavailable
    }
}

pub fn annotate_missing_selection(devices: &mut Vec<InputDeviceInfo>, selected_id: &str) -> bool {
    if selected_id.is_empty() || selected_id == "default" {
        return false;
    }
    if devices.iter().any(|device| device.id == selected_id) {
        return false;
    }
    devices.insert(
        0,
        InputDeviceInfo {
            id: selected_id.to_string(),
            name: selected_id.to_string(),
            is_default: false,
            sample_rate: None,
            channels: None,
            available: false,
        },
    );
    true
}

pub fn resolve_device(preferred: Option<&str>) -> Result<(cpal::Device, bool), String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let listed = list_input_devices().unwrap_or_default();
    let ids: Vec<String> = listed.iter().map(|d| d.id.clone()).collect();
    let names: Vec<String> = listed.iter().map(|d| d.name.clone()).collect();
    let has_default = host.default_input_device().is_some();
    match plan_device_resolve(preferred, &ids, &names, has_default) {
        DeviceResolvePlan::Ambiguous => Err("Microphone selection is ambiguous".into()),
        DeviceResolvePlan::Unavailable => Err("Microphone is unavailable".into()),
        DeviceResolvePlan::Default | DeviceResolvePlan::FallbackDefault => {
            let fallback = matches!(
                plan_device_resolve(preferred, &ids, &names, has_default),
                DeviceResolvePlan::FallbackDefault
            );
            host.default_input_device()
                .map(|device| (device, fallback))
                .ok_or_else(|| "Microphone is unavailable".into())
        }
        DeviceResolvePlan::Exact => {
            let name = preferred.unwrap_or("default");
            let exact = listed.iter().find(|d| d.id == name);
            let name_matches: Vec<_> = listed.iter().filter(|d| d.name == name).collect();
            let resolved_id = if let Some(device) = exact {
                device.id.as_str()
            } else {
                name_matches[0].id.as_str()
            };
            let match_name = listed
                .iter()
                .find(|d| d.id == resolved_id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| name.to_string());
            let occurrence = listed.iter().position(|d| d.id == resolved_id).unwrap_or(0);
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
                    return Ok((found, false));
                }
            }
            host.default_input_device()
                .map(|device| (device, true))
                .ok_or_else(|| "Selected microphone is unavailable".into())
        }
    }
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

    #[test]
    fn missing_preferred_device_is_explicit_fallback() {
        let ids = vec!["Mic".into()];
        let names = vec!["Mic".into()];
        assert_eq!(
            plan_device_resolve(Some("gone"), &ids, &names, true),
            DeviceResolvePlan::FallbackDefault
        );
        assert_eq!(
            plan_device_resolve(Some("gone"), &ids, &names, false),
            DeviceResolvePlan::Unavailable
        );
        let mut devices = vec![InputDeviceInfo {
            id: "Mic".into(),
            name: "Mic".into(),
            is_default: true,
            sample_rate: None,
            channels: None,
            available: true,
        }];
        assert!(annotate_missing_selection(&mut devices, "USB Mic"));
        assert!(!devices[0].available);
        assert_eq!(devices[0].id, "USB Mic");
    }
}
