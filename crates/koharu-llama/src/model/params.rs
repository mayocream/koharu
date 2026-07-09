//! A safe wrapper around `llama_model_params`.

use crate::LlamaCppError;
use crate::context::params::LlamaContextParams;
use crate::model::params::kv_overrides::KvOverrides;
use std::ffi::{CStr, c_char, c_void};
use std::fmt::{Debug, Formatter};
use std::pin::Pin;
use std::ptr::null;

pub mod kv_overrides;

/// Result of [`LlamaModelParams::fit_params`], containing the fitted context size.
#[derive(Debug, Clone)]
pub struct FitResult {
    /// The context size after fitting (may have been reduced from the requested value).
    pub n_ctx: u32,
}

/// Error returned by [`LlamaModelParams::fit_params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FitError {
    /// Could not find allocations that are projected to fit available memory.
    #[error("could not find allocations that fit available memory")]
    Failure,
    /// A hard error occurred during fitting (e.g. model not found at the specified path).
    #[error("hard error during parameter fitting")]
    Error,
}

#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_possible_truncation)]
const LLAMA_SPLIT_MODE_NONE: i8 = koharu_llama_sys::LLAMA_SPLIT_MODE_NONE as i8;
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_possible_truncation)]
const LLAMA_SPLIT_MODE_LAYER: i8 = koharu_llama_sys::LLAMA_SPLIT_MODE_LAYER as i8;
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_possible_truncation)]
const LLAMA_SPLIT_MODE_ROW: i8 = koharu_llama_sys::LLAMA_SPLIT_MODE_ROW as i8;
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_possible_truncation)]
/// A rusty wrapper around `llama_split_mode`.
#[repr(i8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LlamaSplitMode {
    /// Single GPU
    None = LLAMA_SPLIT_MODE_NONE,
    /// Split layers and KV across GPUs
    Layer = LLAMA_SPLIT_MODE_LAYER,
    /// Split layers and KV across GPUs, use tensor parallelism if supported
    Row = LLAMA_SPLIT_MODE_ROW,
}

/// An error that occurs when unknown split mode is encountered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlamaSplitModeParseError(pub i32);

/// Create a `LlamaSplitMode` from a `i32`.
///
/// # Errors
/// Returns `LlamaSplitModeParseError` if the value does not correspond to a valid `LlamaSplitMode`.
impl TryFrom<i32> for LlamaSplitMode {
    type Error = LlamaSplitModeParseError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        let i8_value = value
            .try_into()
            .map_err(|_| LlamaSplitModeParseError(value))?;
        match i8_value {
            LLAMA_SPLIT_MODE_NONE => Ok(Self::None),
            LLAMA_SPLIT_MODE_LAYER => Ok(Self::Layer),
            LLAMA_SPLIT_MODE_ROW => Ok(Self::Row),
            _ => Err(LlamaSplitModeParseError(value)),
        }
    }
}

/// Create a `LlamaSplitMode` from a `u32`.
///
/// # Errors
/// Returns `LlamaSplitModeParseError` if the value does not correspond to a valid `LlamaSplitMode`.
impl TryFrom<u32> for LlamaSplitMode {
    type Error = LlamaSplitModeParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let i8_value = value
            .try_into()
            .map_err(|_| LlamaSplitModeParseError(value.try_into().unwrap_or(i32::MAX)))?;
        match i8_value {
            LLAMA_SPLIT_MODE_NONE => Ok(Self::None),
            LLAMA_SPLIT_MODE_LAYER => Ok(Self::Layer),
            LLAMA_SPLIT_MODE_ROW => Ok(Self::Row),
            _ => Err(LlamaSplitModeParseError(
                value.try_into().unwrap_or(i32::MAX),
            )),
        }
    }
}

/// Create a `i32` from a `LlamaSplitMode`.
impl From<LlamaSplitMode> for i32 {
    fn from(value: LlamaSplitMode) -> Self {
        match value {
            LlamaSplitMode::None => LLAMA_SPLIT_MODE_NONE.into(),
            LlamaSplitMode::Layer => LLAMA_SPLIT_MODE_LAYER.into(),
            LlamaSplitMode::Row => LLAMA_SPLIT_MODE_ROW.into(),
        }
    }
}

