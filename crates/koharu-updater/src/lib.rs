// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Koharu's manifest-free GitHub Releases updater.
//!
//! The platform installation paths are adapted from `tauri-plugin-updater`
//! 2.10.1. Release discovery and package selection are Koharu-owned: the
//! release tag is authoritative and each supported target has one package
//! filename. There is deliberately no updater manifest or signature layer.

use std::{
    ffi::OsString,
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

use futures::StreamExt as _;
use semver::Version;
use serde::Deserialize;
use tempfile::TempPath;
use tokio::io::AsyncWriteExt as _;
use url::Url;

const RELEASES_API: &str = "https://api.github.com/repos/mayocream/koharu/releases?per_page=100";
const RELEASE_DOWNLOAD_ROOT: &str = "https://github.com/mayocream/koharu/releases/download";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("the current Koharu version is invalid: {0}")]
    CurrentVersion(#[source] semver::Error),
    #[error("GitHub did not return a published Koharu release")]
    ReleaseNotFound,
    #[error("Koharu {version} does not contain the required build {asset}")]
    BuildNotFound { version: Version, asset: String },
    #[error("Koharu does not publish updates for this operating system and architecture")]
    UnsupportedTarget,
    #[error("Koharu is not running from an installed updateable package")]
    NotInstalledPackage,
    #[error("the downloaded package is not in the expected executable format")]
    InvalidPackage,
    #[error("the installed macOS application bundle could not be located")]
    AppBundleNotFound,
    #[error("the platform installer could not be launched")]
    InstallerLaunch,
    #[error("the privileged macOS installer failed")]
    PrivilegedInstall,
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the updater installation task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Update {
    version: Version,
    body: Option<String>,
    download_url: Url,
}

impl Update {
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Clone)]
pub struct Updater {
    client: reqwest::Client,
    current_version: Version,
}

impl Updater {
    pub fn new(current_version: &str) -> Result<Self> {
        let current_version = Version::parse(current_version).map_err(Error::CurrentVersion)?;
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            client,
            current_version,
        })
    }

    pub async fn check(&self) -> Result<Option<Update>> {
        let releases = self
            .client
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<GithubRelease>>()
            .await?;
        let release = latest_product_release(releases).ok_or(Error::ReleaseNotFound)?;
        available_update(release, &self.current_version)
    }

    pub async fn download_and_install(
        &self,
        update: &Update,
        mut progress: impl FnMut(DownloadProgress) + Send,
    ) -> Result<()> {
        let package = self.download(update, &mut progress).await?;
        let executable = updateable_executable()?;
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || install(package, executable, arguments)).await??;
        Ok(())
    }

    async fn download(
        &self,
        update: &Update,
        progress: &mut (impl FnMut(DownloadProgress) + Send),
    ) -> Result<TempPath> {
        let package = tempfile::Builder::new()
            .prefix("koharu-update-")
            .suffix(package_suffix()?)
            .tempfile()?
            .into_temp_path();
        let mut file = tokio::fs::File::create(&package).await?;
        let response = self
            .client
            .get(update.download_url.clone())
            .header("Accept", "application/octet-stream")
            .send()
            .await?
            .error_for_status()?;
        let total = response.content_length();
        let mut downloaded = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress(DownloadProgress { downloaded, total });
        }
        file.flush().await?;
        drop(file);
        validate_package(&package)?;
        Ok(package)
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
}

struct ProductRelease {
    tag: String,
    version: Version,
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

fn available_update(release: ProductRelease, current_version: &Version) -> Result<Option<Update>> {
    if release.version <= *current_version {
        return Ok(None);
    }

    let asset = package_name(&release.version)?;
    if !release
        .assets
        .iter()
        .any(|candidate| candidate.name == asset)
    {
        return Err(Error::BuildNotFound {
            version: release.version,
            asset,
        });
    }
    let download_url = Url::parse(&format!("{RELEASE_DOWNLOAD_ROOT}/{}/{asset}", release.tag))?;
    Ok(Some(Update {
        version: release.version,
        body: release.body.filter(|body| !body.trim().is_empty()),
        download_url,
    }))
}

fn latest_product_release(releases: Vec<GithubRelease>) -> Option<ProductRelease> {
    releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter(|release| {
            !release.tag_name.starts_with("llama.cpp-")
                && !release.tag_name.starts_with("stable-diffusion.cpp-")
        })
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            Some(ProductRelease {
                tag: release.tag_name,
                version,
                body: release.body,
                assets: release.assets,
            })
        })
        .max_by(|left, right| left.version.cmp(&right.version))
}

