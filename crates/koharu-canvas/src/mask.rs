use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use image::{ImageEncoder, codecs::png::PngEncoder};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::{
    Brush, Error, MaskOverlay, MaskTarget, PageId, PagePoint, PhysicalSize, PixelRect, PixelSize,
    Result, StrokeMode,
};

const TILE_SIZE: u32 = 256;

#[derive(Clone)]
struct Tile {
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
    nonzero: usize,
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
    tiles: Vec<(usize, Arc<Vec<u8>>)>,
}

/// Sparse mask pixels returned to the application for one persistent update.
pub struct MaskCommit {
    pub page: PageId,
    pub target: MaskTarget,
    pub dirty: PixelRect,
    pub generation: u64,
    snapshot: MaskSnapshot,
}

impl std::fmt::Debug for MaskCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaskCommit")
            .field("page", &self.page)
            .field("target", &self.target)
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
        for (index, tile) in &self.tiles {
            let tile_x = *index as u32 % self.tiles_x;
            let tile_y = *index as u32 / self.tiles_x;
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
    pub generation: u64,
    buffer: MaskBuffer,
}

impl MaskState {
    pub fn empty(size: PhysicalSize) -> Self {
        Self {
            generation: 0,
            buffer: MaskBuffer::empty(size),
        }
    }

    fn paint(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut HashMap<usize, Option<Tile>>,
    ) -> PixelRect {
        self.buffer.paint_segment(from, to, brush, before)
    }

    fn restore(&mut self, before: HashMap<usize, Option<Tile>>) {
        self.buffer.restore(before);
    }

    pub fn finish(
        &mut self,
        page: PageId,
        target: MaskTarget,
        dirty: PixelRect,
    ) -> Option<MaskCommit> {
        if dirty.is_empty() {
            return None;
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        Some(MaskCommit {
            page,
            target,
            dirty,
            generation: self.generation,
            snapshot: self.buffer.snapshot(),
        })
    }

    pub fn for_each_tinted_tile(
        &mut self,
        overlay: MaskOverlay,
        visit: impl FnMut(u32, u32, &ImageData),
    ) {
        self.buffer.for_each_tinted_tile(overlay, visit);
    }
}

pub(crate) struct ActiveStroke {
    pub target: MaskTarget,
    pub brush: Brush,
    pub last: PagePoint,
    before: HashMap<usize, Option<Tile>>,
    pub dirty: PixelRect,
}

impl ActiveStroke {
    pub fn new(target: MaskTarget, brush: Brush, point: PagePoint) -> Self {
        Self {
            target,
            brush,
            last: point,
            before: HashMap::new(),
            dirty: PixelRect::default(),
        }
    }

    pub fn paint(&mut self, mask: &mut MaskState, from: PagePoint, to: PagePoint) -> PixelRect {
        mask.paint(from, to, self.brush, &mut self.before)
    }

    pub fn restore(self, mask: &mut MaskState) {
        mask.restore(self.before);
    }
}

struct MaskBuffer {
    size: PixelSize,
    tiles_x: u32,
    tiles: HashMap<usize, Tile>,
}

impl MaskBuffer {
    fn empty(size: PhysicalSize) -> Self {
        Self {
            size,
            tiles_x: size.width.div_ceil(TILE_SIZE),
            tiles: HashMap::new(),
        }
    }

    fn snapshot(&self) -> MaskSnapshot {
        MaskSnapshot {
            size: self.size,
            tiles_x: self.tiles_x,
            tiles: self
                .tiles
                .iter()
                .map(|(index, tile)| (*index, Arc::clone(&tile.pixels)))
                .collect(),
        }
    }

    fn restore(&mut self, before: HashMap<usize, Option<Tile>>) {
        for (index, tile) in before {
            match tile {
                Some(mut tile) => {
                    tile.version = tile.version.wrapping_add(1);
                    tile.tinted = None;
                    self.tiles.insert(index, tile);
                }
                None => {
                    self.tiles.remove(&index);
                }
            }
        }
    }