/// Create a `u32` from a `LlamaSplitMode`.
impl From<LlamaSplitMode> for u32 {
    fn from(value: LlamaSplitMode) -> Self {
        match value {
            LlamaSplitMode::None => LLAMA_SPLIT_MODE_NONE as u32,
            LlamaSplitMode::Layer => LLAMA_SPLIT_MODE_LAYER as u32,
            LlamaSplitMode::Row => LLAMA_SPLIT_MODE_ROW as u32,
        }
    }
}

/// The default split mode is `Layer` in llama.cpp.
impl Default for LlamaSplitMode {
    fn default() -> Self {
        LlamaSplitMode::Layer
    }
}

/// The maximum number of devices supported.
///
/// The real maximum number of devices is the lesser one of this value and the value returned by
/// `koharu_llama::max_devices()`.
pub const LLAMA_CPP_MAX_DEVICES: usize = 16;

/// A safe wrapper around `llama_model_params`.
#[allow(clippy::module_name_repetitions)]
pub struct LlamaModelParams {
    pub(crate) params: koharu_llama_sys::llama_model_params,
    kv_overrides: Vec<koharu_llama_sys::llama_model_kv_override>,
    buft_overrides: Vec<koharu_llama_sys::llama_model_tensor_buft_override>,
    devices: Pin<Box<[koharu_llama_sys::ggml_backend_dev_t; LLAMA_CPP_MAX_DEVICES]>>,
    tensor_split: Vec<f32>,
    progress_callback: Option<Box<dyn FnMut(f32) -> bool>>,
}

impl Debug for LlamaModelParams {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaModelParams")
            .field("n_gpu_layers", &self.params.n_gpu_layers)
            .field("main_gpu", &self.params.main_gpu)
            .field("vocab_only", &self.params.vocab_only)
            .field("use_mmap", &self.params.use_mmap)
            .field("use_mlock", &self.params.use_mlock)
            .field("split_mode", &self.split_mode())
            .field("devices", &self.devices)
            .field("kv_overrides", &"vec of kv_overrides")
            .finish()
    }
}

impl LlamaModelParams {
    /// Automatically fit model parameters to available device memory.
    ///
    /// Wraps llama.cpp's `llama_params_fit`, which determines optimal `n_gpu_layers`,
    /// `tensor_split`, and `tensor_buft_overrides` based on available memory. On success
    /// the model and context params are updated in place.
    pub fn fit_params(
        mut self: Pin<&mut Self>,
        model_path: &CStr,
        cparams: &mut LlamaContextParams,
        margins: &mut [usize],
        n_ctx_min: u32,
        log_level: koharu_llama_sys::ggml_log_level,
    ) -> Result<FitResult, FitError> {
        let max_devices = unsafe { koharu_llama_sys::llama_max_devices() };
        let max_buft = unsafe { koharu_llama_sys::llama_max_tensor_buft_overrides() };

        self.tensor_split.clear();
        self.tensor_split.resize(max_devices, 0.0);

        self.buft_overrides.clear();
        self.buft_overrides.resize(
            max_buft + 1,
            koharu_llama_sys::llama_model_tensor_buft_override {
                pattern: std::ptr::null(),
                buft: std::ptr::null_mut(),
            },
        );

        self.params.tensor_split = null::<f32>();
        self.params.tensor_buft_overrides = null();

        let status = unsafe {
            koharu_llama_sys::llama_params_fit(
                model_path.as_ptr(),
                &raw mut self.params,
                &raw mut cparams.context_params,
                self.tensor_split.as_mut_ptr(),
                self.buft_overrides.as_mut_ptr(),
                margins.as_mut_ptr(),
                n_ctx_min,
                log_level,
            )
        };

        match status {
            koharu_llama_sys::LLAMA_PARAMS_FIT_STATUS_SUCCESS => {}
            koharu_llama_sys::LLAMA_PARAMS_FIT_STATUS_FAILURE => return Err(FitError::Failure),
            _ => return Err(FitError::Error),
        }

        self.params.tensor_split = self.tensor_split.as_ptr();
        self.params.tensor_buft_overrides = self.buft_overrides.as_ptr();

        Ok(FitResult {
            n_ctx: cparams.context_params.n_ctx,
        })
    }
}

