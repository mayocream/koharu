use std::{
    cell::Cell,
    ffi::CString,
    path::Path,
    sync::{Condvar, Mutex, OnceLock},
};

use crate::{Error, Result};

#[derive(Default)]
struct NativeState {
    active_calls: usize,
    configuring: bool,
}

fn native_state() -> &'static (Mutex<NativeState>, Condvar) {
    static STATE: OnceLock<(Mutex<NativeState>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(NativeState::default()), Condvar::new()))
}

thread_local! {
    static NATIVE_DEPTH: Cell<usize> = const { Cell::new(0) };
    static CONFIGURATION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct NativeCall;

impl NativeCall {
    pub(crate) fn enter() -> Self {
        let (mutex, condvar) = native_state();
        let mut state = mutex.lock().unwrap_or_else(|poison| poison.into_inner());
        while state.configuring {
            state = condvar
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
        state.active_calls += 1;
        drop(state);
        NATIVE_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for NativeCall {
    fn drop(&mut self) {
        NATIVE_DEPTH.with(|depth| depth.set(depth.get() - 1));
        let (mutex, condvar) = native_state();
        let mut state = mutex.lock().unwrap_or_else(|poison| poison.into_inner());
        state.active_calls -= 1;
        if state.active_calls == 0 {
            condvar.notify_all();
        }
    }
}

struct NativeConfiguration;

impl Drop for NativeConfiguration {
    fn drop(&mut self) {
        CONFIGURATION_DEPTH.with(|depth| depth.set(depth.get() - 1));
        let (mutex, condvar) = native_state();
        let mut state = mutex.lock().unwrap_or_else(|poison| poison.into_inner());
        state.configuring = false;
        condvar.notify_all();
    }
}

pub(crate) fn configure_native<T>(configure: impl FnOnce() -> T) -> Result<T> {
    if NATIVE_DEPTH.with(|depth| depth.get() != 0)
        || CONFIGURATION_DEPTH.with(|depth| depth.get() != 0)
    {
        return Err(Error::ReentrantCallbackConfiguration);
    }

    let (mutex, condvar) = native_state();
    let mut state = mutex.lock().unwrap_or_else(|poison| poison.into_inner());
    while state.configuring || state.active_calls != 0 {
        state = condvar
            .wait(state)
            .unwrap_or_else(|poison| poison.into_inner());
    }
    state.configuring = true;
    drop(state);

    CONFIGURATION_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let _configuration = NativeConfiguration;
    Ok(configure())
}

pub(crate) fn cstring(value: &str, field: &'static str) -> Result<CString> {
    CString::new(value).map_err(|source| Error::InteriorNul { field, source })
}

pub(crate) fn optional_cstring(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<CString>> {
    value.map(|value| cstring(value, field)).transpose()
}

pub(crate) fn path_cstring(path: &Path, field: &'static str) -> Result<CString> {
    let value = path.to_str().ok_or_else(|| Error::NonUnicodePath {
        field,
        path: path.to_owned(),
    })?;
    cstring(value, field)
}

pub(crate) fn optional_path_cstring(
    path: Option<&Path>,
    field: &'static str,
) -> Result<Option<CString>> {
    path.map(|path| path_cstring(path, field)).transpose()
}

pub(crate) fn c_int_len(len: usize, field: &'static str) -> Result<i32> {
    i32::try_from(len).map_err(|_| Error::CountOverflow { field, len })
}

pub(crate) fn u32_len(len: usize, field: &'static str) -> Result<u32> {
    u32::try_from(len).map_err(|_| Error::CountOverflow { field, len })
}
