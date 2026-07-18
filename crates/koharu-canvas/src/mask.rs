use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use image::{GrayImage, ImageEncoder, codecs::png::PngEncoder};
use koharu_scene::{BlobId, PageId, Size};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::{
    Brush, Error, MaskOverlay, MaskPlane, PagePoint, PhysicalSize, PixelRect, PixelSize, Result,
    StrokeMode,
};

const TILE_SIZE: u32 = 256;

#[derive(Clone)]
struct Tile {
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
    version: u64,
    tinted: Option<TintedTile>,
}

#[derive(Clone)]
struct TintedTile {
    version: u64,
    tint: [u8; 4],
    opacity: u32,
    image: ImageData,
}

#[derive(Clone)]
struct MaskSnapshot {
    size: PixelSize,
    tiles_x: u32,
    tiles: Vec<Arc<Vec<u8>>>,
}

pub struct MaskCommit {
    pub page: PageId,
    pub plane: MaskPlane,
    pub dirty: PixelRect,
    pub generation: u64,
    snapshot: MaskSnapshot,
}

impl std::fmt::Debug for MaskCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaskCommit")
            .field("page", &self.page)
            .field("plane", &self.plane)
            .field("dirty", &self.dirty)
            .field("generation", &self.generation)
            .field("size", &self.snapshot.size)
            .finish_non_exhaustive()
    }
}

impl MaskCommit {
    #[must_use]
    pub const fn size(&self) -> PixelSize {
        self.snapshot.size
    }

    pub fn encode_png(&self) -> Result<Vec<u8>> {
        let pixels = self.snapshot.flatten();
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(
                &pixels,
                self.snapshot.size.width,
                self.snapshot.size.height,
                image::ExtendedColorType::L8,
            )
            .map_err(|error| Error::Invalid(format!("failed to encode mask PNG: {error}")))?;
        Ok(encoded)
    }
}

impl MaskSnapshot {
    fn flatten(&self) -> Vec<u8> {
        let mut output = vec![0; self.size.width as usize * self.size.height as usize];
        for (index, tile) in self.tiles.iter().enumerate() {
            let tile_x = index as u32 % self.tiles_x;
            let tile_y = index as u32 / self.tiles_x;
            let x = tile_x * TILE_SIZE;
            let y = tile_y * TILE_SIZE;
            let width = (self.size.width - x).min(TILE_SIZE);
            let height = (self.size.height - y).min(TILE_SIZE);
            for row in 0..height {
                let source = row as usize * width as usize;
                let target = (y + row) as usize * self.size.width as usize + x as usize;
                output[target..target + width as usize]
                    .copy_from_slice(&tile[source..source + width as usize]);
            }
        }
        output
    }
}

pub(crate) struct MaskState {
    pub source: Option<BlobId>,
    pub generation: u64,
    pub committed_generation: u64,
    buffer: MaskBuffer,
}

impl MaskState {
    pub fn empty(size: Size) -> Self {
        Self {
            source: None,
            generation: 0,
            committed_generation: 0,
            buffer: MaskBuffer::empty(size),
        }
    }

    pub fn replace(&mut self, source: Option<BlobId>, image: Option<&GrayImage>, size: Size) {
        self.source = source;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.committed_generation = self.generation;
        self.buffer = image.map_or_else(|| MaskBuffer::empty(size), MaskBuffer::from_image);
    }

    #[must_use]
    pub const fn has_uncommitted(&self) -> bool {
        self.generation > self.committed_generation
    }

    pub fn acknowledge(&mut self, generation: u64, blob: BlobId) -> Result<()> {
        if generation > self.generation {
            return Err(Error::Invalid(format!(
                "mask generation {generation} is newer than live generation {}",
                self.generation
            )));
        }
        if generation < self.committed_generation {
            return Ok(());
        }
        self.committed_generation = self.committed_generation.max(generation);
        self.source = Some(blob);
        Ok(())
    }

    pub fn tinted_tiles(&mut self, overlay: MaskOverlay) -> Vec<(u32, u32, ImageData)> {
        self.buffer.tinted_tiles(overlay)
    }

    pub fn paint(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut HashMap<usize, Arc<Vec<u8>>>,
    ) -> PixelRect {
        self.buffer.paint_segment(from, to, brush, before)
    }

