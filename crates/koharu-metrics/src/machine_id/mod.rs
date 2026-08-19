#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(super) use linux::get;
#[cfg(target_os = "macos")]
pub(super) use macos::get;
#[cfg(target_os = "windows")]
pub(super) use windows::get;

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub(super) fn get() -> anyhow::Result<String> {
    anyhow::bail!("machine identifiers are unsupported on this platform")
}

#[cfg(test)]
mod tests {
    #[test]
    fn machine_id_is_available_and_stable() {
        let first = super::get().unwrap();
        let second = super::get().unwrap();
        assert!(!first.is_empty());
        assert_eq!(first, second);
    }
}
