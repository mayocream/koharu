use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const BYTES_PER_PIXEL: u32 = 4;
const MAX_FRAME_DIMENSION: u32 = 32_768;
const MAX_FRAME_BYTES: u64 = 1 << 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DirtyRect {
    #[must_use]
    pub const fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    fn validate(self, width: u32, height: u32) -> Result<(), FrameError> {
        let right = self
            .x
            .checked_add(self.width)
            .ok_or(FrameError::DirtyRectOverflow)?;
        let bottom = self
            .y
            .checked_add(self.height)
            .ok_or(FrameError::DirtyRectOverflow)?;
        if self.is_empty() || right > width || bottom > height {
            return Err(FrameError::DirtyRectOutOfBounds);
        }
        Ok(())
    }
}

/// Complete CEF software paint buffer in premultiplied BGRA byte order.
#[derive(Clone, Debug)]
pub struct SoftwareFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub dirty: Vec<DirtyRect>,
    pub pixels: Arc<[u8]>,
}

/// Frame copied from CEF's platform shared texture while the accelerated-paint
/// callback still owns that external resource.
#[derive(Debug)]
pub(crate) struct AcceleratedFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
}

#[derive(Debug)]
pub(crate) enum BrowserFrame {
    Software(SoftwareFrame),
    Accelerated(AcceleratedFrame),
}

impl BrowserFrame {
    pub(crate) const fn width(&self) -> u32 {
        match self {
            Self::Software(frame) => frame.width,
            Self::Accelerated(frame) => frame.width,
        }
    }

    pub(crate) const fn height(&self) -> u32 {
        match self {
            Self::Software(frame) => frame.height,
            Self::Accelerated(frame) => frame.height,
        }
    }

    fn force_full_damage(&mut self) {
        if let Self::Software(frame) = self {
            frame.force_full_damage();
        }
    }
}

impl From<SoftwareFrame> for BrowserFrame {
    fn from(frame: SoftwareFrame) -> Self {
        Self::Software(frame)
    }
}

impl From<AcceleratedFrame> for BrowserFrame {
    fn from(frame: AcceleratedFrame) -> Self {
        Self::Accelerated(frame)
    }
}

impl SoftwareFrame {
    pub fn new(
        sequence: u64,
        width: u32,
        height: u32,
        stride: u32,
        dirty: Vec<DirtyRect>,
        pixels: impl Into<Arc<[u8]>>,
    ) -> Result<Self, FrameError> {
        if sequence == 0 {
            return Err(FrameError::ZeroSequence);
        }
        if width == 0 || height == 0 || width > MAX_FRAME_DIMENSION || height > MAX_FRAME_DIMENSION
        {
            return Err(FrameError::InvalidSize);
        }
        let minimum_stride = width
            .checked_mul(BYTES_PER_PIXEL)
            .ok_or(FrameError::SizeOverflow)?;
        if stride < minimum_stride || !stride.is_multiple_of(BYTES_PER_PIXEL) {
            return Err(FrameError::InvalidStride);
        }
        let byte_len = u64::from(stride)
            .checked_mul(u64::from(height))
            .ok_or(FrameError::SizeOverflow)?;
        if byte_len > MAX_FRAME_BYTES {
            return Err(FrameError::SizeOverflow);
        }
        let pixels = pixels.into();
        if pixels.len() as u64 != byte_len {
            return Err(FrameError::PixelLength {
                expected: byte_len,
                actual: pixels.len() as u64,
            });
        }
        let dirty = if dirty.is_empty() {
            vec![DirtyRect::full(width, height)]
        } else {
            dirty
        };
        for rect in &dirty {
            rect.validate(width, height)?;
        }
        Ok(Self {
            sequence,
            width,
            height,
            stride,
            dirty,
            pixels,
        })
    }

    pub(crate) fn rect_bytes(&self, rect: DirtyRect) -> &[u8] {
        let start = (u64::from(rect.y) * u64::from(self.stride)
            + u64::from(rect.x) * u64::from(BYTES_PER_PIXEL)) as usize;
        let len = if rect.height == 1 {
            u64::from(rect.width) * u64::from(BYTES_PER_PIXEL)
        } else {
            u64::from(rect.height - 1) * u64::from(self.stride)
                + u64::from(rect.width) * u64::from(BYTES_PER_PIXEL)
        } as usize;
        &self.pixels[start..start + len]
    }