fn package_name(version: &Version) -> Result<String> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok(format!("Koharu_{version}_x64-setup.exe"));
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok(format!("Koharu_{version}_amd64.AppImage"));
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok(format!("Koharu_{version}_aarch64.app.tar.gz"));
    #[allow(unreachable_code)]
    Err(Error::UnsupportedTarget)
}

fn package_suffix() -> Result<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok(".exe");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok(".AppImage");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok(".app.tar.gz");
    #[allow(unreachable_code)]
    Err(Error::UnsupportedTarget)
}

fn validate_package(path: &Path) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)?;
    #[cfg(target_os = "windows")]
    let valid = magic.starts_with(b"MZ");
    #[cfg(target_os = "linux")]
    let valid = magic == *b"\x7fELF";
    #[cfg(target_os = "macos")]
    let valid = magic.starts_with(&[0x1f, 0x8b]);
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let valid = false;
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidPackage)
    }
}

fn updateable_executable() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .ok_or(Error::NotInstalledPackage)
    }
    #[cfg(not(target_os = "linux"))]
    Ok(std::env::current_exe()?)
}

#[cfg(target_os = "linux")]
fn install(package: TempPath, executable: PathBuf, arguments: Vec<OsString>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let parent = executable.parent().ok_or(Error::NotInstalledPackage)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".koharu-update-")
        .tempfile_in(parent)?;
    std::io::copy(&mut std::fs::File::open(&package)?, &mut staged)?;
    staged.as_file().sync_all()?;
    let permissions = std::fs::metadata(&executable)?.permissions();
    std::fs::set_permissions(staged.path(), permissions)?;
    if std::fs::metadata(staged.path())?.permissions().mode() & 0o111 == 0 {
        return Err(Error::InvalidPackage);
    }

    let backup = tempfile::Builder::new()
        .prefix(".koharu-previous-")
        .tempfile_in(parent)?
        .into_temp_path();
    std::fs::remove_file(&backup)?;
    std::fs::rename(&executable, &backup)?;
    let staged = staged.into_temp_path();
    if let Err(error) = std::fs::rename(&staged, &executable) {
        std::fs::rename(&backup, &executable)?;
        return Err(error.into());
    }
    std::process::Command::new(&executable)
        .args(arguments)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install(package: TempPath, _executable: PathBuf, arguments: Vec<OsString>) -> Result<()> {
    use std::{ffi::OsStr, iter::once, os::windows::ffi::OsStrExt as _};
    use windows_sys::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOW},
        w,
    };

    let installer = package.keep().map_err(|error| error.error)?;
    let escaped_arguments = arguments
        .iter()
        .map(escape_nsis_argument)
        .collect::<Vec<_>>();
    let parameters = ["/P", "/R", "/UPDATE", "/ARGS"]
        .into_iter()
        .map(OsStr::new)
        .chain(escaped_arguments.iter().map(OsStr::new))
        .collect::<Vec<_>>()
        .join(OsStr::new(" "));
    let installer = installer
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let parameters = parameters.encode_wide().chain(once(0)).collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            w!("open"),
            installer.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOW,
        )
    };
    if result as isize <= 32 {
        return Err(Error::InstallerLaunch);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn escape_nsis_argument(argument: &OsString) -> String {
    let argument = argument.to_string_lossy();
    let quote = argument
        .chars()
        .any(|character| character == ' ' || character == '\t' || character == '/')
        || argument.is_empty();
    let mut command = Vec::new();
    if quote {
        command.push('"');
    }
    let mut backslashes = 0_usize;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
        } else {
            if character == '"' {
                command.extend((0..=backslashes).map(|_| '\\'));
            }
            backslashes = 0;
        }
        command.push(character);
    }
    if quote {
        command.extend((0..backslashes).map(|_| '\\'));
        command.push('"');
    }
    command.into_iter().collect()
}