impl LlamaModelParams {
    /// See [`KvOverrides`]
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use koharu_llama::model::params::LlamaModelParams;
    /// let params = Box::pin(LlamaModelParams::default());
    /// let kv_overrides = params.kv_overrides();
    /// let count = kv_overrides.into_iter().count();
    /// assert_eq!(count, 0);
    /// ```
    #[must_use]
    pub fn kv_overrides<'a>(&'a self) -> KvOverrides<'a> {
        KvOverrides::new(self)
    }

    /// Appends a key-value override to the model parameters. It must be pinned as this creates a self-referential struct.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::ffi::{CStr, CString};
    /// use std::pin::pin;
    /// # use koharu_llama::model::params::LlamaModelParams;
    /// # use koharu_llama::model::params::kv_overrides::ParamOverrideValue;
    /// let mut params = pin!(LlamaModelParams::default());
    /// let key = CString::new("key").expect("CString::new failed");
    /// params.as_mut().append_kv_override(&key, ParamOverrideValue::Int(50));
    ///
    /// let kv_overrides = params.kv_overrides().into_iter().collect::<Vec<_>>();
    /// assert_eq!(kv_overrides.len(), 1);
    ///
    /// let (k, v) = &kv_overrides[0];
    /// assert_eq!(v, &ParamOverrideValue::Int(50));
    ///
    /// assert_eq!(k.to_bytes(), b"key", "expected key to be 'key', was {:?}", k);
    /// ```
    #[allow(clippy::missing_panics_doc)] // panics are just to enforce internal invariants, not user errors
    pub fn append_kv_override(
        mut self: Pin<&mut Self>,
        key: &CStr,
        value: kv_overrides::ParamOverrideValue,
    ) {
        let kv_override = self
            .kv_overrides
            .get_mut(0)
            .expect("kv_overrides did not have a next allocated");

        assert_eq!(kv_override.key[0], 0, "last kv_override was not empty");

        // There should be some way to do this without iterating over everything.
        for (i, &c) in key.to_bytes_with_nul().iter().enumerate() {
            kv_override.key[i] = c_char::try_from(c).expect("invalid character in key");
        }

        kv_override.tag = value.tag();
        kv_override.__bindgen_anon_1 = value.value();

        // set to null pointer for panic safety (as push may move the vector, invalidating the pointer)
        self.params.kv_overrides = null();

        // push the next one to ensure we maintain the iterator invariant of ending with a 0
        self.kv_overrides
            .push(koharu_llama_sys::llama_model_kv_override {
                key: [0; 128],
                tag: 0,
                __bindgen_anon_1: koharu_llama_sys::llama_model_kv_override__bindgen_ty_1 {
                    val_i64: 0,
                },
            });

        // set the pointer to the (potentially) new vector
        self.params.kv_overrides = self.kv_overrides.as_ptr();

        eprintln!("saved ptr: {:?}", self.params.kv_overrides);
    }
}

impl LlamaModelParams {
    /// Adds buffer type overides to move all mixture-of-experts layers to CPU.
    pub fn add_cpu_moe_override(self: Pin<&mut Self>) {
        self.add_cpu_buft_override(c"\\.ffn_(up|down|gate)_(ch|)exps");
    }

    /// Appends a buffer type override to the model parameters, to move layers matching pattern to CPU.
    /// It must be pinned as this creates a self-referential struct.
    pub fn add_cpu_buft_override(mut self: Pin<&mut Self>, key: &CStr) {
        let buft_override = self
            .buft_overrides
            .get_mut(0)
            .expect("buft_overrides did not have a next allocated");

        assert!(
            buft_override.pattern.is_null(),
            "last buft_override was not empty"
        );

        // There should be some way to do this without iterating over everything.
        for &c in key.to_bytes_with_nul().iter() {
            c_char::try_from(c).expect("invalid character in key");
        }

        buft_override.pattern = key.as_ptr();
        buft_override.buft = unsafe { koharu_llama_sys::ggml_backend_cpu_buffer_type() };

        // set to null pointer for panic safety (as push may move the vector, invalidating the pointer)
        self.params.tensor_buft_overrides = null();

        // push the next one to ensure we maintain the iterator invariant of ending with a 0
        self.buft_overrides
            .push(koharu_llama_sys::llama_model_tensor_buft_override {
                pattern: std::ptr::null(),
                buft: std::ptr::null_mut(),
            });

        // set the pointer to the (potentially) new vector
        self.params.tensor_buft_overrides = self.buft_overrides.as_ptr();
    }