    pub fn restore(&mut self, before: HashMap<usize, Arc<Vec<u8>>>) {
        self.buffer.restore(before);
    }

    pub fn finish(&mut self, page: PageId, plane: MaskPlane, dirty: PixelRect) -> MaskCommit {
        self.generation = self.generation.wrapping_add(1).max(1);
        MaskCommit {
            page,
            plane,
            dirty,
            generation: self.generation,
            snapshot: self.buffer.snapshot(),
        }
    }
}

pub(crate) struct ActiveStroke {
    pub plane: MaskPlane,
    pub brush: Brush,
    pub last: PagePoint,
    pub before: HashMap<usize, Arc<Vec<u8>>>,
    pub dirty: PixelRect,
}

impl ActiveStroke {
    pub fn new(plane: MaskPlane, brush: Brush, point: PagePoint) -> Self {
        Self {
            plane,
            brush,
            last: point,
            before: HashMap::new(),
            dirty: PixelRect::default(),
        }
    }
}

struct MaskBuffer {
    size: PixelSize,
    tiles_x: u32,
    tiles: Vec<Tile>,
}

impl MaskBuffer {
    fn empty(size: Size) -> Self {
        Self::from_pixels(size, &vec![0; size.width as usize * size.height as usize])
    }

    fn from_image(image: &GrayImage) -> Self {
        Self::from_pixels(Size::new(image.width(), image.height()), image.as_raw())
    }

    fn from_pixels(size: Size, pixels: &[u8]) -> Self {
        let tiles_x = size.width.div_ceil(TILE_SIZE);
        let tiles_y = size.height.div_ceil(TILE_SIZE);
        let mut tiles = Vec::with_capacity((tiles_x * tiles_y) as usize);
        for tile_y in 0..tiles_y {
            for tile_x in 0..tiles_x {
                let x = tile_x * TILE_SIZE;
                let y = tile_y * TILE_SIZE;
                let width = (size.width - x).min(TILE_SIZE);
                let height = (size.height - y).min(TILE_SIZE);
                let mut tile = Vec::with_capacity(width as usize * height as usize);
                for row in 0..height {
                    let start = (y + row) as usize * size.width as usize + x as usize;
                    tile.extend_from_slice(&pixels[start..start + width as usize]);
                }
                tiles.push(Tile {
                    width,
                    height,
                    pixels: Arc::new(tile),
                    version: 0,
                    tinted: None,
                });
            }
        }
        Self {
            size: PhysicalSize::new(size.width, size.height),
            tiles_x,
            tiles,
        }
    }

    fn snapshot(&self) -> MaskSnapshot {
        MaskSnapshot {
            size: self.size,
            tiles_x: self.tiles_x,
            tiles: self
                .tiles
                .iter()
                .map(|tile| Arc::clone(&tile.pixels))
                .collect(),
        }
    }

    fn restore(&mut self, before: HashMap<usize, Arc<Vec<u8>>>) {
        for (index, pixels) in before {
            if let Some(tile) = self.tiles.get_mut(index) {
                tile.pixels = pixels;
                tile.version = tile.version.wrapping_add(1);
                tile.tinted = None;
            }
        }
    }

