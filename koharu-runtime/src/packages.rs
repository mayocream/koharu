use std::sync::Arc;

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use indexmap::IndexMap;

use crate::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Native,
    Model,
}

pub type PackageFuture<'a> = BoxFuture<'a, Result<()>>;

pub struct PackageRegistration {
    pub id: &'static str,
    pub kind: PackageKind,
    pub bootstrap: bool,
    pub order: u32,
    pub enabled: fn(&Runtime) -> bool,
    pub present: fn(&Runtime) -> Result<bool>,
    pub ensure: for<'a> fn(&'a Runtime) -> PackageFuture<'a>,
}

inventory::collect!(PackageRegistration);

#[derive(Clone)]
pub struct PackageCatalog {
    packages: Arc<IndexMap<&'static str, &'static PackageRegistration>>,
}

impl PackageCatalog {
    pub fn discover() -> Self {
        let mut discovered = inventory::iter::<PackageRegistration>
            .into_iter()
            .collect::<Vec<_>>();
        discovered.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.id.cmp(right.id))
        });

        let mut packages = IndexMap::with_capacity(discovered.len());
        for package in discovered {
            let replaced = packages.insert(package.id, package);
            assert!(
                replaced.is_none(),
                "duplicate runtime package id registered"
            );
        }

        Self {
            packages: Arc::new(packages),
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &'static PackageRegistration> + '_ {
        self.packages.values().copied()
    }

    pub async fn prepare_bootstrap(&self, runtime: &Runtime) -> Result<()> {
        for package in self
            .all()
            .filter(|package| package.bootstrap)
            .filter(|package| (package.enabled)(runtime))
        {
            (package.ensure)(runtime)
                .await
                .with_context(|| format!("failed to prepare package `{}`", package.id))?;
        }

        Ok(())
    }
}

#[macro_export]
macro_rules! declare_hf_model_package {
    (
        id: $id:literal,
        repo: $repo:expr,
        file: $file:expr,
        bootstrap: $bootstrap:expr,
        order: $order:expr
        $(,)?
    ) => {
        const _: () = {
            fn enabled(_: &$crate::Runtime) -> bool {
                true
            }

            fn present(runtime: &$crate::Runtime) -> anyhow::Result<bool> {
                Ok(
                    $crate::hf_hub::Cache::new(runtime.root().join("models").join("huggingface"))
                        .model($repo.to_string())
                        .get($file)
                        .is_some(),
                )
            }

            fn ensure(runtime: &$crate::Runtime) -> $crate::packages::PackageFuture<'_> {
                Box::pin(async move {
                    runtime.downloads().huggingface_model($repo, $file).await?;
                    Ok(())
                })
            }

            $crate::inventory::submit! {
                $crate::packages::PackageRegistration {
                    id: $id,
                    kind: $crate::packages::PackageKind::Model,
                    bootstrap: $bootstrap,
                    order: $order,
                    enabled,
                    present,
                    ensure,
                }
            }
        };
    };
}

#[macro_export]
macro_rules! declare_native_package {
    (
        id: $id:literal,
        bootstrap: $bootstrap:expr,
        order: $order:expr,
        enabled: $enabled:path,
        present: $present:path,
        prepare: $prepare:path
        $(,)?
    ) => {
        const _: () = {
            fn ensure(runtime: &$crate::Runtime) -> $crate::packages::PackageFuture<'_> {
                Box::pin(async move { $prepare(runtime).await })
            }

            $crate::inventory::submit! {
                $crate::packages::PackageRegistration {
                    id: $id,
                    kind: $crate::packages::PackageKind::Native,
                    bootstrap: $bootstrap,
                    order: $order,
                    enabled: $enabled,
                    present: $present,
                    ensure,
                }
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::{PackageCatalog, PackageKind};

    #[test]
    fn catalog_is_sorted_by_order_then_id() {
        let catalog = PackageCatalog::discover();
        let packages = catalog.all().collect::<Vec<_>>();

        assert!(packages.windows(2).all(|pair| {
            let left = pair[0];
            let right = pair[1];
            left.order < right.order || (left.order == right.order && left.id <= right.id)
        }));
    }

    #[test]
    fn catalog_contains_native_packages() {
        let catalog = PackageCatalog::discover();
        assert!(
            catalog
                .all()
                .any(|package| package.kind == PackageKind::Native)
        );
    }
}
