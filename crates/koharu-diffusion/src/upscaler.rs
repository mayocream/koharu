use std::{cell::Cell, fmt, marker::PhantomData, path::PathBuf, ptr::NonNull};

use crate::{
    Error, Result, RgbImage,
    context::RawImages,
    ffi::{NativeCall, optional_cstring, path_cstring},
    image::raw_rgb_image,
    sys,
};

/// Model and backend settings for an ESRGAN upscaler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpscalerParams {
    pub model_path: PathBuf,
    pub direct_convolution: bool,
    pub n_threads: i32,
    pub tile_size: i32,
    pub backend: Option<String>,
    pub params_backend: Option<String>,
}

impl UpscalerParams {
    #[must_use]
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            direct_convolution: false,
            n_threads: crate::physical_core_count(),
            tile_size: 128,
            backend: None,
            params_backend: None,
        }
    }
}

/// An owning ESRGAN upscaler context.
pub struct Upscaler {
    pointer: NonNull<sys::upscaler_ctx_t>,
    _not_sync: PhantomData<Cell<()>>,
}

unsafe impl Send for Upscaler {}

impl fmt::Debug for Upscaler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Upscaler")
            .field("pointer", &self.pointer)
            .finish_non_exhaustive()
    }
}

impl Upscaler {
    pub fn new(params: &UpscalerParams) -> Result<Self> {
        if params.n_threads <= 0 {
            return Err(Error::InvalidParameter {
                name: "n_threads",
                reason: "must be greater than zero",
            });
        }
        let model_path = path_cstring(&params.model_path, "upscaler model_path")?;
        let backend = optional_cstring(params.backend.as_deref(), "backend")?;
        let params_backend = optional_cstring(params.params_backend.as_deref(), "params_backend")?;
        let _call = NativeCall::enter();
        let pointer = unsafe {
            sys::new_upscaler_ctx(
                model_path.as_ptr(),
                params.direct_convolution,
                params.n_threads,
                params.tile_size,
                backend
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                params_backend
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
            )
        };
        let pointer = NonNull::new(pointer).ok_or(Error::UpscalerCreationFailed)?;
        Ok(Self {
            pointer,
            _not_sync: PhantomData,
        })
    }

    /// Returns the scale factor encoded by the loaded model.
    #[must_use]
    pub fn factor(&self) -> i32 {
        let _call = NativeCall::enter();
        unsafe { sys::get_upscale_factor(self.pointer.as_ptr()) }
    }

    /// Upscales an image and returns the native output as owned Rust images.
    pub fn upscale(&mut self, image: &RgbImage, requested_factor: u32) -> Result<Vec<RgbImage>> {
        if requested_factor == 0 {
            return Err(Error::InvalidParameter {
                name: "requested_factor",
                reason: "must be greater than zero",
            });
        }
        let image = raw_rgb_image(image)?;
        let _call = NativeCall::enter();
        let mut output = RawImages::default();
        let succeeded = unsafe {
            sys::upscale(
                self.pointer.as_ptr(),
                image,
                requested_factor,
                &raw mut output.pointer,
                &raw mut output.count,
            )
        };
        if !succeeded {
            return Err(Error::UpscaleFailed);
        }
        output.copy("upscaled image")
    }
}

impl Drop for Upscaler {
    fn drop(&mut self) {
        let _call = NativeCall::enter();
        unsafe { sys::free_upscaler_ctx(self.pointer.as_ptr()) };
    }
}
