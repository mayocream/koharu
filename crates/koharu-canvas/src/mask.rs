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
type TileKey = (u32, u32);
type StrokeSnapshot = HashMap<TileKey, Option<Arc<Vec<u8>>>>;

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
    tiles: HashMap<TileKey, Arc<Vec<u8>>>,
}

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
        for (&(tile_x, tile_y), tile) in &self.tiles {
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
    generation: u64,
    buffer: MaskBuffer,
}

impl MaskState {
    pub fn empty(size: PhysicalSize) -> Self {
        Self {
            generation: 0,
            buffer: MaskBuffer::empty(size),
        }
    }

    pub fn for_each_tinted_tile(
        &mut self,
        overlay: MaskOverlay,
        mut visit: impl FnMut(u32, u32, &ImageData),
    ) {
        self.buffer
            .for_each_tinted_tile(overlay, |x, y, image| visit(x, y, image));
    }

    pub fn paint(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut StrokeSnapshot,
    ) -> PixelRect {
        self.buffer.paint_segment(from, to, brush, before)
    }

    pub fn restore(&mut self, before: StrokeSnapshot) {
        self.buffer.restore(before);
    }

    pub fn finish(&mut self, page: PageId, target: MaskTarget, dirty: PixelRect) -> MaskCommit {
        self.generation = self.generation.wrapping_add(1).max(1);
        MaskCommit {
            page,
            target,
            dirty,
            generation: self.generation,
            snapshot: self.buffer.snapshot(),
        }
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

pub(crate) struct ActiveStroke {
    pub target: MaskTarget,
    pub brush: Brush,
    pub last: PagePoint,
    before: StrokeSnapshot,
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

    pub fn paint(&mut self, state: &mut MaskState, from: PagePoint, to: PagePoint) -> PixelRect {
        state.paint(from, to, self.brush, &mut self.before)
    }

    pub fn restore(self, state: &mut MaskState) {
        state.restore(self.before);
    }
}

struct MaskBuffer {
    size: PixelSize,
    tiles: HashMap<TileKey, Tile>,
}

impl MaskBuffer {
    fn empty(size: PhysicalSize) -> Self {
        Self {
            size,
            tiles: HashMap::new(),
        }
    }

    fn snapshot(&self) -> MaskSnapshot {
        MaskSnapshot {
            size: self.size,
            tiles: self
                .tiles
                .iter()
                .map(|(&key, tile)| (key, Arc::clone(&tile.pixels)))
                .collect(),
        }
    }

    fn restore(&mut self, before: StrokeSnapshot) {
        for (key, pixels) in before {
            match pixels {
                Some(pixels) => {
                    let (width, height) = self.tile_size(key);
                    self.tiles.insert(
                        key,
                        Tile {
                            width,
                            height,
                            pixels,
                            version: 0,
                            tinted: None,
                        },
                    );
                }
                None => {
                    self.tiles.remove(&key);
                }
            }
        }
    }

    fn paint_segment(
        &mut self,
        from: PagePoint,
        to: PagePoint,
        brush: Brush,
        before: &mut StrokeSnapshot,
    ) -> PixelRect {
        let radius = f64::from(brush.diameter) * 0.5;
        let min_y = ((from.y.min(to.y) - radius).floor().max(0.0) as u32).min(self.size.height);
        let max_y = ((from.y.max(to.y) + radius).ceil().max(0.0) as u32).min(self.size.height);
        if min_y >= max_y {
            return PixelRect::default();
        }

        let value = match brush.mode {
            StrokeMode::Paint => u8::MAX,
            StrokeMode::Erase => 0,
        };
        let mut changed = ChangedBounds::default();
        let mut touched = HashSet::new();
        for y in min_y..max_y {
            let Some((left, right)) = capsule_row_interval(from, to, radius, f64::from(y) + 0.5)
            else {
                continue;
            };
            let first = ((left - 0.5).ceil().max(0.0) as u32).min(self.size.width);
            let last = ((right - 0.5).floor().max(-1.0) as i64).min(i64::from(self.size.width) - 1);
            if i64::from(first) > last {
                continue;
            }
            for x in first..=last as u32 {
                let key = (x / TILE_SIZE, y / TILE_SIZE);
                if value == 0 && !self.tiles.contains_key(&key) {
                    continue;
                }
                before
                    .entry(key)
                    .or_insert_with(|| self.tiles.get(&key).map(|tile| Arc::clone(&tile.pixels)));
                let (width, height) = self.tile_size(key);
                let tile = self.tiles.entry(key).or_insert_with(|| Tile {
                    width,
                    height,
                    pixels: Arc::new(vec![0; width as usize * height as usize]),
                    version: 0,
                    tinted: None,
                });
                let local_x = x - key.0 * TILE_SIZE;
                let local_y = y - key.1 * TILE_SIZE;
                let offset = local_y as usize * tile.width as usize + local_x as usize;
                if tile.pixels[offset] == value {
                    continue;
                }
                Arc::make_mut(&mut tile.pixels)[offset] = value;
                touched.insert(key);
                changed.include(x, y);
            }
        }

        for key in touched {
            let empty = self
                .tiles
                .get(&key)
                .is_some_and(|tile| tile.pixels.iter().all(|pixel| *pixel == 0));
            if empty {
                self.tiles.remove(&key);
            } else if let Some(tile) = self.tiles.get_mut(&key) {
                tile.version = tile.version.wrapping_add(1);
                tile.tinted = None;
            }
        }
        changed.rect()
    }

    fn tile_size(&self, key: TileKey) -> (u32, u32) {
        let x = key.0 * TILE_SIZE;
        let y = key.1 * TILE_SIZE;
        (
            (self.size.width - x).min(TILE_SIZE),
            (self.size.height - y).min(TILE_SIZE),
        )
    }

    fn for_each_tinted_tile(
        &mut self,
        overlay: MaskOverlay,
        mut visit: impl FnMut(u32, u32, &ImageData),
    ) {
        let opacity = overlay.opacity.clamp(0.0, 1.0);
        for (&key, tile) in &mut self.tiles {
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
            visit(
                key.0 * TILE_SIZE,
                key.1 * TILE_SIZE,
                &tile
                    .tinted
                    .as_ref()
                    .expect("occupied mask tile has a tinted image")
                    .image,
            );
        }
    }
}

/// Horizontal interval of a capsule at a pixel-center scanline. A capsule is
/// convex, so the endpoint circles and projected interior strip collapse to
/// one interval. This avoids scanning the segment's full bounding rectangle.
fn capsule_row_interval(from: PagePoint, to: PagePoint, radius: f64, y: f64) -> Option<(f64, f64)> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length_squared = dx.mul_add(dx, dy * dy);
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut include = |start: f64, end: f64| {
        left = left.min(start);
        right = right.max(end);
    };

    for point in [from, to] {
        let vertical = y - point.y;
        if vertical.abs() <= radius {
            let half_width = (radius.mul_add(radius, -vertical * vertical))
                .max(0.0)
                .sqrt();
            include(point.x - half_width, point.x + half_width);
        }
    }

    if length_squared > f64::EPSILON {
        let projection = if dx.abs() > f64::EPSILON {
            let at_start = from.x - (y - from.y) * dy / dx;
            let at_end = from.x + (length_squared - (y - from.y) * dy) / dx;
            Some((at_start.min(at_end), at_start.max(at_end)))
        } else {
            let progress = (y - from.y) * dy / length_squared;
            (0.0..=1.0)
                .contains(&progress)
                .then_some((f64::NEG_INFINITY, f64::INFINITY))
        };
        let strip = if dy.abs() > f64::EPSILON {
            let center = from.x + (y - from.y) * dx / dy;
            let half_width = radius * length_squared.sqrt() / dy.abs();
            Some((center - half_width, center + half_width))
        } else {
            ((y - from.y).abs() <= radius).then_some((f64::NEG_INFINITY, f64::INFINITY))
        };
        if let (Some(projection), Some(strip)) = (projection, strip) {
            let start = projection.0.max(strip.0);
            let end = projection.1.min(strip.1);
            if start <= end {
                include(start, end);
            }
        }
    }

    left.is_finite().then_some((left, right))
}

#[derive(Default)]
struct ChangedBounds {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
    any: bool,
}

impl ChangedBounds {
    fn include(&mut self, x: u32, y: u32) {
        if !self.any {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.any = true;
        } else {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        }
    }

