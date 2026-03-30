use koharu_runtime::RuntimeManager;

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
                runtime: &koharu_runtime::RuntimeManager,
            ) -> anyhow::Result<std::path::PathBuf> {
                use strum::EnumProperty;
                let repo = self.get_str("repo").expect("repo property");
                let filename = self.get_str("filename").expect("filename property");
                runtime.artifacts().huggingface_model(repo, filename).await
            }
        }

        #[allow(unused)]
        pub async fn prefetch(runtime: &koharu_runtime::RuntimeManager) -> anyhow::Result<()> {
            use futures::stream::{self, StreamExt, TryStreamExt};
            let manifests = [
                $(Manifest::$variant),*
            ];
            stream::iter(manifests)
                .map(|manifest| {
                    let runtime = runtime.clone();
                    async move { manifest.get(&runtime).await }
                })
                .buffer_unordered(num_cpus::get())
                .try_collect::<Vec<_>>()
                .await?;
            Ok(())
        }
    };
}

#[allow(unused)]
pub async fn model(
    runtime: &RuntimeManager,
    repo: &str,
    filename: &str,
) -> anyhow::Result<std::path::PathBuf> {
    runtime.artifacts().huggingface_model(repo, filename).await
}