#[cfg(target_os = "macos")]
fn install(package: TempPath, executable: PathBuf, arguments: Vec<OsString>) -> Result<()> {
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
        .ok_or(Error::AppBundleNotFound)?;
    let parent = bundle.parent().ok_or(Error::AppBundleNotFound)?;
    let extraction = tempfile::Builder::new()
        .prefix(".koharu-update-")
        .tempdir_in(parent)
        .or_else(|_| tempfile::Builder::new().prefix("koharu-update-").tempdir())?;
    let archive = std::fs::File::open(&package)?;
    let decoder = flate2::read::GzDecoder::new(archive);
    tar::Archive::new(decoder).unpack(extraction.path())?;
    let updated_bundle = extraction.path().join("Koharu.app");
    if !updated_bundle.join("Contents/MacOS/koharu").is_file() {
        return Err(Error::InvalidPackage);
    }

    match replace_macos_bundle(&bundle, &updated_bundle) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            privileged_replace_macos_bundle(&bundle, &updated_bundle)?;
        }
        Err(error) => return Err(error.into()),
    }
    std::process::Command::new(&executable)
        .args(arguments)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn replace_macos_bundle(bundle: &Path, updated_bundle: &Path) -> std::io::Result<()> {
    let parent = bundle
        .parent()
        .ok_or_else(|| std::io::Error::other("application bundle has no parent"))?;
    let backup = tempfile::Builder::new()
        .prefix(".koharu-previous-")
        .tempdir_in(parent)?;
    let previous_bundle = backup.path().join("Koharu.app");
    std::fs::rename(bundle, &previous_bundle)?;
    if let Err(error) = std::fs::rename(updated_bundle, bundle) {
        std::fs::rename(previous_bundle, bundle)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn privileged_replace_macos_bundle(bundle: &Path, updated_bundle: &Path) -> Result<()> {
    let backup = bundle.with_extension(format!("app.koharu-backup-{}", std::process::id()));
    let shell = format!(
        "test ! -e {backup} && /bin/mv {bundle} {backup} && \
         (/bin/mv {updated} {bundle} && /bin/rm -rf {backup} || \
         (/bin/mv {backup} {bundle}; exit 1))",
        backup = shell_quote(&backup),
        bundle = shell_quote(bundle),
        updated = shell_quote(updated_bundle),
    );
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        shell.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::PrivilegedInstall)
    }
}

#[cfg(target_os = "macos")]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn install(_package: TempPath, _executable: PathBuf, _arguments: Vec<OsString>) -> Result<()> {
    Err(Error::UnsupportedTarget)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn releases(value: serde_json::Value) -> Vec<GithubRelease> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn latest_release_ignores_native_dependency_and_prerelease_streams() {
        let selected = latest_product_release(releases(serde_json::json!([
            {"tag_name":"llama.cpp-b12000","assets":[]},
            {"tag_name":"stable-diffusion.cpp-master-900-deadbeef","assets":[]},
            {"tag_name":"0.70.0-beta.1","prerelease":true,"assets":[]},
            {"tag_name":"0.69.0","body":"notes","assets":[{"name":"build"}]},
            {"tag_name":"0.68.0","assets":[]}
        ])))
        .unwrap();
        assert_eq!(selected.version, Version::new(0, 69, 0));
        assert_eq!(selected.body.as_deref(), Some("notes"));
    }

    #[test]
    fn latest_release_is_selected_by_version_not_api_order() {
        let selected = latest_product_release(releases(serde_json::json!([
            {"tag_name":"v0.65.0","assets":[]},
            {"tag_name":"0.67.0","assets":[]},
            {"tag_name":"0.66.0","assets":[]}
        ])))
        .unwrap();
        assert_eq!(selected.tag, "0.67.0");
    }

    #[test]
    fn package_name_is_bound_to_the_release_version() {
        let version = Version::new(1, 2, 3);
        let name = package_name(&version).unwrap();
        assert!(name.contains("1.2.3"));
        assert!(!name.ends_with(".sig"));
    }

    #[test]
    fn newer_release_without_the_exact_platform_build_is_rejected() {
        let release = ProductRelease {
            tag: "1.2.3".into(),
            version: Version::new(1, 2, 3),
            body: None,
            assets: vec![GithubAsset {
                name: "Koharu_1.2.3_wrong-build.zip".into(),
            }],
        };
        let error = available_update(release, &Version::new(1, 2, 2)).unwrap_err();
        assert!(matches!(error, Error::BuildNotFound { .. }));
    }
}