    fn paint_segment(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut HashMap<usize, Arc<Vec<u8>>>,
    ) -> PixelRect {
        let radius = f64::from(brush.diameter) * 0.5;
        let min_x = ((from.x.min(to.x) - radius).floor().max(0.0) as u32).min(self.size.width);
        let min_y = ((from.y.min(to.y) - radius).floor().max(0.0) as u32).min(self.size.height);
        let max_x = ((from.x.max(to.x) + radius).ceil().max(0.0) as u32).min(self.size.width);
        let max_y = ((from.y.max(to.y) + radius).ceil().max(0.0) as u32).min(self.size.height);
        if min_x >= max_x || min_y >= max_y {
            return PixelRect::default();
        }

        let value = match brush.mode {
            StrokeMode::Paint => u8::MAX,
            StrokeMode::Erase => 0,
        };
        let mut touched = HashSet::new();
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let length = dx.hypot(dy);
        let spacing = (radius * 0.5).max(0.25);
        let steps = (length / spacing).ceil().max(1.0) as u32;
        for step in 0..=steps {
            let progress = f64::from(step) / f64::from(steps);
            let center = PagePoint::new(from.x + dx * progress, from.y + dy * progress);
            let circle_min_x = ((center.x - radius).floor().max(0.0) as u32).min(self.size.width);
            let circle_min_y = ((center.y - radius).floor().max(0.0) as u32).min(self.size.height);
            let circle_max_x = ((center.x + radius).ceil().max(0.0) as u32).min(self.size.width);
            let circle_max_y = ((center.y + radius).ceil().max(0.0) as u32).min(self.size.height);
            for y in circle_min_y..circle_max_y {
                for x in circle_min_x..circle_max_x {
                    let pixel_x = f64::from(x) + 0.5;
                    let pixel_y = f64::from(y) + 0.5;
                    if (pixel_x - center.x).powi(2) + (pixel_y - center.y).powi(2) > radius * radius
                    {
                        continue;
                    }
                    let tile_x = x / TILE_SIZE;
                    let tile_y = y / TILE_SIZE;
                    let index = (tile_y * self.tiles_x + tile_x) as usize;
                    let tile = &mut self.tiles[index];
                    let local_x = x - tile_x * TILE_SIZE;
                    let local_y = y - tile_y * TILE_SIZE;
                    let offset = local_y as usize * tile.width as usize + local_x as usize;
                    if tile.pixels[offset] == value {
                        continue;
                    }
                    before
                        .entry(index)
                        .or_insert_with(|| Arc::clone(&tile.pixels));
                    Arc::make_mut(&mut tile.pixels)[offset] = value;
                    touched.insert(index);
                }
            }
        }
        for index in touched {
            let tile = &mut self.tiles[index];
            tile.version = tile.version.wrapping_add(1);
            tile.tinted = None;
        }
        PixelRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    fn tinted_tiles(&mut self, overlay: MaskOverlay) -> Vec<(u32, u32, ImageData)> {
        let opacity = overlay.opacity.clamp(0.0, 1.0);
        let mut output = Vec::with_capacity(self.tiles.len());
        for (index, tile) in self.tiles.iter_mut().enumerate() {
            if !tile.pixels.iter().any(|coverage| *coverage != 0) {
                continue;
            }
            let cache_matches = tile.tinted.as_ref().is_some_and(|cached| {
                cached.version == tile.version
                    && cached.tint == overlay.tint
                    && cached.opacity == opacity.to_bits()
            });
            if !cache_matches {
                let tint_alpha = f32::from(overlay.tint[3]) / 255.0 * opacity;
                let mut rgba = Vec::with_capacity(tile.pixels.len() * 4);
                for &coverage in tile.pixels.iter() {
                    rgba.extend_from_slice(&[
                        overlay.tint[0],
                        overlay.tint[1],
                        overlay.tint[2],
                        (f32::from(coverage) * tint_alpha).round() as u8,
                    ]);
                }
                let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(rgba);
                tile.tinted = Some(TintedTile {
                    version: tile.version,
                    tint: overlay.tint,
                    opacity: opacity.to_bits(),
                    image: ImageData {
                        data: Blob::new(data),
                        format: ImageFormat::Rgba8,
                        alpha_type: ImageAlphaType::Alpha,
                        width: tile.width,
                        height: tile.height,
                    },
                });
            }
            let tile_x = index as u32 % self.tiles_x;
            let tile_y = index as u32 / self.tiles_x;
            output.push((
                tile_x * TILE_SIZE,
                tile_y * TILE_SIZE,
                tile.tinted
                    .as_ref()
                    .expect("tinted tile was created")
                    .image
                    .clone(),
            ));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_shares_unchanged_tiles_and_encodes_luma() {
        fn assert_send<T: Send>() {}
        assert_send::<MaskCommit>();

        let page = PageId::new();
        let mut state = MaskState::empty(Size::new(300, 300));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(10.0, 10.0),
            PagePoint::new(30.0, 10.0),
            Brush {
                diameter: 8.0,
                mode: StrokeMode::Paint,
            },
            &mut before,
        );
        let commit = state.finish(page, MaskPlane::Text, dirty);
        let encoded = commit.encode_png().unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        assert_eq!(decoded.color().channel_count(), 1);
        assert_eq!(decoded.to_luma8().get_pixel(20, 10).0[0], 255);
    }

    #[test]
    fn restore_cancels_changed_tiles() {
        let mut state = MaskState::empty(Size::new(32, 32));
        let mut before = HashMap::new();
        state.paint(
            PagePoint::new(8.0, 8.0),
            PagePoint::new(8.0, 8.0),
            Brush {
                diameter: 4.0,
                mode: StrokeMode::Paint,
            },
            &mut before,
        );
        state.restore(before);
        assert!(
            state
                .buffer
                .snapshot()
                .flatten()
                .iter()
                .all(|pixel| *pixel == 0)
        );
    }

    #[test]
    fn stale_commit_acknowledgement_does_not_replace_a_newer_blob() {
        let mut state = MaskState::empty(Size::new(32, 32));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(8.0, 8.0),
            PagePoint::new(8.0, 8.0),
            Brush {
                diameter: 4.0,
                mode: StrokeMode::Paint,
            },
            &mut before,
        );
        let page = PageId::new();
        let first = state.finish(page, MaskPlane::Text, dirty).generation;
        let mut second_before = HashMap::new();
        let second_dirty = state.paint(
            PagePoint::new(16.0, 16.0),
            PagePoint::new(16.0, 16.0),
            Brush {
                diameter: 4.0,
                mode: StrokeMode::Paint,
            },
            &mut second_before,
        );
        let second = state.finish(page, MaskPlane::Text, second_dirty).generation;
        let newer_blob = BlobId::for_bytes(b"newer");
        let older_blob = BlobId::for_bytes(b"older");
        state.acknowledge(second, newer_blob).unwrap();
        state.acknowledge(first, older_blob).unwrap();
        assert_eq!(state.source, Some(newer_blob));
    }

    #[test]
    fn long_diagonal_stroke_has_no_sample_gaps() {
        let page = PageId::new();
        let mut state = MaskState::empty(Size::new(512, 512));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(4.0, 4.0),
            PagePoint::new(508.0, 508.0),
            Brush {
                diameter: 6.0,
                mode: StrokeMode::Paint,
            },
            &mut before,
        );
        let encoded = state
            .finish(page, MaskPlane::Brush, dirty)
            .encode_png()
            .unwrap();
        let pixels = image::load_from_memory(&encoded).unwrap().into_luma8();
        for coordinate in [32, 128, 256, 384, 480] {
            assert_eq!(pixels.get_pixel(coordinate, coordinate).0[0], 255);
        }
    }

