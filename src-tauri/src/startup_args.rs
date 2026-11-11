//! Reject removed experimental entry points before initializing the real profile.
use std::ffi::OsString;

pub(crate) fn validate(arguments: &[OsString]) -> Result<(), &'static str> {
    if arguments.iter().skip(1).any(|argument| {
        let argument = argument.to_string_lossy();
        argument.starts_with("--native-ui") || argument.starts_with("--cleanup-native-ui-fixture")
    }) {
        return Err("The experimental native interface has been removed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        std::iter::once("voxely")
            .chain(values.iter().copied())
            .map(OsString::from)
            .collect()
    }
    #[test]
    fn removed_experimental_flags_cannot_initialize_the_real_profile() {
        for flag in [
            "--native-ui",
            "--native-ui-check",
            "--native-ui=true",
            "--native-ui-fixture",
            "--native-ui-fixture-microphone",
            "--native-ui-fixture-hud",
            "--native-ui-fixture-backup",
            "--cleanup-native-ui-fixture",
            "--cleanup-native-ui-fixture=",
            "--cleanup-native-ui-fixture=C:/fixture",
        ] {
            for companion in ["--autostart", "--legacy-ui", "--quit-for-local-rebuild=42"] {
                assert!(
                    validate(&args(&[flag, companion])).is_err(),
                    "{flag} {companion}"
                );
            }
        }
    }
    #[test]
    fn ordinary_autostart_and_independent_hud_launches_are_unchanged() {
        for flags in [
            vec![],
            vec!["--autostart"],
            vec!["--legacy-ui"],
            vec!["--native-hud"],
            vec!["--quit-for-local-rebuild=42"],
        ] {
            assert!(validate(&args(&flags)).is_ok(), "{flags:?}");
        }
    }
    #[cfg(windows)]
    #[test]
    fn malformed_utf16_removed_flags_are_rejected() {
        use std::os::windows::ffi::OsStringExt;
        for prefix in ["--native-ui-fixture=", "--cleanup-native-ui-fixture=C:/"] {
            let mut argument: Vec<_> = prefix.encode_utf16().collect();
            argument.push(0xd800);
            assert!(validate(&["voxely".into(), OsString::from_wide(&argument)]).is_err());
        }
    }
}
