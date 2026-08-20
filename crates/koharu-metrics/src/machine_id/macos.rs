use anyhow::Result;

pub(crate) fn get() -> Result<String> {
    let mut value = [0_u8; 16];
    let timeout = libc::timespec {
        tv_sec: 5,
        tv_nsec: 0,
    };
    let status = unsafe { libc::gethostuuid(value.as_mut_ptr(), &timeout) };
    anyhow::ensure!(status == 0, "gethostuuid failed with status {status}");
    Ok(value.iter().map(|byte| format!("{byte:02x}")).collect())
}