    pub(crate) fn force_full_damage(&mut self) {
        self.dirty.clear();
        self.dirty.push(DirtyRect::full(self.width, self.height));
    }
}

/// A single-slot handoff from CEF paint callbacks to the winit thread.
///
/// CEF supplies a complete image on both paint paths. Replacing an intermediate
/// software frame is therefore safe when the survivor is marked fully dirty;
/// accelerated frames already own complete copied textures. Keeping the wake
/// flag under the same lock as the slot prevents a missed drain transition.
#[derive(Clone)]
pub(crate) struct BrowserFrameMailbox {
    inner: Arc<BrowserFrameMailboxInner>,
}

struct BrowserFrameMailboxInner {
    state: Mutex<BrowserFrameMailboxState>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct BrowserFrameMailboxState {
    frame: Option<BrowserFrame>,
    wake_pending: bool,
}

impl BrowserFrameMailbox {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            inner: Arc::new(BrowserFrameMailboxInner {
                state: Mutex::new(BrowserFrameMailboxState::default()),
                wake,
            }),
        }
    }

    pub(crate) fn submit(&self, mut frame: BrowserFrame) {
        let should_wake = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.frame.is_some() {
                frame.force_full_damage();
            }
            state.frame = Some(frame);
            if state.wake_pending {
                false
            } else {
                state.wake_pending = true;
                true
            }
        };
        if should_wake {
            (self.inner.wake)();
        }
    }

    pub(crate) fn take(&self) -> Option<BrowserFrame> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = state.frame.take();
        state.wake_pending = false;
        frame
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FrameError {
    #[error("frame sequence zero is reserved")]
    ZeroSequence,
    #[error("software frame size is zero or exceeds the supported maximum")]
    InvalidSize,
    #[error("software frame byte size overflowed the supported range")]
    SizeOverflow,
    #[error("software frame stride is not a four-byte-aligned BGRA row")]
    InvalidStride,
    #[error("software frame has {actual} bytes, expected {expected}")]
    PixelLength { expected: u64, actual: u64 },
    #[error("software frame dirty rectangle overflowed")]
    DirtyRectOverflow,
    #[error("software frame dirty rectangle is empty or outside the frame")]
    DirtyRectOutOfBounds,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn frame(sequence: u64, dirty: DirtyRect) -> SoftwareFrame {
        SoftwareFrame::new(sequence, 2, 2, 8, vec![dirty], vec![0_u8; 16]).unwrap()
    }

    #[test]
    fn invalid_dirty_rect_never_reaches_wgpu() {
        assert_eq!(
            SoftwareFrame::new(
                1,
                2,
                2,
                8,
                vec![DirtyRect {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 1,
                }],
                vec![0_u8; 16],
            )
            .unwrap_err(),
            FrameError::DirtyRectOutOfBounds
        );
    }

    #[test]
    fn rect_slice_preserves_source_stride() {
        let frame = SoftwareFrame::new(
            1,
            3,
            2,
            16,
            vec![DirtyRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }],
            (0_u8..32).collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(frame.rect_bytes(frame.dirty[0]), &frame.pixels[4..24]);
    }

    #[test]
    fn mailbox_coalesces_frames_into_one_wake_and_full_damage() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let mailbox = BrowserFrameMailbox::new(Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }));
        let partial = DirtyRect {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };

        mailbox.submit(frame(1, partial).into());
        mailbox.submit(frame(2, partial).into());

        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        let delivered = mailbox.take().unwrap();
        let BrowserFrame::Software(delivered) = delivered else {
            panic!("expected a software frame");
        };
        assert_eq!(delivered.sequence, 2);
        assert_eq!(delivered.dirty, [DirtyRect::full(2, 2)]);
    }

    #[test]
    fn mailbox_rearms_after_the_consumer_drains_it() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let mailbox = BrowserFrameMailbox::new(Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }));
        let full = DirtyRect::full(2, 2);

        mailbox.submit(frame(1, full).into());
        let BrowserFrame::Software(first) = mailbox.take().unwrap() else {
            panic!("expected a software frame");
        };
        assert_eq!(first.sequence, 1);
        mailbox.submit(frame(2, full).into());

        assert_eq!(wakes.load(Ordering::Relaxed), 2);
        let BrowserFrame::Software(second) = mailbox.take().unwrap() else {
            panic!("expected a software frame");
        };
        assert_eq!(second.sequence, 2);
    }
}
