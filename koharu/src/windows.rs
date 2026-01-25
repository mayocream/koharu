use anyhow::Result;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
};

use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::LibraryLoader::{
    AddDllDirectory, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS,
    SetDefaultDllDirectories,
};

const CLASS_NAME: &str = "Koharu.khr";
// const THUMBNAIL_PROVIDER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

pub fn register_khr() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.create_subkey("Software\\Classes")?.0;

    let (ext_key, _) = classes.create_subkey(".khr")?;
    ext_key.set_value("", &CLASS_NAME)?;
    ext_key.set_value("Content Type", &"image/jpeg")?;
    ext_key.set_value("PerceivedType", &"image")?;
    // let (ext_thumb, _) = ext_key.create_subkey(format!("ShellEx\\{THUMBNAIL_PROVIDER}"))?;
    // ext_thumb.set_value("", &THUMBNAIL_PROVIDER)?;

    let (class_key, _) = classes.create_subkey(CLASS_NAME)?;
    class_key.set_value("", &"Koharu Document")?;
    // let (thumb_key, _) = class_key.create_subkey(format!("ShellEx\\{THUMBNAIL_PROVIDER}"))?;
    // thumb_key.set_value("", &THUMBNAIL_PROVIDER)?;

    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_owned()))
    {
        let (icon_key, _) = class_key.create_subkey("DefaultIcon")?;
        icon_key.set_value("", &format!("{exe},0"))?;
    }
    // add default open with
    let (shell_key, _) = class_key.create_subkey("shell\\open\\command")?;
    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_owned()))
    {
        shell_key.set_value("", &format!("\"{exe}\" \"%1\""))?;
    }

    Ok(())
}

pub fn enable_ansi_support() -> Result<()> {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE)?;
        if handle == HANDLE::default() {
            println!("Failed to get console handle");
            return Ok(());
        }

        let mut mode = std::mem::zeroed();
        GetConsoleMode(handle, &mut mode)?;
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
        Ok(())
    }
}

pub fn create_console_window() {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
    }
}

pub fn add_dll_directory(path: &std::path::Path) -> Result<()> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        if SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_USER_DIRS | LOAD_LIBRARY_SEARCH_SYSTEM32)
            == 0
        {
            anyhow::bail!(
                "Failed to set default DLL directories: {}",
                std::io::Error::last_os_error()
            );
        }
        if AddDllDirectory(wide.as_ptr()).is_null() {
            anyhow::bail!(
                "Failed to add DLL directory: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
}
