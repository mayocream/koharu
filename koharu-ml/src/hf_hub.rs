use std::path::{Path, PathBuf};

pub async fn cached_model_path(
    models_root: &Path,
    repo: &str,
    filename: &str,
) -> anyhow::Result<PathBuf> {
    koharu_runtime::download::cached_model_path(models_root, repo, filename)
}

#[macro_export]
macro_rules! define_models {
    ($($variant:ident => ($repo:literal, $filename:literal)),* $(,)?) => {
        #[derive(Debug, Clone, strum::EnumIter, strum::EnumProperty)]
        pub enum Manifest {
            $(
                #[strum(props(repo = $repo, filename = $filename))]
                $variant,
            )*
        }

        impl Manifest {
            pub async fn get(
                &self,
                models_root: &std::path::Path,
            ) -> anyhow::Result<std::path::PathBuf> {
                use strum::EnumProperty;
                let repo = self.get_str("repo").expect("repo property");
                let filename = self.get_str("filename").expect("filename property");
                $crate::hf_hub::cached_model_path(models_root, repo, filename).await
            }
        }

        #[allow(unused)]
        pub fn component_assets() -> Vec<(&'static str, &'static str)> {
            vec![
                $(($repo, $filename)),*
            ]
        }

        #[allow(unused)]
        pub fn manifest_registry_entries(
            priority: u32,
            required: impl Fn(Manifest) -> bool,
        ) -> Vec<koharu_runtime::registry::BootstrapEntry> {
            use strum::{EnumProperty, IntoEnumIterator};

            Manifest::iter()
                .map(|manifest| {
                    let repo = manifest.get_str("repo").expect("repo property");
                    let filename = manifest.get_str("filename").expect("filename property");
                    koharu_runtime::registry::BootstrapEntry::model(
                        format!("hf:{repo}:{filename}"),
                        filename.to_string(),
                        priority,
                        required(manifest.clone()),
                        repo,
                        filename,
                    )
                })
                .collect()
        }
    };
}
