use std::sync::{LazyLock, RwLock};

use anyhow::Context;
use koharu_core::{MirrorKind, MirrorSelection};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const OFFICIAL_PYPI_BASE: &str = "https://pypi.org";

#[derive(Debug, Clone)]
pub struct DownloadSettings {
    pub proxy_url: Option<String>,
    pub pypi_mirror: MirrorSelection,
    pub github_mirror: MirrorSelection,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            proxy_url: None,
            pypi_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
            github_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
        }
    }
}

struct HttpState {
    settings: DownloadSettings,
    client: ClientWithMiddleware,
}

static HTTP_STATE: LazyLock<RwLock<HttpState>> = LazyLock::new(|| {
    let settings = DownloadSettings::default();
    RwLock::new(HttpState {
        client: build_client(&settings).expect("build default HTTP client"),
        settings,
    })
});

pub fn http_client() -> ClientWithMiddleware {
    HTTP_STATE.read().expect("read HTTP state").client.clone()
}

pub fn download_settings() -> DownloadSettings {
    HTTP_STATE.read().expect("read HTTP state").settings.clone()
}

pub fn set_download_settings(settings: DownloadSettings) -> anyhow::Result<()> {
    apply_proxy_env(&settings.proxy_url);
    let client = build_client(&settings)?;
    *HTTP_STATE.write().expect("write HTTP state") = HttpState { settings, client };
    Ok(())
}

pub fn pypi_metadata_url(dist: &str, version: &str) -> String {
    let base = match &download_settings().pypi_mirror {
        MirrorSelection {
            kind: MirrorKind::Custom,
            custom_base_url: Some(base),
        } if !base.trim().is_empty() => base.trim().trim_end_matches('/').to_string(),
        _ => OFFICIAL_PYPI_BASE.to_string(),
    };

    format!("{base}/pypi/{dist}/{version}/json")
}

pub fn rewrite_url(url: &str) -> String {
    let settings = download_settings();
    let github = rewrite_github_url(url, &settings.github_mirror);
    rewrite_pypi_url(&github, &settings.pypi_mirror)
}

fn build_client(settings: &DownloadSettings) -> anyhow::Result<ClientWithMiddleware> {
    let mut builder = reqwest::Client::builder().user_agent(USER_AGENT);
    if let Some(proxy_url) = settings.proxy_url.as_deref().map(str::trim)
        && !proxy_url.is_empty()
    {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy URL")?);
    }

    let client = builder.build().context("build reqwest client")?;
    Ok(ClientBuilder::new(client)
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .build())
}

fn rewrite_github_url(url: &str, mirror: &MirrorSelection) -> String {
    if !is_github_url(url) {
        return url.to_string();
    }

    match mirror {
        MirrorSelection {
            kind: MirrorKind::Custom,
            custom_base_url: Some(base),
        } if !base.trim().is_empty() => apply_custom_mirror(base, url),
        _ => url.to_string(),
    }
}

fn rewrite_pypi_url(url: &str, mirror: &MirrorSelection) -> String {
    match mirror {
        MirrorSelection {
            kind: MirrorKind::Custom,
            custom_base_url: Some(base),
        } if !base.trim().is_empty() => {
            if url.starts_with(OFFICIAL_PYPI_BASE) {
                return replace_origin(url, base);
            }
            if url.starts_with("https://files.pythonhosted.org") {
                return replace_origin(url, base);
            }
            url.to_string()
        }
        _ => url.to_string(),
    }
}

fn is_github_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
        || url.starts_with("http://github.com/")
        || url.starts_with("https://objects.githubusercontent.com/")
        || url.starts_with("https://github-releases.githubusercontent.com/")
}

fn apply_custom_mirror(base: &str, url: &str) -> String {
    let base = base.trim();
    if base.contains("{url}") {
        base.replace("{url}", url)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), url)
    }
}

fn replace_origin(url: &str, base: &str) -> String {
    match url.find("://") {
        Some(scheme_end) => match url[scheme_end + 3..].find('/') {
            Some(path_start) => {
                let path = &url[scheme_end + 3 + path_start..];
                format!("{}{}", base.trim_end_matches('/'), path)
            }
            None => base.trim_end_matches('/').to_string(),
        },
        None => url.to_string(),
    }
}

fn apply_proxy_env(proxy_url: &Option<String>) {
    match proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(proxy) => unsafe {
            std::env::set_var("HTTP_PROXY", proxy);
            std::env::set_var("HTTPS_PROXY", proxy);
            std::env::set_var("ALL_PROXY", proxy);
        },
        None => unsafe {
            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("HTTPS_PROXY");
            std::env::remove_var("ALL_PROXY");
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadSettings, OFFICIAL_PYPI_BASE, apply_custom_mirror, pypi_metadata_url,
        replace_origin, rewrite_url, set_download_settings,
    };
    use koharu_core::{MirrorKind, MirrorSelection};

    #[test]
    fn pypi_metadata_uses_custom_base_when_configured() {
        set_download_settings(DownloadSettings {
            proxy_url: None,
            pypi_mirror: MirrorSelection {
                kind: MirrorKind::Custom,
                custom_base_url: Some("https://mirror.example".to_string()),
            },
            github_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
        })
        .unwrap();

        assert_eq!(
            pypi_metadata_url("nvidia-cuda-runtime", "13.1.80"),
            "https://mirror.example/pypi/nvidia-cuda-runtime/13.1.80/json"
        );
    }

    #[test]
    fn github_rewrite_prefixes_full_url() {
        set_download_settings(DownloadSettings {
            proxy_url: None,
            pypi_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
            github_mirror: MirrorSelection {
                kind: MirrorKind::Custom,
                custom_base_url: Some("https://ghproxy.example".to_string()),
            },
        })
        .unwrap();

        assert_eq!(
            rewrite_url("https://github.com/ggml-org/llama.cpp/releases/download/a/b.zip"),
            "https://ghproxy.example/https://github.com/ggml-org/llama.cpp/releases/download/a/b.zip"
        );
    }

    #[test]
    fn helper_functions_keep_path_when_replacing_origin() {
        assert_eq!(
            replace_origin(
                &format!("{OFFICIAL_PYPI_BASE}/files/demo.whl"),
                "https://mirror.example"
            ),
            "https://mirror.example/files/demo.whl"
        );
        assert_eq!(
            apply_custom_mirror("https://mirror/{url}", "https://github.com/demo"),
            "https://mirror/https://github.com/demo"
        );
    }
}
