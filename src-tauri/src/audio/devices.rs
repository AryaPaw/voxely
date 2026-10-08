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

fn selected_device_identity(listed: &[InputDeviceInfo], selected: &str) -> Option<(String, usize)> {
    let index = listed
        .iter()
        .position(|device| device.id == selected)
        .or_else(|| listed.iter().position(|device| device.name == selected))?;
    let name = listed[index].name.clone();
    let occurrence = listed[..index]
        .iter()
        .filter(|device| device.name == name)
        .count();
    Some((name, occurrence))
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
            let Some((match_name, occurrence)) = selected_device_identity(&listed, name) else {
                return Err("Selected microphone is unavailable".into());
            };
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

    #[test]
    fn selected_unique_device_uses_occurrence_among_same_names_only() {
        let listed = ["Internal", "USB", "Virtual"]
            .into_iter()
            .map(|name| InputDeviceInfo {
                id: name.into(),
                name: name.into(),
                is_default: false,
                sample_rate: None,
                channels: None,
                available: true,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            selected_device_identity(&listed, "USB"),
            Some(("USB".into(), 0))
        );
    }

    #[test]
    fn duplicate_device_occurrence_is_counted_within_its_name() {
        let listed = ["Internal", "USB", "USB"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| InputDeviceInfo {
                id: if index == 2 {
                    "USB #2".into()
                } else {
                    name.into()
                },
                name: name.into(),
                is_default: false,
                sample_rate: None,
                channels: None,
                available: true,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            selected_device_identity(&listed, "USB #2"),
            Some(("USB".into(), 1))
        );
    }

    #[test]
    fn explicit_id_wins_over_ambiguous_legacy_device_name() {
        let ids = vec!["USB #1".into(), "USB #2".into()];
        let names = vec!["USB".into(), "USB".into()];
        assert_eq!(
            plan_device_resolve(Some("USB #2"), &ids, &names, false),
            DeviceResolvePlan::Exact
        );
        assert_eq!(
            plan_device_resolve(Some("USB"), &ids, &names, true),
            DeviceResolvePlan::Ambiguous
        );
        assert_eq!(
            plan_device_resolve(Some("USB"), &[], &["USB".into()], false),
            DeviceResolvePlan::Exact
        );
    }

    #[test]
    fn default_selection_does_not_require_any_enumerated_device() {
        for selected in [None, Some("default")] {
            assert_eq!(
                plan_device_resolve(selected, &[], &[], true),
                DeviceResolvePlan::Default
            );
            assert_eq!(
                plan_device_resolve(selected, &[], &[], false),
                DeviceResolvePlan::Unavailable
            );
        }
    }

    #[test]
    fn missing_selection_annotation_is_idempotent_and_preserves_existing_devices() {
        let mut listed = vec![InputDeviceInfo {
            id: "USB".into(),
            name: "USB".into(),
            is_default: true,
            sample_rate: Some(48_000),
            channels: Some(2),
            available: true,
        }];
        let original = listed.clone();
        for selected in ["", "default", "USB"] {
            assert!(!annotate_missing_selection(&mut listed, selected));
            assert_eq!(listed, original);
        }
        assert!(annotate_missing_selection(&mut listed, "disconnected"));
        assert!(!annotate_missing_selection(&mut listed, "disconnected"));
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1], original[0]);
        assert!(!listed[0].available);
        assert!(!listed[0].is_default);
        assert_eq!(listed[0].sample_rate, None);
        assert_eq!(listed[0].channels, None);
        assert_eq!(selected_device_identity(&original, "missing"), None);
    }
}
