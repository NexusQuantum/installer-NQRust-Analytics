use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use color_eyre::eyre::{Result, bail};
use reqwest::{Client, StatusCode};
use semver::Version;
use serde::Deserialize;
use tokio::process::Command;

const OWNER: &str = "NexusQuantum";

struct ServiceConfig {
    pub display_name: &'static str,
    pub image: &'static str,
    pub package: &'static str,
    pub current_tag: &'static str,
}

const SERVICE_CONFIGS: &[ServiceConfig] = &[
    ServiceConfig {
        display_name: "Analytics Engine",
        image: "ghcr.io/nexusquantum/analytics-engine",
        package: "analytics-engine",
        current_tag: "latest",
    },
    ServiceConfig {
        display_name: "Analytics Engine Ibis",
        image: "ghcr.io/nexusquantum/analytics-engine-ibis",
        package: "analytics-engine-ibis",
        current_tag: "latest",
    },
    ServiceConfig {
        display_name: "Analytics Service",
        image: "ghcr.io/nexusquantum/analytics-service",
        package: "analytics-service",
        current_tag: "latest",
    },
    ServiceConfig {
        display_name: "Analytics UI",
        image: "ghcr.io/nexusquantum/analytics-ui",
        package: "analytics-ui",
        current_tag: "latest",
    },
];

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub display_name: String,
    pub image: String,
    pub current_tag: String,
    pub available_tags: Vec<String>,
    pub latest_release_tag: Option<String>,
    pub latest_release_published: Option<DateTime<Utc>>,
    pub remote_latest_updated: Option<DateTime<Utc>>,
    pub local_created: Option<DateTime<Utc>>,
    pub status_note: Option<String>,
    pub has_update: bool,
    pub is_self: bool,
    pub download_url: Option<String>,
    pub checksum_url: Option<String>,
}

impl UpdateInfo {
    fn new(config: &ServiceConfig) -> Self {
        Self {
            display_name: config.display_name.to_string(),
            image: config.image.to_string(),
            current_tag: config.current_tag.to_string(),
            available_tags: Vec::new(),
            latest_release_tag: None,
            latest_release_published: None,
            remote_latest_updated: None,
            local_created: None,
            status_note: None,
            has_update: false,
            is_self: false,
            download_url: None,
            checksum_url: None,
        }
    }

    pub fn recompute_status(&mut self) {
        if let Some(remote) = self.remote_latest_updated {
            match self.local_created {
                Some(local) => {
                    let delta = remote - local;
                    self.has_update = delta > Duration::seconds(5);
                }
                None => {
                    self.has_update = true;
                }
            }
        } else {
            self.has_update = false;
        }
    }

    pub fn apply_local_created(&mut self, created: Option<DateTime<Utc>>) {
        self.local_created = created;
        self.recompute_status();
    }

    pub fn pull_reference(&self) -> String {
        format!("{}:{}", self.image, self.current_tag)
    }

    pub fn append_status(&mut self, message: &str) {
        append_status(&mut self.status_note, message);
    }

