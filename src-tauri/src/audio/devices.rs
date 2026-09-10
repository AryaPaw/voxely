use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut devices = Vec::new();
    let inputs = host.input_devices().map_err(|e| e.to_string())?;
    for device in inputs {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());
        let is_default = default_name.as_ref() == Some(&name);
        let (sample_rate, channels) = device
            .default_input_config()
            .ok()
            .map(|c| (Some(c.sample_rate().0), Some(c.channels())))
            .unwrap_or((None, None));
        devices.push(InputDeviceInfo {
            id: name.clone(),
            name,
            is_default,
            sample_rate,
            channels,
        });
    }
    Ok(devices)
}

pub fn resolve_device(preferred: Option<&str>) -> Result<cpal::Device, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(name) = preferred {
        if name != "default" {
            if let Ok(mut devices) = host.input_devices() {
                if let Some(found) = devices.find(|d| d.name().ok().as_deref() == Some(name)) {
                    return Ok(found);
                }
            }
        }
    }
    host.default_input_device()
        .ok_or_else(|| "Microphone is unavailable".into())
}
