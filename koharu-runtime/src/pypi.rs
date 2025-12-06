use std::time::{Duration, Instant};

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use strum::Display;

pub static PYPI_ENDPOINT: once_cell::sync::Lazy<&PypiEndpoint> = once_cell::sync::Lazy::new(|| {
    [PypiEndpoint::Official, PypiEndpoint::Tsinghua]
        .par_iter()
        .map(|endpoint| {
            let start = Instant::now();
            let url = format!("{endpoint}/pypi/sampleproject/json");
            let resp = reqwest::blocking::get(&url);
            match resp {
                Ok(resp) if resp.status().is_success() => {
                    let duration = start.elapsed();
                    (duration, endpoint)
                }
                _ => (Duration::MAX, endpoint),
            }
        })
        .min_by_key(|(duration, _)| *duration)
        .map(|(_, endpoint)| endpoint)
        .unwrap_or_else(|| &PypiEndpoint::Official)
});

#[derive(Debug, Clone, Display)]
pub enum PypiEndpoint {
    #[strum(serialize = "https://pypi.org")]
    Official,
    #[strum(serialize = "https://mirrors.tuna.tsinghua.edu.cn")]
    Tsinghua,
}

impl PypiEndpoint {
    pub fn refine_url(&self, path: &str) -> String {
        match self {
            PypiEndpoint::Tsinghua => path.replace(
                "https://files.pythonhosted.org",
                format!("{self}/pypi/web").as_str(),
            ),
            _ => path.to_string(),
        }
    }
}