    pub fn clear_local_error(&mut self) {
        if let Some(note) = &self.status_note {
            if note.contains("Failed to inspect local image") {
                self.status_note = None;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct PackageVersion {
    #[serde(default)]
    metadata: Option<PackageMetadata>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    #[serde(default)]
    container: Option<ContainerMetadata>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    published_at: Option<DateTime<Utc>>,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ContainerMetadata {
    #[serde(default)]
    tags: Option<Vec<String>>,
}

pub async fn collect_update_infos(client: &Client, token: Option<&str>) -> Result<Vec<UpdateInfo>> {
    let mut infos = Vec::new();

    for config in SERVICE_CONFIGS {
        let mut info = UpdateInfo::new(config);

        match fetch_package_versions(client, OWNER, config.package, token).await? {
            Some(versions) => apply_remote_versions(&mut info, versions),
            None => append_status(
                &mut info.status_note,
                "Package not found in GitHub Container Registry",
            ),
        }

        match inspect_local_image_created_at(config.image, config.current_tag).await {
            Ok(created) => info.apply_local_created(created),
            Err(e) => {
                append_status(
                    &mut info.status_note,
                    &format!("Failed to inspect local image: {}", e),
                );
                info.apply_local_created(None);
            }
        }

        infos.push(info);
    }

    if let Some(self_update) = fetch_installer_update(client).await? {
        infos.push(self_update);
    }

    Ok(infos)
}

async fn fetch_installer_update(client: &Client) -> Result<Option<UpdateInfo>> {
    let url = format!(
        "https://api.github.com/repos/{owner}/installer-NQRust-Analytics/releases/latest",
        owner = OWNER
    );

    let response = client
        .get(&url)
        .header("User-Agent", "nqrust-analytics")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let release: ReleaseResponse = response.error_for_status()?.json().await?;

    let current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));

    let remote_version = Version::parse(release.tag_name.trim_start_matches('v')).ok();

    let mut download_url = None;
    let mut checksum_url = None;
    for asset in &release.assets {
        if asset.name.ends_with("_amd64.deb") {
            download_url = Some(asset.browser_download_url.clone());
        }
        if asset.name.eq_ignore_ascii_case("SHA256SUMS") {
            checksum_url = Some(asset.browser_download_url.clone());
        }
    }

    if download_url.is_none() {
        // No installer artifact available; skip adding entry.
        return Ok(None);
    }

    let mut info = UpdateInfo {
        display_name: "Installer (self-update)".to_string(),
        image: "installer".to_string(),
        current_tag: format!("v{}", env!("CARGO_PKG_VERSION")),
        available_tags: Vec::new(),
        latest_release_tag: Some(release.tag_name.clone()),
        latest_release_published: release.published_at,
        remote_latest_updated: release.published_at,
        local_created: None,
        status_note: None,
        has_update: false,
        is_self: true,
        download_url,
        checksum_url,
    };

    if let Some(remote) = remote_version {
        info.has_update = remote > current_version;
    }

    Ok(Some(info))
}

fn apply_remote_versions(info: &mut UpdateInfo, versions: Vec<PackageVersion>) {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let mut tag_dates: HashMap<String, DateTime<Utc>> = HashMap::new();

    for version in versions {
        let timestamp = version.updated_at.or(version.created_at);
        let Some(metadata) = version.metadata else {
            continue;
        };
        let Some(container) = metadata.container else {
            continue;
        };
        let Some(version_tags) = container.tags else {
            continue;
        };

        for tag in version_tags {
            if seen.insert(tag.clone()) {
                tags.push(tag.clone());
            }
            if let Some(ts) = timestamp {
                tag_dates.entry(tag.clone()).or_insert(ts);
            }
        }
    }

    tags.sort();
    info.available_tags = tags.clone();

    if let Some(latest_tag) = determine_latest_release_tag(&tags) {
        info.latest_release_published = tag_dates.get(&latest_tag).cloned();
        info.latest_release_tag = Some(latest_tag);
    }

    info.remote_latest_updated = tag_dates.get("latest").cloned();
    info.recompute_status();

    if info.available_tags.is_empty() {
        append_status(&mut info.status_note, "No tags found for this image yet");
    }
}

fn determine_latest_release_tag(tags: &[String]) -> Option<String> {
    let mut semver_tags: Vec<(Version, String)> = tags
        .iter()
        .filter_map(|tag| {
            let candidate = tag.trim_start_matches('v');
            Version::parse(candidate)
                .ok()
                .map(|version| (version, tag.clone()))
        })
        .collect();

    if semver_tags.is_empty() {
        return None;
    }

    semver_tags.sort_by(|a, b| b.0.cmp(&a.0));
    Some(semver_tags.remove(0).1)
}

async fn fetch_package_versions(
    client: &Client,
    owner: &str,
    package: &str,
    token: Option<&str>,
) -> Result<Option<Vec<PackageVersion>>> {
    let endpoints = [
        format!(
            "https://api.github.com/orgs/{owner}/packages/container/{package}/versions?per_page=100"
        ),
        format!(
            "https://api.github.com/users/{owner}/packages/container/{package}/versions?per_page=100"
        ),
    ];

    for url in endpoints {
        let mut request = client
            .get(&url)
            .header("User-Agent", "nqrust-analytics")
            .header("Accept", "application/vnd.github+json");

        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        match response.status() {
            StatusCode::OK => {
                let data = response.json::<Vec<PackageVersion>>().await?;
                return Ok(Some(data));
            }
            StatusCode::NOT_FOUND => continue,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                let body = response.text().await.unwrap_or_default();
                bail!("GitHub API request for package {package} requires authentication: {body}");
            }
            status if status.is_server_error() => {
                let body = response.text().await.unwrap_or_default();
                bail!("GitHub API error {status} for package {package}: {body}");
            }
            status => {
                let body = response.text().await.unwrap_or_default();
                bail!("Failed to fetch package {package}: {status} {body}");
            }
        }
    }

    Ok(None)
}

async fn inspect_local_image_created_at(image: &str, tag: &str) -> Result<Option<DateTime<Utc>>> {
    let reference = format!("{}:{}", image, tag);
    let output = Command::new("docker")
        .args(["image", "inspect", &reference, "--format", "{{.Created}}"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout.trim();

    if value.is_empty() {
        return Ok(None);
    }

    let created = DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .ok();

    Ok(created)
}

pub async fn get_local_image_created(image: &str, tag: &str) -> Result<Option<DateTime<Utc>>> {
    inspect_local_image_created_at(image, tag).await
}

fn append_status(target: &mut Option<String>, message: &str) {
    match target {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(message);
        }
        None => {
            *target = Some(message.to_string());
        }
    }
}
