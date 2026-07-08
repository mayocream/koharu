use std::{ffi::OsStr, sync::OnceLock};

use libloading::Library;

static SHIM: OnceLock<Library> = OnceLock::new();

pub unsafe fn load(path: impl AsRef<OsStr>) -> Result<(), libloading::Error> {
    if SHIM.get().is_none() {
        let _ = SHIM.set(unsafe { Library::new(path) }?);
    }
    Ok(())
}

pub fn library_name() -> &'static str {
    if cfg!(windows) {
        "koharu_torch_shim.dll"
    } else if cfg!(target_os = "macos") {
        "libkoharu_torch_shim.dylib"
    } else {
        "libkoharu_torch_shim.so"
    }
}

pub(crate) unsafe fn symbol<F: Copy>(name: &[u8]) -> F {
    let library = SHIM
        .get()
        .expect("koharu_torch_shim has not been loaded; call koharu_torch_sys::load first");
    unsafe {
        *library.get::<F>(name).unwrap_or_else(|error| {
            panic!(
                "missing koharu_torch_shim symbol {}: {error}",
                String::from_utf8_lossy(name).trim_end_matches('\0')
            )
        })
    }
}

#[macro_export]
macro_rules! torch_fn {
    ($(
        $(#[$meta:meta])*
        pub fn $name:ident($($arg:ident: $arg_ty:ty),* $(,)?) $(-> $ret:ty)?;
    )*) => {
        $(
            $(#[$meta])*
            #[allow(non_snake_case)]
            pub unsafe fn $name($($arg: $arg_ty),*) $(-> $ret)? {
                type FnPtr = unsafe extern "C" fn($($arg_ty),*) $(-> $ret)?;
                static FN: ::std::sync::OnceLock<FnPtr> = ::std::sync::OnceLock::new();
                let f = FN.get_or_init(|| unsafe {
                    $crate::loader::symbol::<FnPtr>(concat!(stringify!($name), "\0").as_bytes())
                });
                unsafe { f($($arg),*) }
            }
        )*
    };
}