    fn paint_segment(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut HashMap<usize, Option<Tile>>,
    ) -> PixelRect {
        let radius = f64::from(brush.diameter) * 0.5;
        let value = match brush.mode {
            StrokeMode::Paint => u8::MAX,
            StrokeMode::Erase => 0,
        };
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let length = dx.hypot(dy);
        let spacing = (radius * 0.5).max(0.25);
        let steps = (length / spacing).ceil().max(1.0) as u32;
        let mut touched = HashSet::new();
        let mut changed_min = (u32::MAX, u32::MAX);
        let mut changed_max = (0, 0);

        for step in 0..=steps {
            let progress = f64::from(step) / f64::from(steps);
            let center = PagePoint::new(from.x + dx * progress, from.y + dy * progress);
            let min_x = ((center.x - radius).floor().max(0.0) as u32).min(self.size.width);
            let min_y = ((center.y - radius).floor().max(0.0) as u32).min(self.size.height);
            let max_x = ((center.x + radius).ceil().max(0.0) as u32).min(self.size.width);
            let max_y = ((center.y + radius).ceil().max(0.0) as u32).min(self.size.height);
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let pixel_x = f64::from(x) + 0.5;
                    let pixel_y = f64::from(y) + 0.5;
                    if (pixel_x - center.x).powi(2) + (pixel_y - center.y).powi(2) > radius * radius
                    {
                        continue;
                    }
                    let tile_x = x / TILE_SIZE;
                    let tile_y = y / TILE_SIZE;
                    let index = (tile_y * self.tiles_x + tile_x) as usize;
                    if !self.tiles.contains_key(&index) {
                        if value == 0 {
                            continue;
                        }
                        before.entry(index).or_insert(None);
                        let width = (self.size.width - tile_x * TILE_SIZE).min(TILE_SIZE);
                        let height = (self.size.height - tile_y * TILE_SIZE).min(TILE_SIZE);
                        self.tiles.insert(index, Tile::empty(width, height));
                    } else if !before.contains_key(&index) {
                        before.insert(index, self.tiles.get(&index).cloned());
                    }

                    let tile = self.tiles.get_mut(&index).expect("tile exists above");
                    let local_x = x - tile_x * TILE_SIZE;
                    let local_y = y - tile_y * TILE_SIZE;
                    let offset = local_y as usize * tile.width as usize + local_x as usize;
                    if tile.pixels[offset] == value {
                        continue;
                    }
                    let pixels = Arc::make_mut(&mut tile.pixels);
                    if value == 0 {
                        tile.nonzero -= 1;
                    } else {
                        tile.nonzero += 1;
                    }
                    pixels[offset] = value;
                    touched.insert(index);
                    changed_min.0 = changed_min.0.min(x);
                    changed_min.1 = changed_min.1.min(y);
                    changed_max.0 = changed_max.0.max(x);
                    changed_max.1 = changed_max.1.max(y);
                }
            }
        }

        for index in touched {
            if self.tiles[&index].nonzero == 0 {
                self.tiles.remove(&index);
            } else {
                let tile = self.tiles.get_mut(&index).expect("occupied tile exists");
                tile.version = tile.version.wrapping_add(1);
                tile.tinted = None;
            }
        }
        if changed_min.0 == u32::MAX {
            PixelRect::default()
        } else {
            PixelRect::new(
                changed_min.0,
                changed_min.1,
                changed_max.0 - changed_min.0 + 1,
                changed_max.1 - changed_min.1 + 1,
            )
        }
    }

    fn for_each_tinted_tile(
        &mut self,
        overlay: MaskOverlay,
        mut visit: impl FnMut(u32, u32, &ImageData),
    ) {
        let opacity = overlay.opacity.clamp(0.0, 1.0);
        for (index, tile) in &mut self.tiles {
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
            let tile_x = *index as u32 % self.tiles_x;
            let tile_y = *index as u32 / self.tiles_x;
            visit(
                tile_x * TILE_SIZE,
                tile_y * TILE_SIZE,
                &tile.tinted.as_ref().expect("tinted tile exists").image,
            );
        }
    }
}