    fn rect(self) -> PixelRect {
        if self.any {
            PixelRect::new(
                self.min_x,
                self.min_y,
                self.max_x - self.min_x + 1,
                self.max_y - self.min_y + 1,
            )
        } else {
            PixelRect::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_id() -> PageId {
        koharu_scene::EntityId::new()
    }

    fn brush(mode: StrokeMode) -> Brush {
        Brush {
            diameter: 8.0,
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
            PagePoint::new(10.0, 10.0),
            PagePoint::new(30.0, 10.0),
            brush(StrokeMode::Erase),
            &mut before,
        );
        assert!(dirty.is_empty());
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn changed_bounds_are_exact_and_empty_tiles_are_released() {
        let mut state = MaskState::empty(PhysicalSize::new(512, 512));
        let mut before = HashMap::new();
        let painted = state.paint(
            PagePoint::new(128.0, 128.0),
            PagePoint::new(128.0, 128.0),
            brush(StrokeMode::Paint),
            &mut before,
        );
        assert_eq!(painted, PixelRect::new(124, 124, 8, 8));
        assert_eq!(state.buffer.tiles.len(), 1);

        let mut erased_before = HashMap::new();
        let erased = state.paint(
            PagePoint::new(128.0, 128.0),
            PagePoint::new(128.0, 128.0),
            brush(StrokeMode::Erase),
            &mut erased_before,
        );
        assert_eq!(erased, painted);
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn cancel_restores_sparse_allocation() {
        let mut state = MaskState::empty(PhysicalSize::new(1024, 1024));
        let mut before = HashMap::new();
        state.paint(
            PagePoint::new(300.0, 300.0),
            PagePoint::new(300.0, 300.0),
            brush(StrokeMode::Paint),
            &mut before,
        );
        assert_eq!(state.buffer.tiles.len(), 1);
        state.restore(before);
        assert!(state.buffer.tiles.is_empty());
    }

    #[test]
    fn snapshot_preserves_sparsity_until_encoding() {
        let mut state = MaskState::empty(PhysicalSize::new(1024, 1024));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(8.0, 8.0),
            PagePoint::new(8.0, 8.0),
            brush(StrokeMode::Paint),
            &mut before,
        );
        let commit = state.finish(page_id(), MaskTarget::Scratch(0), dirty);
        assert_eq!(commit.snapshot.tiles.len(), 1);
        let decoded = image::load_from_memory(&commit.encode_png().unwrap())
            .unwrap()
            .into_luma8();
        assert_eq!(decoded.get_pixel(8, 8).0[0], 255);
        assert_eq!(decoded.get_pixel(900, 900).0[0], 0);
    }

    #[test]
    fn long_diagonal_stroke_has_no_sample_gaps() {
        let mut state = MaskState::empty(PhysicalSize::new(512, 512));
        let mut before = HashMap::new();
        let dirty = state.paint(
            PagePoint::new(4.0, 4.0),
            PagePoint::new(508.0, 508.0),
            Brush {
                diameter: 6.0,
                ..brush(StrokeMode::Paint)
            },
            &mut before,
        );
        let pixels = image::load_from_memory(
            &state
                .finish(page_id(), MaskTarget::Scratch(0), dirty)
                .encode_png()
                .unwrap(),
        )
        .unwrap()
        .into_luma8();
        for coordinate in [32, 128, 256, 384, 480] {
            assert_eq!(pixels.get_pixel(coordinate, coordinate).0[0], 255);
        }
    }

    #[test]
    fn scanline_capsule_matches_distance_to_segment() {
        for (from, to, diameter) in [
            (PagePoint::new(8.0, 14.0), PagePoint::new(55.0, 41.0), 9.0),
            (PagePoint::new(32.0, 4.0), PagePoint::new(32.0, 59.0), 12.0),
            (PagePoint::new(58.0, 20.0), PagePoint::new(5.0, 20.0), 7.0),
        ] {
            let mut state = MaskState::empty(PhysicalSize::new(64, 64));
            let mut before = HashMap::new();
            let dirty = state.paint(
                from,
                to,
                Brush {
                    diameter,
                    ..brush(StrokeMode::Paint)
                },
                &mut before,
            );
            let pixels = state
                .finish(page_id(), MaskTarget::Scratch(0), dirty)
                .snapshot
                .flatten();
            let dx = to.x - from.x;
            let dy = to.y - from.y;
            let length_squared = dx.mul_add(dx, dy * dy);
            for y in 0_u32..64 {
                for x in 0_u32..64 {
                    let px = f64::from(x) + 0.5;
                    let py = f64::from(y) + 0.5;
                    let progress = (((px - from.x) * dx + (py - from.y) * dy) / length_squared)
                        .clamp(0.0, 1.0);
                    let nearest_x = from.x + dx * progress;
                    let nearest_y = from.y + dy * progress;
                    let expected = (px - nearest_x)
                        .mul_add(px - nearest_x, (py - nearest_y) * (py - nearest_y))
                        <= f64::from(diameter * diameter) * 0.25;
                    assert_eq!(
                        pixels[(y * 64 + x) as usize] != 0,
                        expected,
                        "pixel ({x}, {y})"
                    );
                }
            }
        }
    }
}