    /// Returns the tensor-name patterns of the buffer-type overrides currently set on these
    /// parameters, in order.
    ///
    /// This is the read-only counterpart to [`add_cpu_buft_override`](Self::add_cpu_buft_override)
    /// and [`add_cpu_moe_override`](Self::add_cpu_moe_override). After
    /// [`fit_params`](Self::fit_params) it reflects the overrides the auto-fit chose â€” for example
    /// the routed-expert tensors (`blk.<N>.ffn_(up|down|gate_up|gate)_(ch|)exps`) a
    /// mixture-of-experts fit assigns to the CPU buffer type to make the model fit. Returns an empty
    /// vector when no overrides are set. The trailing null-terminator entry the override list
    /// carries is skipped; only entries with a non-null pattern are returned.
    #[must_use]
    pub fn tensor_buft_override_patterns(&self) -> Vec<String> {
        self.buft_overrides
            .iter()
            .filter(|o| !o.pattern.is_null())
            .map(|o| {
                // SAFETY: a non-null `pattern` is a NUL-terminated C string. For fit-produced
                // overrides it points into process-lifetime function-local `static` storage in
                // llama.cpp's `common/fit.cpp`, so it is always valid to read here. For overrides set
                // via `add_cpu_buft_override` the pointer is borrowed from the caller's `&CStr` with
                // no lifetime tie recorded on the params, so that setter's callers are responsible
                // for keeping the string alive at least as long as the params; every in-tree caller
                // passes a `'static` literal. In both cases the string outlives this `&self` borrow.
                unsafe { CStr::from_ptr(o.pattern) }
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
}

impl LlamaModelParams {
    /// Get the number of layers to offload to the GPU.
    #[must_use]
    pub fn n_gpu_layers(&self) -> i32 {
        self.params.n_gpu_layers
    }

    /// The GPU that is used for scratch and small tensors
    #[must_use]
    pub fn main_gpu(&self) -> i32 {
        self.params.main_gpu
    }

    /// only load the vocabulary, no weights
    #[must_use]
    pub fn vocab_only(&self) -> bool {
        self.params.vocab_only
    }

    /// use mmap if possible
    #[must_use]
    pub fn use_mmap(&self) -> bool {
        self.params.use_mmap
    }

    /// force system to keep model in RAM
    #[must_use]
    pub fn use_mlock(&self) -> bool {
        self.params.use_mlock
    }

    /// get the split mode
    ///
    /// # Errors
    /// Returns `LlamaSplitModeParseError` if the unknown split mode is encountered.
    pub fn split_mode(&self) -> Result<LlamaSplitMode, LlamaSplitModeParseError> {
        LlamaSplitMode::try_from(self.params.split_mode)
    }

    /// get the devices
    #[must_use]
    pub fn devices(&self) -> Vec<usize> {
        let mut backend_devices = Vec::new();
        for i in 0..unsafe { koharu_llama_sys::ggml_backend_dev_count() } {
            let dev = unsafe { koharu_llama_sys::ggml_backend_dev_get(i) };
            backend_devices.push(dev);
        }
        let mut devices = Vec::new();
        for &dev in self.devices.iter() {
            if dev.is_null() {
                break;
            }
            if let Some((index, _)) = backend_devices
                .iter()
                .enumerate()
                .find(|&(_i, &d)| d == dev)
            {
                devices.push(index);
            }
        }
        devices
    }

    /// sets the number of gpu layers to offload to the GPU.
    /// ```
    /// # use koharu_llama::model::params::LlamaModelParams;
    /// let params = LlamaModelParams::default();
    /// let params = params.with_n_gpu_layers(1);
    /// assert_eq!(params.n_gpu_layers(), 1);
    /// ```
    #[must_use]
    pub fn with_n_gpu_layers(mut self, n_gpu_layers: u32) -> Self {
        // The only way this conversion can fail is if u32 overflows the i32 - in which case we set
        // to MAX
        let n_gpu_layers = i32::try_from(n_gpu_layers).unwrap_or(i32::MAX);
        self.params.n_gpu_layers = n_gpu_layers;
        self
    }

    /// sets the main GPU
    ///
    /// To enable this option, you must set `split_mode` to `LlamaSplitMode::None` to enable single GPU mode.
    #[must_use]
    pub fn with_main_gpu(mut self, main_gpu: i32) -> Self {
        self.params.main_gpu = main_gpu;
        self
    }

    /// sets `vocab_only`
    #[must_use]
    pub fn with_vocab_only(mut self, vocab_only: bool) -> Self {
        self.params.vocab_only = vocab_only;
        self
    }

    /// sets `use_mmap`
    #[must_use]
    pub fn with_use_mmap(mut self, use_mmap: bool) -> Self {
        self.params.use_mmap = use_mmap;
        self
    }

    /// sets `use_mlock`
    #[must_use]
    pub fn with_use_mlock(mut self, use_mlock: bool) -> Self {
        self.params.use_mlock = use_mlock;
        self
    }

    /// sets `split_mode`
    #[must_use]
    pub fn with_split_mode(mut self, split_mode: LlamaSplitMode) -> Self {
        self.params.split_mode = split_mode.into();
        self
    }

    /// sets `devices`
    ///
    /// The devices are specified as indices that correspond to the ggml backend device indices.
    ///
    /// The maximum number of devices is 16.
    ///
    /// You don't need to specify CPU or ACCEL devices.
    ///
    /// # Errors
    /// Returns `LlamaCppError::BackendDeviceNotFound` if any device index is invalid.
    pub fn with_devices(mut self, devices: &[usize]) -> Result<Self, LlamaCppError> {
        for dev in self.devices.iter_mut() {
            *dev = std::ptr::null_mut();
        }
        // Check device count
        let max_devices = crate::max_devices().min(LLAMA_CPP_MAX_DEVICES);
        if devices.len() > max_devices {
            return Err(LlamaCppError::MaxDevicesExceeded(max_devices));
        }
        for (i, &dev) in devices.iter().enumerate() {
            if dev >= unsafe { koharu_llama_sys::ggml_backend_dev_count() } {
                return Err(LlamaCppError::BackendDeviceNotFound(dev));
            }
            let backend_dev = unsafe { koharu_llama_sys::ggml_backend_dev_get(dev) };
            self.devices[i] = backend_dev;
        }
        if self.devices.is_empty() {
            self.params.devices = std::ptr::null_mut();
        } else {
            self.params.devices = self.devices.as_mut_ptr();
        }
        Ok(self)
    }

    /// Set `no_alloc`
    ///
    /// If this parameter is true, don't allocate memory for the tensor data
    ///
    /// You can't use `no_alloc` with `use_mmap`, so this also sets `use_mmap` to false.
    #[must_use]
    pub fn with_no_alloc(mut self, no_alloc: bool) -> Self {
        self.params.no_alloc = no_alloc;
        if no_alloc {
            self = self.with_use_mmap(false);
        }
        self
    }

    /// Get `no_alloc`
    ///
    /// If this parameter is true, don't allocate memory for the tensor data
    #[must_use]
    pub fn no_alloc(&self) -> bool {
        self.params.no_alloc
    }

    /// Sets a callback invoked during loading with progress in `0.0..=1.0`.
    /// Returning `false` aborts the load (it then fails with `NullResult`).
    #[must_use]
    pub fn with_progress_callback<F: FnMut(f32) -> bool + 'static>(mut self, callback: F) -> Self {
        unsafe extern "C" fn trampoline<F: FnMut(f32) -> bool>(
            progress: f32,
            user_data: *mut c_void,
        ) -> bool {
            let callback = unsafe { &mut *user_data.cast::<F>() };
            callback(progress)
        }

        let mut callback = Box::new(callback);
        self.params.progress_callback_user_data =
            std::ptr::from_mut(&mut *callback).cast::<c_void>();
        self.params.progress_callback = Some(trampoline::<F>);
        self.progress_callback = Some(callback);
        self
    }
}

/// Default parameters for `LlamaModel`. (as defined in llama.cpp by `llama_model_default_params`)
/// ```
/// # use koharu_llama::model::params::LlamaModelParams;
/// use koharu_llama::model::params::LlamaSplitMode;
/// let params = LlamaModelParams::default();
/// assert_eq!(params.n_gpu_layers(), -1, "n_gpu_layers should be -1");
/// assert_eq!(params.main_gpu(), 0, "main_gpu should be 0");
/// assert_eq!(params.vocab_only(), false, "vocab_only should be false");
/// assert_eq!(params.use_mmap(), true, "use_mmap should be true");
/// assert_eq!(params.use_mlock(), false, "use_mlock should be false");
/// assert_eq!(params.split_mode(), Ok(LlamaSplitMode::Layer), "split_mode should be LAYER");
/// assert_eq!(params.devices().len(), 0, "devices should be empty");
/// assert_eq!(params.no_alloc(), false, "no_alloc should be false");
/// ```
impl Default for LlamaModelParams {
    fn default() -> Self {
        let default_params = unsafe { koharu_llama_sys::llama_model_default_params() };
        LlamaModelParams {
            params: default_params,
            // push the next one to ensure we maintain the iterator invariant of ending with a 0
            kv_overrides: vec![koharu_llama_sys::llama_model_kv_override {
                key: [0; 128],
                tag: 0,
                __bindgen_anon_1: koharu_llama_sys::llama_model_kv_override__bindgen_ty_1 {
                    val_i64: 0,
                },
            }],
            buft_overrides: vec![koharu_llama_sys::llama_model_tensor_buft_override {
                pattern: std::ptr::null(),
                buft: std::ptr::null_mut(),
            }],
            devices: Box::pin([std::ptr::null_mut(); 16]),
            tensor_split: Vec::new(),
            progress_callback: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LlamaModelParams;
    use std::pin::pin;

    #[test]
    fn tensor_buft_override_patterns_empty_by_default() {
        // Fresh params carry only the null-terminator entry, so no patterns are reported.
        assert!(
            LlamaModelParams::default()
                .tensor_buft_override_patterns()
                .is_empty()
        );
    }

    #[test]
    fn tensor_buft_override_patterns_reads_back_added_override() {
        // The getter is the read-only counterpart to the setter: the override added is reported and
        // the trailing null terminator is skipped. This mirrors how `fit_params` populates the same
        // buffer for the auto-fit's MoE expert offload (`add_cpu_moe_override` uses the same expert
        // tensor pattern shape the fit emits).
        let mut params = pin!(LlamaModelParams::default());
        params.as_mut().add_cpu_moe_override();
        assert_eq!(
            params.tensor_buft_override_patterns(),
            vec!["\\.ffn_(up|down|gate)_(ch|)exps".to_owned()],
        );
    }

    #[test]
    fn progress_callback_round_trips_and_can_abort() {
        use super::LlamaModelParams;
        use std::cell::Cell;
        use std::rc::Rc;

        let calls = Rc::new(Cell::new(0_u32));
        let counter = Rc::clone(&calls);
        let params = LlamaModelParams::default().with_progress_callback(move |_progress| {
            counter.set(counter.get() + 1);
            false
        });

        assert!(params.params.progress_callback.is_some());
        assert!(!params.params.progress_callback_user_data.is_null());

        let trampoline = params.params.progress_callback.unwrap();
        let user_data = params.params.progress_callback_user_data;
        let first = unsafe { trampoline(0.5, user_data) };
        let second = unsafe { trampoline(1.0, user_data) };

        assert!(!first && !second, "returning false signals an abort");
        assert_eq!(calls.get(), 2);
    }
}
