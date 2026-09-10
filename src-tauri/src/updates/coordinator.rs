use std::path::{Path, PathBuf};
use std::time::Duration;

use super::feed::{asset_expected_hash, download, query_latest, verify_file_hash, ReleaseQuery};
use super::launcher::try_start_silent;
use super::policy::{
    allows_background_process, has_inno_uninstaller, is_allowed_asset_url, is_inside_root,
    is_newer, parse_tag, safe_installer_file_name,
};
use reqwest::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentUpdateOutcome {
    Skipped,
    Busy,
    Offline,
    NoUpdate,
    Applied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePlan {
    Skip,
    Offline,
    NoUpdate,
    Failed,
    Download {
        url: String,
        file_name: String,
        expected_sha256: String,
    },
}

pub fn manual_copy(outcome: SilentUpdateOutcome, ui: &str) -> String {
    if ui == "en" {
        return match outcome {
            SilentUpdateOutcome::NoUpdate => "You already have the latest version.".into(),
            SilentUpdateOutcome::Applied => "Update downloaded. It will install now.".into(),
            SilentUpdateOutcome::Failed => "Could not check or download the update.".into(),
            SilentUpdateOutcome::Offline => "No network.".into(),
            SilentUpdateOutcome::Skipped => "Updates work only for an installed copy.".into(),
            SilentUpdateOutcome::Busy => "Cannot update right now. Try again in a minute.".into(),
        };
    }
    match outcome {
        SilentUpdateOutcome::NoUpdate => "Уже установлена последняя версия.".into(),
        SilentUpdateOutcome::Applied => "Обновление скачано, сейчас установится.".into(),
        SilentUpdateOutcome::Failed => "Не удалось проверить или скачать.".into(),
        SilentUpdateOutcome::Offline => "Нет сети.".into(),
        SilentUpdateOutcome::Skipped => "Обновления доступны только установленной копии.".into(),
        SilentUpdateOutcome::Busy => "Сейчас нельзя обновить. Попробуйте через минуту.".into(),
    }
}

pub fn delay_after(outcome: SilentUpdateOutcome) -> Duration {
    match outcome {
        SilentUpdateOutcome::Failed | SilentUpdateOutcome::Busy | SilentUpdateOutcome::Offline => {
            Duration::from_secs(120)
        }
        _ => Duration::from_secs(15),
    }
}

pub fn plan_update(
    auto_update_enabled: bool,
    current_version: &str,
    process_name: &str,
    application_directory: &Path,
    online: bool,
    query: &ReleaseQuery,
) -> UpdatePlan {
    if !allows_background_process(process_name)
        || !auto_update_enabled
        || !has_inno_uninstaller(application_directory)
    {
        return UpdatePlan::Skip;
    }
    if !online {
        return UpdatePlan::Offline;
    }
    let Some(current) = parse_tag(current_version) else {
        return UpdatePlan::Failed;
    };
    if query.not_found {
        return UpdatePlan::NoUpdate;
    }
    let Some(latest) = query.snapshot.as_ref() else {
        return UpdatePlan::Failed;
    };
    if latest.prerelease {
        return UpdatePlan::NoUpdate;
    }
    let Some(candidate) = parse_tag(&latest.tag_name) else {
        return UpdatePlan::NoUpdate;
    };
    if !is_newer(current, candidate) {
        return UpdatePlan::NoUpdate;
    }
    let Some(asset) = latest
        .assets
        .iter()
        .find(|asset| safe_installer_file_name(&asset.name).is_some())
    else {
        return UpdatePlan::Failed;
    };
    if !is_allowed_asset_url(&asset.browser_download_url) {
        return UpdatePlan::Failed;
    }
    let Some(expected_sha256) = asset_expected_hash(&asset.digest) else {
        return UpdatePlan::Failed;
    };
    let Some(file_name) = safe_installer_file_name(&asset.name) else {
        return UpdatePlan::Failed;
    };
    UpdatePlan::Download {
        url: asset.browser_download_url.clone(),
        file_name,
        expected_sha256,
    }
}

pub fn setup_destination(download_directory: &Path, file_name: &str) -> Option<PathBuf> {
    let dest = download_directory.join(file_name);
    is_inside_root(download_directory, &dest).then_some(dest)
}

pub async fn apply_download(
    client: &Client,
    download_directory: &Path,
    plan: UpdatePlan,
    start_setup: impl Fn(&str) -> bool,
) -> SilentUpdateOutcome {
    let UpdatePlan::Download {
        url,
        file_name,
        expected_sha256,
    } = plan
    else {
        return plan_to_outcome(&plan);
    };
    let Some(destination) = setup_destination(download_directory, &file_name) else {
        return SilentUpdateOutcome::Failed;
    };
    if !download(client, &url, &destination).await {
        let _ = std::fs::remove_file(&destination);
        return SilentUpdateOutcome::Failed;
    }
    if !verify_file_hash(&destination, &expected_sha256) {
        let _ = std::fs::remove_file(&destination);
        return SilentUpdateOutcome::Failed;
    }
    if !start_setup(&destination.to_string_lossy()) {
        return SilentUpdateOutcome::Failed;
    }
    SilentUpdateOutcome::Applied
}

pub fn plan_to_outcome(plan: &UpdatePlan) -> SilentUpdateOutcome {
    match plan {
        UpdatePlan::Skip => SilentUpdateOutcome::Skipped,
        UpdatePlan::Offline => SilentUpdateOutcome::Offline,
        UpdatePlan::NoUpdate => SilentUpdateOutcome::NoUpdate,
        UpdatePlan::Failed => SilentUpdateOutcome::Failed,
        UpdatePlan::Download { .. } => SilentUpdateOutcome::Failed,
    }
}

pub async fn run_installed_pass(
    client: &Client,
    auto_update_enabled: bool,
    current_version: &str,
    process_name: &str,
    application_directory: &Path,
    download_directory: &Path,
    online: bool,
) -> SilentUpdateOutcome {
    let query = if online {
        query_latest(client).await
    } else {
        ReleaseQuery {
            not_found: false,
            snapshot: None,
        }
    };
    let plan = plan_update(
        auto_update_enabled,
        current_version,
        process_name,
        application_directory,
        online,
        &query,
    );
    match plan {
        UpdatePlan::Download { .. } => {
            apply_download(client, download_directory, plan, try_start_silent).await
        }
        other => plan_to_outcome(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::feed::{GitHubAsset, GitHubRelease};
    use tempfile::tempdir;

    fn release(tag: &str) -> ReleaseQuery {
        ReleaseQuery {
            not_found: false,
            snapshot: Some(GitHubRelease {
                tag_name: tag.into(),
                prerelease: false,
                assets: vec![GitHubAsset {
                    name: format!("Voxely-Setup-win-x64-{}.exe", tag.trim_start_matches('v')),
                    browser_download_url: format!(
                        "https://github.com/AryaPaw/voxely/releases/download/{tag}/Voxely-Setup-win-x64-1.exe"
                    ),
                    digest: Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
                }],
            }),
        }
    }

    #[test]
    fn skips_without_inno_uninstaller() {
        let dir = tempdir().unwrap();
        assert_eq!(
            plan_update(
                true,
                "0.1.0",
                "voxely",
                dir.path(),
                true,
                &release("v0.1.1")
            ),
            UpdatePlan::Skip
        );
    }

    #[test]
    fn no_update_when_same_version() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("unins000.exe"), b"x").unwrap();
        assert_eq!(
            plan_update(
                true,
                "0.1.1",
                "voxely",
                dir.path(),
                true,
                &release("v0.1.1")
            ),
            UpdatePlan::NoUpdate
        );
    }

    #[test]
    fn plans_download_for_newer_tag() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("unins000.exe"), b"x").unwrap();
        match plan_update(
            true,
            "0.1.0",
            "voxely",
            dir.path(),
            true,
            &release("v0.1.1"),
        ) {
            UpdatePlan::Download { file_name, .. } => {
                assert_eq!(file_name, "Voxely-Setup-win-x64-0.1.1.exe");
            }
            other => panic!("expected download, got {other:?}"),
        }
    }

    #[test]
    fn english_copy_for_no_update() {
        assert!(manual_copy(SilentUpdateOutcome::NoUpdate, "en").contains("latest"));
    }
}
