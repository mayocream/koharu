#[allow(unused_imports)]
pub use koharu_core::download::model;
#[allow(unused_imports)]
pub use koharu_core::hf_hub::set_cache_dir;

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
            pub async fn get(&self) -> anyhow::Result<std::path::PathBuf> {
                use strum::EnumProperty;
                let repo = self.get_str("repo").expect("repo property");
                let filename = self.get_str("filename").expect("filename property");
                koharu_core::download::model(repo, filename).await
            }
        }

        #[allow(unused)]
        pub async fn prefetch() -> anyhow::Result<()> {
            use futures::stream::{self, StreamExt, TryStreamExt};
            let manifests = [
                $(Manifest::$variant),*
            ];
            stream::iter(manifests)
                .map(|manifest| async move {
                    manifest.get().await
                })
                .buffer_unordered(num_cpus::get())
                .try_collect::<Vec<_>>()
                .await?;
            Ok(())
        }
    };
}
