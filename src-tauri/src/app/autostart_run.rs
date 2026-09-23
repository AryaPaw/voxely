/// HKCU Run values that launch this exe besides the live plugin name.

pub fn launches_voxely(command: &str) -> bool {
    command
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .any(|part| part == "voxely.exe")
}

pub fn should_delete_run_value(value_name: &str, command: &str, keep: &str) -> bool {
    value_name != keep && launches_voxely(command)
}

pub fn extra_run_value_names<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    keep: &str,
) -> Vec<String> {
    entries
        .into_iter()
        .filter(|(name, command)| should_delete_run_value(name, command, keep))
        .map(|(name, _)| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{extra_run_value_names, launches_voxely, should_delete_run_value};

    #[test]
    fn detects_voxely_exe_in_quoted_and_bare_commands() {
        assert!(launches_voxely(
            r#""C:\Users\AryaPaw\Documents\GitHub\voxely\src-tauri\target\debug\voxely.exe" --autostart"#
        ));
        assert!(launches_voxely(
            r"C:\Users\AryaPaw\Documents\GitHub\voxely\src-tauri\target\debug\voxely.exe --autostart"
        ));
        assert!(!launches_voxely(
            r"C:\Program Files\NetLights\NetLights.exe"
        ));
    }

    #[test]
    fn keeps_the_plugin_run_value_and_drops_stale_names() {
        assert!(!should_delete_run_value(
            "Voxely",
            r"C:\app\voxely.exe --autostart",
            "Voxely"
        ));
        assert!(should_delete_run_value(
            "com.voxely.desktop",
            r#""C:\app\voxely.exe" --autostart"#,
            "Voxely"
        ));
        let extra = extra_run_value_names(
            [
                ("com.voxely.desktop", r#""C:\app\voxely.exe" --autostart"#),
                ("Voxely", r"C:\app\voxely.exe --autostart"),
                ("Docker Desktop", r"C:\Program Files\Docker\Docker.exe"),
            ],
            "Voxely",
        );
        assert_eq!(extra, vec!["com.voxely.desktop".to_string()]);
    }
}
