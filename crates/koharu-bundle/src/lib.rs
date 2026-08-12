//! Thin integration between cef-rs' official runtime layout and tauri-bundler.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use koharu_runtime::Torch;
#[cfg(target_os = "linux")]
use tauri_bundler::AppImageSettings;
use tauri_bundler::{
    AppCategory, BundleBinary, BundleSettings, PackageSettings, PackageType, SettingsBuilder,
    bundle_project,
};
#[cfg(target_os = "macos")]
use tauri_bundler::{Entitlements, MacOsSettings};
use thiserror::Error;

const BUNDLE_IDENTIFIER: &str = "rs.koharu.Koharu";

#[cfg(any(target_os = "macos", test))]
const CEF_HELPERS: &[(&str, &str)] = &[
    ("Koharu Helper.app", ""),
    ("Koharu Helper (GPU).app", ".GPU"),
    ("Koharu Helper (Renderer).app", ".Renderer"),
    ("Koharu Helper (Plugin).app", ".Plugin"),
    ("Koharu Helper (Alerts).app", ".Alerts"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Package {
    Nsis,
    AppImage,
    Dmg,
}

impl Package {
    fn tauri(self) -> Vec<PackageType> {
        match self {
            Self::Nsis => vec![PackageType::Nsis],
            Self::AppImage => vec![PackageType::AppImage],
            // Keep the notarized application bundle for the updater archive.
            // Tauri removes it when DMG is the only requested macOS artifact.
            Self::Dmg => vec![PackageType::MacOsBundle, PackageType::Dmg],
        }
    }
}

#[derive(Clone, Debug)]
pub struct BundleConfig {
    pub package: Package,
    pub target: String,
    pub executable: PathBuf,
    pub libraries: Vec<PathBuf>,
    pub ui: PathBuf,
    pub licenses: Vec<PathBuf>,
    pub icon: PathBuf,
    pub output: PathBuf,
    pub version: String,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("package {package:?} cannot be built on {host}")]
    WrongHost {
        package: Package,
        host: &'static str,
    },
    #[error("the cef-rs CEF distribution is unavailable")]
    MissingCef,
    #[error("path has no file name: {0}")]
    InvalidPath(PathBuf),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cef-rs could not prepare its runtime: {0}")]
    Cef(String),
    #[cfg(target_os = "linux")]
    #[error("could not configure runtime-owned libraries: {0}")]
    Runtime(String),
    #[error("tauri-bundler failed: {0}")]
    Tauri(#[from] tauri_bundler::Error),
    #[cfg(target_os = "macos")]
    #[error("Tauri could not sign a CEF helper: {0}")]
    MacOsSign(#[from] tauri_macos_sign::Error),
}

pub type Result<T> = std::result::Result<T, BundleError>;

pub fn bundle(config: &BundleConfig) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(&config.output).map_err(|source| io(&config.output, source))?;
    let mut config = config.clone();
    config.output =
        std::path::absolute(&config.output).map_err(|source| io(config.output.clone(), source))?;
    bundle_inner(&config)
}

fn bundle_inner(config: &BundleConfig) -> Result<Vec<PathBuf>> {
    let binary_name = if cfg!(windows) {
        "koharu.exe"
    } else {
        "koharu"
    };
    copy(&config.executable, &config.output.join(binary_name))?;

    let cef_root = cef_dll_sys::get_cef_dir().ok_or(BundleError::MissingCef)?;
    let prepared = tempfile::tempdir().map_err(|source| io("CEF staging", source))?;
    let cef_runtime = prepare_cef(config, prepared.path())?;

    let mut settings = BundleSettings {
        identifier: Some(BUNDLE_IDENTIFIER.into()),
        icon: Some(vec![path_string(&config.icon)]),
        license: Some("GPL-3.0-only".into()),
        license_file: config.licenses.first().cloned(),
        category: Some(AppCategory::GraphicsAndDesign),
        short_description: Some("Manga translation tools".into()),
        ..Default::default()
    };
    configure_payload(config, &cef_root, &cef_runtime, &mut settings)?;

    let settings = SettingsBuilder::new()
        .project_out_directory(&config.output)
        .package_types(config.package.tauri())
        .package_settings(PackageSettings {
            product_name: "Koharu".into(),
            version: config.version.clone(),
            description: "Manga translation tools".into(),
            homepage: Some("https://koharu.rs".into()),
            authors: Some(vec!["Mayo Takanashi".into()]),
            default_run: Some("koharu".into()),
        })
        .bundle_settings(settings)
        .binaries(vec![BundleBinary::new("koharu".into(), true)])
        .target(config.target.clone())
        .build()?;

    Ok(bundle_project(&settings)?
        .into_iter()
        .flat_map(|bundle| bundle.bundle_paths)
        .collect())
}

#[cfg(target_os = "windows")]
fn prepare_cef(config: &BundleConfig, temporary: &Path) -> Result<PathBuf> {
    require_package(config.package, Package::Nsis)?;
    let runtime = temporary.join("runtime");
    cef::build_util::win::bundle(&runtime, &config.output, "koharu")
        .map_err(|error| BundleError::Cef(error.to_string()))?;
    remove_staged_binary(&runtime, "koharu.exe")?;
    Ok(runtime)
}

#[cfg(target_os = "linux")]
fn prepare_cef(config: &BundleConfig, temporary: &Path) -> Result<PathBuf> {
    require_package(config.package, Package::AppImage)?;
    let runtime = temporary.join("runtime");
    cef::build_util::linux::bundle(&runtime, &config.output, "koharu")
        .map_err(|error| BundleError::Cef(error.to_string()))?;
    remove_staged_binary(&runtime, "koharu")?;
    Ok(runtime)
}

#[cfg(target_os = "macos")]
fn prepare_cef(config: &BundleConfig, temporary: &Path) -> Result<PathBuf> {
    require_package(config.package, Package::Dmg)?;
    let targets = temporary.join("targets");
    fs::create_dir_all(&targets).map_err(|source| io(&targets, source))?;
    copy(&config.executable, &targets.join("Koharu"))?;
    copy(&config.executable, &targets.join("koharu-helper"))?;
    let version = semver::Version::parse(&config.version)
        .map_err(|error| BundleError::Cef(format!("invalid version: {error}")))?;
    let app = cef::build_util::mac::bundle(
        temporary,
        &targets,
        "Koharu",
        "koharu-helper",
        None,
        cef::build_util::mac::BundleInfo::new("Koharu", BUNDLE_IDENTIFIER, "Koharu", "en", version),
    )
    .map_err(|error| BundleError::Cef(error.to_string()))?;
    set_cef_helper_identifiers(&app)?;
    Ok(app)
}

#[cfg(any(target_os = "macos", test))]
fn cef_helper_identifier(role: &str) -> String {
    format!("{BUNDLE_IDENTIFIER}.helper{role}")
}

#[cfg(target_os = "macos")]
fn set_cef_helper_identifiers(app: &Path) -> Result<()> {
    for (helper, role) in CEF_HELPERS {
        let path = app
            .join("Contents/Frameworks")
            .join(helper)
            .join("Contents/Info.plist");
        let mut value = plist::Value::from_file(&path).map_err(|error| {
            BundleError::Cef(format!("failed to read {}: {error}", path.display()))
        })?;
        let dictionary = value.as_dictionary_mut().ok_or_else(|| {
            BundleError::Cef(format!(
                "{} is not a property-list dictionary",
                path.display()
            ))
        })?;
        dictionary.insert(
            "CFBundleIdentifier".into(),
            plist::Value::String(cef_helper_identifier(role)),
        );
        value.to_file_xml(&path).map_err(|error| {
            BundleError::Cef(format!("failed to write {}: {error}", path.display()))
        })?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn prepare_cef(config: &BundleConfig, _: &Path) -> Result<PathBuf> {
    Err(BundleError::WrongHost {
        package: config.package,
        host: std::env::consts::OS,
    })
}

#[cfg(target_os = "windows")]
fn configure_payload(
    config: &BundleConfig,
    cef_root: &Path,
    cef_runtime: &Path,
    settings: &mut BundleSettings,
) -> Result<()> {
    let mut resources = HashMap::from([
        (path_string(cef_runtime), ".".into()),
        (path_string(&config.ui), "resources/ui".into()),
    ]);
    insert_licenses(config, cef_root, "resources/licenses", &mut resources)?;
    for library in &config.libraries {
        resources.insert(path_string(library), ".".into());
    }
    settings.resources_map = Some(resources);
    Ok(())
}

#[cfg(target_os = "linux")]
fn configure_payload(
    config: &BundleConfig,
    cef_root: &Path,
    cef_runtime: &Path,
    settings: &mut BundleSettings,
) -> Result<()> {
    let mut files = HashMap::from([
        (PathBuf::from("/usr/lib"), cef_runtime.to_path_buf()),
        (PathBuf::from("/usr/lib/resources/ui"), config.ui.clone()),
    ]);
    insert_linux_licenses(config, cef_root, &mut files)?;
    for library in &config.libraries {
        let name = library
            .file_name()
            .ok_or_else(|| BundleError::InvalidPath(library.clone()))?;
        files.insert(PathBuf::from("/usr/lib").join(name), library.clone());
    }
    settings.appimage = AppImageSettings {
        files,
        // Torch backends are installed and activated at runtime; the CPU
        // package used to link the shim must not become AppImage payload.
        exclude_libraries: Torch::CPU
            .library_names()
            .map_err(|error| BundleError::Runtime(error.to_string()))?
            .map(str::to_owned)
            .collect(),
        ..Default::default()
    };
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_payload(
    config: &BundleConfig,
    cef_root: &Path,
    cef_app: &Path,
    settings: &mut BundleSettings,
) -> Result<()> {
    let frameworks = cef_app.join("Contents/Frameworks");
    let cef_framework = frameworks.join("Chromium Embedded Framework.framework");
    let mut signed_frameworks = vec![path_string(&cef_framework)];
    signed_frameworks.extend(config.libraries.iter().map(|path| path_string(path)));

    let signing_identity = std::env::var("APPLE_SIGNING_IDENTITY").ok();
    let keychain = signing_identity
        .as_deref()
        .map(tauri_macos_sign::Keychain::with_signing_identity);
    let entitlements = Path::new(".github/signing/macos-entitlements.plist");
    let mut files = HashMap::new();
    for suffix in ["", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"] {
        let name = format!("Koharu Helper{suffix}.app");
        let source = frameworks.join(&name);
        if let Some(keychain) = &keychain {
            keychain.sign(&source, Some(entitlements), true)?;
        }
        files.insert(PathBuf::from("Frameworks").join(&name), source);
    }
    settings.macos = MacOsSettings {
        frameworks: Some(signed_frameworks),
        files,
        minimum_system_version: Some("12.0".into()),
        hardened_runtime: true,
        signing_identity,
        entitlements: Some(Entitlements::Path(entitlements.to_path_buf())),
        ..Default::default()
    };
    let mut resources = HashMap::from([(path_string(&config.ui), "ui".into())]);
    insert_licenses(config, cef_root, "licenses", &mut resources)?;
    settings.resources_map = Some(resources);
    Ok(())
}

#[allow(unused)]
fn insert_licenses(
    config: &BundleConfig,
    cef_root: &Path,
    target: &str,
    resources: &mut HashMap<String, String>,
) -> Result<()> {
    for license in &config.licenses {
        resources.insert(path_string(license), target.into());
    }
    for name in ["CREDITS.html", "archive.json"] {
        let source = cef_root.join(name);
        resources.insert(path_string(&source), target.into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn insert_linux_licenses(
    config: &BundleConfig,
    cef_root: &Path,
    files: &mut HashMap<PathBuf, PathBuf>,
) -> Result<()> {
    let target = PathBuf::from("/usr/lib/resources/licenses");
    for license in &config.licenses {
        let name = license
            .file_name()
            .ok_or_else(|| BundleError::InvalidPath(license.clone()))?;
        files.insert(target.join(name), license.clone());
    }
    for name in ["CREDITS.html", "archive.json"] {
        let source = cef_root.join(name);
        files.insert(target.join(name), source);
    }
    Ok(())
}

fn require_package(actual: Package, expected: Package) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(BundleError::WrongHost {
            package: actual,
            host: std::env::consts::OS,
        })
    }
}

#[allow(unused)]
fn remove_staged_binary(runtime: &Path, name: &str) -> Result<()> {
    let path = runtime.join(name);
    fs::remove_file(&path).map_err(|source| io(path, source))
}

fn copy(source: &Path, target: &Path) -> Result<()> {
    fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| io(target, error))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn io(path: impl Into<PathBuf>, source: std::io::Error) -> BundleError {
    BundleError::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_mapping_uses_tauri_artifacts() {
        assert_eq!(Package::Nsis.tauri(), [PackageType::Nsis]);
        assert_eq!(Package::AppImage.tauri(), [PackageType::AppImage]);
        assert_eq!(
            Package::Dmg.tauri(),
            [PackageType::MacOsBundle, PackageType::Dmg]
        );
    }

    #[test]
    fn bundle_identity_is_stable_and_distinct_from_the_product_name() {
        assert_eq!(BUNDLE_IDENTIFIER, "rs.koharu.Koharu");
        assert_ne!(BUNDLE_IDENTIFIER, "Koharu");
    }

    #[test]
    fn cef_helper_bundles_have_unique_identifiers() {
        let identifiers = CEF_HELPERS
            .iter()
            .map(|(_, role)| cef_helper_identifier(role))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(identifiers.len(), CEF_HELPERS.len());
        assert!(identifiers.contains("rs.koharu.Koharu.helper"));
        assert!(identifiers.contains("rs.koharu.Koharu.helper.Renderer"));
    }
}
