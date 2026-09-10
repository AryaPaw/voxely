use std::path::Path;
use std::time::Duration;

use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::policy::{
    digest_sha256, hashes_match, is_allowed_asset_url, is_allowed_redirect_url, latest_api,
    MAX_INSTALLER_BYTES, MAX_MANIFEST_BYTES, PROBE_URL,
};

#[derive(Debug, Clone)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReleaseQuery {
    pub not_found: bool,
    pub snapshot: Option<GitHubRelease>,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

pub fn build_client(timeout: Duration, github_api: bool) -> Result<Client, reqwest::Error> {
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .timeout(timeout)
        .user_agent("Voxely-Updater");
    if github_api {
        builder = builder.default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
            );
            headers
        });
    }
    builder.build()
}

pub async fn query_latest(client: &Client) -> ReleaseQuery {
    let response = match client.get(latest_api()).send().await {
        Ok(response) => response,
        Err(_) => {
            return ReleaseQuery {
                not_found: false,
                snapshot: None,
            };
        }
    };
    if response.status() == StatusCode::NOT_FOUND {
        return ReleaseQuery {
            not_found: true,
            snapshot: None,
        };
    }
    if !response.status().is_success() {
        return ReleaseQuery {
            not_found: false,
            snapshot: None,
        };
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_MANIFEST_BYTES)
    {
        return ReleaseQuery {
            not_found: false,
            snapshot: None,
        };
    }
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_MANIFEST_BYTES => bytes,
        _ => {
            return ReleaseQuery {
                not_found: false,
                snapshot: None,
            };
        }
    };
    match serde_json::from_slice::<ReleaseJson>(&bytes) {
        Ok(parsed) => ReleaseQuery {
            not_found: false,
            snapshot: Some(GitHubRelease {
                tag_name: parsed.tag_name,
                prerelease: parsed.prerelease,
                assets: parsed
                    .assets
                    .into_iter()
                    .map(|asset| GitHubAsset {
                        name: asset.name,
                        browser_download_url: asset.browser_download_url,
                        digest: asset.digest,
                    })
                    .collect(),
            }),
        },
        Err(_) => ReleaseQuery {
            not_found: false,
            snapshot: None,
        },
    }
}

pub async fn is_reachable(client: &Client) -> bool {
    client.get(PROBE_URL).send().await.is_ok()
}

pub async fn download(client: &Client, url: &str, destination: &Path) -> bool {
    if !is_allowed_asset_url(url) {
        return false;
    }
    let Some(parent) = destination.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let _ = std::fs::remove_file(destination);
    let Ok(bytes) = get_following_redirects(client, url).await else {
        return false;
    };
    if bytes.len() as u64 > MAX_INSTALLER_BYTES {
        let _ = std::fs::remove_file(destination);
        return false;
    }
    std::fs::write(destination, bytes).is_ok()
}

async fn get_following_redirects(client: &Client, url: &str) -> Result<Vec<u8>, ()> {
    let mut current = url.to_string();
    for hop in 0..=5 {
        let allowed = if hop == 0 {
            is_allowed_asset_url(&current)
        } else {
            is_allowed_redirect_url(&current)
        };
        if !allowed {
            return Err(());
        }
        let response = client.get(&current).send().await.map_err(|_| ())?;
        let status = response.status();
        if matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(())?
                .to_string();
            current = if next.starts_with("https://") {
                next
            } else {
                return Err(());
            };
            continue;
        }
        if !status.is_success() {
            return Err(());
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_INSTALLER_BYTES)
        {
            return Err(());
        }
        let bytes = response.bytes().await.map_err(|_| ())?;
        if bytes.len() as u64 > MAX_INSTALLER_BYTES {
            return Err(());
        }
        return Ok(bytes.to_vec());
    }
    Err(())
}

pub fn file_sha256_hex(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!("{digest:x}"))
}

pub fn asset_expected_hash(digest: &Option<String>) -> Option<String> {
    digest.as_deref().and_then(digest_sha256)
}

pub fn verify_file_hash(path: &Path, expected: &str) -> bool {
    file_sha256_hex(path).is_some_and(|actual| hashes_match(expected, &actual))
}
