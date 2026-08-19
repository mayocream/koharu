use anyhow::{Context as _, Result};
use windows::{
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
    },
    core::w,
};

pub(crate) fn get() -> Result<String> {
    let mut size = 0_u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "failed to read MachineGuid size: {status:?}"
    );
    anyhow::ensure!(
        size >= 2 && size.is_multiple_of(2),
        "MachineGuid has an invalid size"
    );
    let mut value = vec![0_u16; size as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "failed to read MachineGuid: {status:?}"
    );
    let value = String::from_utf16(&value)
        .context("MachineGuid is not valid UTF-16")?
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    anyhow::ensure!(!value.is_empty(), "MachineGuid is empty");
    Ok(value)
}