    #[test]
    fn erase_and_page_edge_clipping_are_applied() {
        let page = PageId::new();
        let mut state = MaskState::empty(Size::new(32, 32));
        let mut paint_before = HashMap::new();
        state.paint(
            PagePoint::new(0.0, 0.0),
            PagePoint::new(16.0, 16.0),
            Brush {
                diameter: 8.0,
                mode: StrokeMode::Paint,
            },
            &mut paint_before,
        );
        let mut erase_before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(16.0, 16.0),
            PagePoint::new(16.0, 16.0),
            Brush {
                diameter: 4.0,
                mode: StrokeMode::Erase,
            },
            &mut erase_before,
        );
        assert!(dirty.x.saturating_add(dirty.width) <= 32);
        assert!(dirty.y.saturating_add(dirty.height) <= 32);
        let pixels = image::load_from_memory(
            &state
                .finish(page, MaskPlane::Brush, dirty)
                .encode_png()
                .unwrap(),
        )
        .unwrap()
        .into_luma8();
        assert_eq!(pixels.get_pixel(0, 0).0[0], 255);
        assert_eq!(pixels.get_pixel(16, 16).0[0], 0);
    }

    #[test]
    fn empty_mask_does_not_create_rgba_preview_tiles() {
        let mut state = MaskState::empty(Size::new(1024, 1024));
        let overlay = MaskOverlay::new([255, 0, 0, 255], 0.5);
        assert!(state.tinted_tiles(overlay).is_empty());

        let mut before = HashMap::new();
        state.paint(
            PagePoint::new(8.0, 8.0),
            PagePoint::new(8.0, 8.0),
            Brush {
                diameter: 4.0,
                mode: StrokeMode::Paint,
            },
            &mut before,
        );
        assert_eq!(state.tinted_tiles(overlay).len(), 1);
    }
}
