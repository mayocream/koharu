use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use goblin::{Object, mach};

/// Copies native libraries into a temporary directory and isolates local imports.
pub fn isolate(paths: &[PathBuf]) -> Result<PathBuf> {
    let entry = paths.last().context("empty native library list")?;
    let modules = paths
        .iter()
        .map(|path| (module_name(path), path.clone()))
        .collect::<BTreeMap<_, _>>();

    let entry = module_name(entry);
    let mut aliases = BTreeMap::new();
    for path in modules.values() {
        let bytes = fs::read(path)?;
        for name in imports(&bytes)? {
            let name = name
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name != entry && modules.contains_key(&name) {
                aliases.entry(name.clone()).or_insert_with(|| alias(&name));
            }
        }
    }

    // The returned directory must remain valid after this function returns.
    let temporary = tempfile::tempdir()?.keep();
    for (name, source) in modules {
        let mut bytes = fs::read(source)?;
        for (import, alias) in &aliases {
            for offset in 0..=bytes.len().saturating_sub(import.len()) {
                if bytes[offset..].starts_with(import.as_bytes()) {
                    bytes[offset..offset + alias.len()].copy_from_slice(alias.as_bytes());
                }
            }
        }
        let output = temporary.join(aliases.get(&name).unwrap_or(&name));
        fs::write(&output, bytes)?;
    }

    Ok(temporary)
}

fn imports(bytes: &[u8]) -> Result<Vec<&str>> {
    Ok(match Object::parse(bytes)? {
        Object::PE(pe) => pe.libraries,
        Object::Elf(elf) => elf.libraries,
        Object::Mach(mach::Mach::Binary(macho)) => macho.libs,
        Object::Mach(mach::Mach::Fat(fat)) => fat
            .into_iter()
            .filter_map(|arch| match arch.ok()? {
                mach::SingleArch::MachO(macho) => Some(macho.libs),
                mach::SingleArch::Archive(_) => None,
            })
            .flatten()
            .collect(),
        _ => Vec::new(),
    })
}

pub fn alias(module: &str) -> String {
    let end = module
        .find(".so")
        .or_else(|| module.rfind('.'))
        .unwrap_or(module.len());
    let prefix = module[..end].strip_prefix("lib").map_or("", |_| "lib");
    let hash = module.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let hash = format!("{hash:016x}");
    format!(
        "{prefix}{}{}",
        hash.chars()
            .cycle()
            .take(end - prefix.len())
            .collect::<String>(),
        &module[end..]
    )
}

fn module_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