impl Tile {
    fn empty(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: Arc::new(vec![0; width as usize * height as usize]),
            nonzero: 0,
            version: 0,
            tinted: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementId;

    fn ids() -> (PageId, ElementId) {
        let session = koharu_scene::Session::memory().unwrap();
        let mut ids = None;
        session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(
                    koharu_scene::PageDraft::new("mask test", 32.0, 32.0),
                    koharu_scene::At::End,
                )?;
                ids = Some((page, edit.add_entity(page, koharu_scene::At::End)?));
                Ok(())
            })
            .unwrap();
        ids.unwrap()
    }

    fn brush(mode: StrokeMode, diameter: f32) -> Brush {
        Brush {
            diameter,
            color: [0; 4],
            mode,
        }
    }

    #[test]
    fn empty_masks_allocate_no_tiles_and_noop_erase_stays_empty() {
        let mut state = MaskState::empty(PhysicalSize::new(4096, 4096));
        assert!(state.buffer.tiles.is_empty());
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(20.0, 20.0),
            PagePoint::new(30.0, 30.0),
            brush(StrokeMode::Erase, 8.0),
            &mut before,
        );
        assert!(dirty.is_empty());
        assert!(before.is_empty());
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn changed_bounds_are_exact_and_empty_tiles_are_released() {
        let mut state = MaskState::empty(PhysicalSize::new(64, 64));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(10.0, 10.0),
            PagePoint::new(10.0, 10.0),
            brush(StrokeMode::Paint, 2.0),
            &mut before,
        );
        assert_eq!(dirty, PixelRect::new(9, 9, 2, 2));
        assert_eq!(state.buffer.tiles.len(), 1);

        let mut erase_before = HashMap::new();
        let erased = state.paint(
            PagePoint::new(10.0, 10.0),
            PagePoint::new(10.0, 10.0),
            brush(StrokeMode::Erase, 2.0),
            &mut erase_before,
        );
        assert_eq!(erased, dirty);
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn cancel_restores_sparse_allocation() {
        let mut state = MaskState::empty(PhysicalSize::new(300, 300));
        let mut before = HashMap::new();
        state.paint(
            PagePoint::new(8.0, 8.0),
            PagePoint::new(8.0, 8.0),
            brush(StrokeMode::Paint, 4.0),
            &mut before,
        );
        state.restore(before);
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn snapshot_preserves_sparsity_until_encoding() {
        fn assert_send<T: Send>() {}
        assert_send::<MaskCommit>();

        let (page, layer) = ids();
        let mut state = MaskState::empty(PhysicalSize::new(600, 600));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(10.0, 10.0),
            PagePoint::new(30.0, 10.0),
            brush(StrokeMode::Paint, 8.0),
            &mut before,
        );
        let commit = state.finish(page, MaskTarget::Layer(layer), dirty).unwrap();
        assert_eq!(commit.snapshot.tiles.len(), 1);
        let decoded = image::load_from_memory(&commit.encode_png().unwrap())
            .unwrap()
            .into_luma8();
        assert_eq!(decoded.get_pixel(20, 10).0[0], 255);
    }

    #[test]
    fn long_diagonal_stroke_has_no_sample_gaps() {
        let (page, layer) = ids();
        let mut state = MaskState::empty(PhysicalSize::new(512, 512));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(4.0, 4.0),
            PagePoint::new(508.0, 508.0),
            brush(StrokeMode::Paint, 6.0),
            &mut before,
        );
        let pixels = image::load_from_memory(
            &state
                .finish(page, MaskTarget::Layer(layer), dirty)
                .unwrap()
                .encode_png()
                .unwrap(),
        )
        .unwrap()
        .into_luma8();
        for coordinate in [32, 128, 256, 384, 480] {
            assert_eq!(pixels.get_pixel(coordinate, coordinate).0[0], 255);
        }
    }
}
