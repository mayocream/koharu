use image::{
    DynamicImage, GrayImage, Luma, Rgb, RgbImage,
    imageops::{self},
};
use imageproc::{
    contours::{BorderType as ContourBorderType, find_contours},
    contrast::otsu_level,
    distance_transform::Norm,
    drawing::draw_polygon_mut,
    geometric_transformations::{Interpolation, Projection, warp_into},
    morphology::{dilate, erode},
    point::Point,
    region_labelling::{Connectivity, connected_components},
};
use koharu_types::{TextBlock, TextDirection};

const LINE_THRESHOLD: f32 = 0.3;
const LINE_SCORE_THRESHOLD: f32 = 0.6;
const MASK_SCORE_THRESHOLD: f32 = 0.1;
const CTD_UNCLIP_RATIO: f32 = 1.5;
const FINAL_MASK_DILATE_RADIUS: u8 = 2;

pub type Quad = [[f32; 2]; 4];

#[derive(Debug, Clone)]
pub struct ComicTextDetection {
    pub shrink_map: GrayImage,
    pub threshold_map: GrayImage,
    pub line_polygons: Vec<Quad>,
    pub text_blocks: Vec<TextBlock>,
    pub mask: GrayImage,
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreMap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl ScoreMap {
    pub(crate) fn get(&self, x: u32, y: u32) -> f32 {
        self.values[(y * self.width + x) as usize]
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DetectionMaps {
    pub raw_shrink_map: ScoreMap,
    pub raw_threshold_map: ScoreMap,
    pub shrink_map: GrayImage,
    pub threshold_map: GrayImage,
    pub mask_map: GrayImage,
}

#[derive(Debug, Clone)]
struct DetectedLine {
    quad: Quad,
    vertical: bool,
    score: f32,
}

#[derive(Debug, Clone)]
struct CtdBlock {
    bbox: [f32; 4],
    confidence: f32,
    source_language: String,
    source_direction: TextDirection,
    lines: Vec<Quad>,
    angle_deg: f32,
    detected_font_size_px: f32,
    distances: Vec<f32>,
    direction_vec: [f32; 2],
    direction_norm: f32,
    merged: bool,
}

#[derive(Debug, Clone)]
struct Component {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    area: u32,
    pixels: Vec<(u32, u32)>,
}

#[derive(Debug, Clone)]
struct CandidateMask {
    mask: GrayImage,
    xor_sum: u64,
}

impl CtdBlock {
    fn from_line(line: &DetectedLine) -> Self {
        let bbox = quad_bbox(&line.quad);
        Self {
            bbox,
            confidence: line.score,
            source_language: "unknown".to_string(),
            source_direction: if line.vertical {
                TextDirection::Vertical
            } else {
                TextDirection::Horizontal
            },
            lines: vec![line.quad],
            angle_deg: 0.0,
            detected_font_size_px: 0.0,
            distances: Vec::new(),
            direction_vec: [1.0, 0.0],
            direction_norm: 1.0,
            merged: false,
        }
    }

    fn center(&self) -> [f32; 2] {
        [
            (self.bbox[0] + self.bbox[2]) * 0.5,
            (self.bbox[1] + self.bbox[3]) * 0.5,
        ]
    }

    fn adjust_bbox(&mut self, with_bbox: bool) {
        if self.lines.is_empty() {
            return;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for line in &self.lines {
            let bbox = quad_bbox(line);
            min_x = min_x.min(bbox[0]);
            min_y = min_y.min(bbox[1]);
            max_x = max_x.max(bbox[2]);
            max_y = max_y.max(bbox[3]);
        }

        if with_bbox {
            self.bbox[0] = self.bbox[0].min(min_x);
            self.bbox[1] = self.bbox[1].min(min_y);
            self.bbox[2] = self.bbox[2].max(max_x);
            self.bbox[3] = self.bbox[3].max(max_y);
        } else {
            self.bbox = [min_x, min_y, max_x, max_y];
        }
    }

    fn sort_lines(&mut self) {
        if self.distances.len() != self.lines.len() {
            return;
        }

        let mut indexed: Vec<(f32, Quad)> = self
            .distances
            .iter()
            .copied()
            .zip(self.lines.iter().copied())
            .collect();
        indexed.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.distances = indexed.iter().map(|(distance, _)| *distance).collect();
        self.lines = indexed.into_iter().map(|(_, line)| line).collect();
    }
}

pub fn build_detection(
    image: &DynamicImage,
    maps: DetectionMaps,
) -> anyhow::Result<ComicTextDetection> {
    let DetectionMaps {
        raw_shrink_map,
        raw_threshold_map,
        shrink_map,
        threshold_map,
        mask_map,
    } = maps;
    let scale_x = image.width() as f32 / raw_shrink_map.width.max(1) as f32;
    let scale_y = image.height() as f32 / raw_shrink_map.height.max(1) as f32;
    let detected_lines = extract_detected_lines(&raw_shrink_map, &raw_threshold_map)
        .into_iter()
        .map(|mut line| {
            if (scale_x - 1.0).abs() > f32::EPSILON || (scale_y - 1.0).abs() > f32::EPSILON {
                line.quad = scale_quad(&line.quad, scale_x, scale_y);
            }
            line
        })
        .collect::<Vec<_>>();
    let line_polygons = detected_lines.iter().map(|line| line.quad).collect();
    let text_blocks = group_output(&detected_lines, &mask_map, image.width(), image.height());
    let refined_mask = refine_mask(&image.to_rgb8(), &mask_map, &text_blocks);
    let mask = dilate(&refined_mask, Norm::L1, FINAL_MASK_DILATE_RADIUS);

    Ok(ComicTextDetection {
        shrink_map,
        threshold_map,
        line_polygons,
        text_blocks,
        mask,
    })
}

pub fn crop_text_block_bbox(image: &DynamicImage, block: &TextBlock) -> DynamicImage {
    let [x1, y1, x2, y2] = expanded_text_block_crop_bounds(image.width(), image.height(), block);
    image.crop_imm(x1, y1, x2.saturating_sub(x1), y2.saturating_sub(y1))
}

pub fn extract_text_block_regions(image: &DynamicImage, block: &TextBlock) -> Vec<DynamicImage> {
    let Some(line_polygons) = block.line_polygons.as_ref() else {
        return vec![crop_text_block_bbox(image, block)];
    };
    if line_polygons.is_empty() {
        return vec![crop_text_block_bbox(image, block)];
    }

    let rgb = image.to_rgb8();
    let mut regions = Vec::with_capacity(line_polygons.len());
    for line in line_polygons {
        if let Some(region) = warp_line_region(&rgb, block, line) {
            regions.push(DynamicImage::ImageRgb8(region));
        }
    }

    if regions.is_empty() {
        vec![crop_text_block_bbox(image, block)]
    } else {
        regions
    }
}

fn expanded_text_block_crop_bounds(
    image_width: u32,
    image_height: u32,
    block: &TextBlock,
) -> [u32; 4] {
    let should_expand = block.detector.as_deref() == Some("ctd")
        || block
            .line_polygons
            .as_ref()
            .map(|lines| !lines.is_empty())
            .unwrap_or(false);
    if !should_expand {
        let x1 = block.x.max(0.0).floor() as u32;
        let y1 = block.y.max(0.0).floor() as u32;
        let x2 = (block.x + block.width)
            .ceil()
            .clamp(x1 as f32 + 1.0, image_width as f32) as u32;
        let y2 = (block.y + block.height)
            .ceil()
            .clamp(y1 as f32 + 1.0, image_height as f32) as u32;
        return [x1, y1, x2, y2];
    }

    let mut min_x = block.x;
    let mut min_y = block.y;
    let mut max_x = block.x + block.width;
    let mut max_y = block.y + block.height;

    if let Some(line_polygons) = block.line_polygons.as_ref() {
        for line in line_polygons {
            let quad = maybe_expand_ctd_line(block, line);
            let bbox = quad_bbox(&quad);
            min_x = min_x.min(bbox[0]);
            min_y = min_y.min(bbox[1]);
            max_x = max_x.max(bbox[2]);
            max_y = max_y.max(bbox[3]);
        }
    }

    let font = block
        .detected_font_size_px
        .unwrap_or_else(|| block.width.min(block.height).max(1.0));
    let base_pad = (font * 0.08).max(2.0);
    let (pad_x, pad_y) = match block.source_direction.unwrap_or(TextDirection::Horizontal) {
        TextDirection::Horizontal => ((font * 0.12).max(base_pad), (font * 0.18).max(base_pad)),
        TextDirection::Vertical => ((font * 0.18).max(base_pad), (font * 0.12).max(base_pad)),
    };

    let x1 = (min_x - pad_x)
        .floor()
        .clamp(0.0, image_width.saturating_sub(1) as f32) as u32;
    let y1 = (min_y - pad_y)
        .floor()
        .clamp(0.0, image_height.saturating_sub(1) as f32) as u32;
    let x2 = (max_x + pad_x)
        .ceil()
        .clamp(x1 as f32 + 1.0, image_width as f32) as u32;
    let y2 = (max_y + pad_y)
        .ceil()
        .clamp(y1 as f32 + 1.0, image_height as f32) as u32;
    [x1, y1, x2, y2]
}

fn warp_line_region(image: &RgbImage, block: &TextBlock, line: &Quad) -> Option<RgbImage> {
    let expanded = maybe_expand_ctd_line(block, line);
    let clipped = clip_quad(&expanded, image.width() as f32, image.height() as f32);
    let bbox = quad_bbox(&clipped);
    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().min(image.width() as f32) as u32;
    let y2 = bbox[3].ceil().min(image.height() as f32) as u32;
    if x2 <= x1 || y2 <= y1 {
        return None;
    }

    let cropped = imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
    let mut src = clipped;
    for point in &mut src {
        point[0] -= x1 as f32;
        point[1] -= y1 as f32;
    }

    let (norm_v, norm_h) = quad_axis_lengths(&src);
    if norm_v <= 0.0 || norm_h <= 0.0 {
        return None;
    }

    let direction = block.source_direction.unwrap_or(TextDirection::Horizontal);
    let text_height = match direction {
        TextDirection::Horizontal => norm_v.max(1.0).round() as u32,
        TextDirection::Vertical => norm_h.max(1.0).round() as u32,
    }
    .max(1);
    let ratio = norm_v / norm_h;

    let (width, height, rotate_vertical) = match direction {
        TextDirection::Horizontal => {
            let h = text_height.max(1);
            let w = ((text_height as f32 / ratio).round() as u32).max(1);
            (w, h, false)
        }
        TextDirection::Vertical => {
            let w = text_height.max(1);
            let h = ((text_height as f32 * ratio).round() as u32).max(1);
            (w, h, true)
        }
    };

    let dst = [
        (0.0f32, 0.0f32),
        ((width.saturating_sub(1)) as f32, 0.0f32),
        (
            (width.saturating_sub(1)) as f32,
            (height.saturating_sub(1)) as f32,
        ),
        (0.0f32, (height.saturating_sub(1)) as f32),
    ];
    let src = quad_to_tuples(&src);
    let projection = Projection::from_control_points(src, dst)?;

    let mut region = RgbImage::from_pixel(width, height, Rgb([0, 0, 0]));
    warp_into(
        &cropped,
        &projection,
        Interpolation::Bilinear,
        Rgb([0, 0, 0]),
        &mut region,
    );

    if rotate_vertical {
        Some(imageops::rotate270(&region))
    } else {
        Some(region)
    }
}

fn maybe_expand_ctd_line(block: &TextBlock, line: &Quad) -> Quad {
    let should_expand = block.detector.as_deref() == Some("ctd")
        && block.source_direction == Some(TextDirection::Horizontal);
    if !should_expand {
        return *line;
    }

    let expand_size = (block.detected_font_size_px.unwrap_or(0.0) * 0.1).max(3.0);
    let angle = block.rotation_deg.unwrap_or(0.0).to_radians();
    let sin = angle.sin();
    let cos = angle.cos();
    let signs = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    let mut out = *line;
    for (index, point) in out.iter_mut().enumerate() {
        point[0] += signs[index][0] * sin * expand_size;
        point[1] += signs[index][1] * cos * expand_size;
    }
    out
}

fn extract_detected_lines(shrink_map: &ScoreMap, _threshold_map: &ScoreMap) -> Vec<DetectedLine> {
    let binary = GrayImage::from_fn(shrink_map.width, shrink_map.height, |x, y| {
        let shrink_score = shrink_map.get(x, y);
        if shrink_score > LINE_THRESHOLD {
            Luma([255u8])
        } else {
            Luma([0u8])
        }
    });

    let contours = find_contours::<i32>(&binary);
    let mut lines = Vec::new();
    for contour in contours {
        if contour.border_type != ContourBorderType::Outer || contour.points.len() < 4 {
            continue;
        }

        let contour_points = contour
            .points
            .into_iter()
            .map(|point| [point.x as f32, point.y as f32])
            .collect::<Vec<_>>();
        let score = contour_score_fast(shrink_map, &contour_points);
        if score < LINE_SCORE_THRESHOLD {
            continue;
        }

        if let Some((quad, vertical)) = contour_quad(&contour_points) {
            let (norm_v, norm_h) = quad_axis_lengths(&quad);
            if norm_v.min(norm_h) < 2.0 {
                continue;
            }
            lines.push(DetectedLine {
                quad,
                vertical,
                score,
            });
        }
    }

    lines
}

#[cfg(test)]
fn component_quad(component: &Component) -> Option<(Quad, bool)> {
    if component.pixels.len() < 2 {
        return None;
    }

    let mut mean_x = 0.0f32;
    let mut mean_y = 0.0f32;
    for (x, y) in &component.pixels {
        mean_x += *x as f32;
        mean_y += *y as f32;
    }
    let n = component.pixels.len() as f32;
    mean_x /= n;
    mean_y /= n;

    let mut sxx = 0.0f32;
    let mut syy = 0.0f32;
    let mut sxy = 0.0f32;
    for (x, y) in &component.pixels {
        let dx = *x as f32 - mean_x;
        let dy = *y as f32 - mean_y;
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let ux = angle.cos();
    let uy = angle.sin();
    let vx = -uy;
    let vy = ux;

    let mut min_u = f32::MAX;
    let mut max_u = f32::MIN;
    let mut min_v = f32::MAX;
    let mut max_v = f32::MIN;
    for (x, y) in &component.pixels {
        let dx = *x as f32 - mean_x;
        let dy = *y as f32 - mean_y;
        let u = dx * ux + dy * uy;
        let v = dx * vx + dy * vy;
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }

    let width = (max_u - min_u).max(1.0);
    let height = (max_v - min_v).max(1.0);
    let perimeter = 2.0 * (width + height);
    let offset = if perimeter > 0.0 {
        (width * height * CTD_UNCLIP_RATIO) / perimeter
    } else {
        0.0
    };
    min_u -= offset;
    max_u += offset;
    min_v -= offset;
    max_v += offset;

    let quad = [
        [
            mean_x + ux * min_u + vx * min_v,
            mean_y + uy * min_u + vy * min_v,
        ],
        [
            mean_x + ux * max_u + vx * min_v,
            mean_y + uy * max_u + vy * min_v,
        ],
        [
            mean_x + ux * max_u + vx * max_v,
            mean_y + uy * max_u + vy * max_v,
        ],
        [
            mean_x + ux * min_u + vx * max_v,
            mean_y + uy * min_u + vy * max_v,
        ],
    ];
    let (quad, vertical) = sort_quad_points(&quad);
    Some((quad, vertical))
}

fn contour_quad(points: &[[f32; 2]]) -> Option<(Quad, bool)> {
    let quad = minimum_area_rect(points)?;
    let area = polygon_area(&quad);
    let perimeter = polygon_perimeter(&quad);
    let offset = if perimeter > 0.0 {
        (area * CTD_UNCLIP_RATIO) / perimeter
    } else {
        0.0
    };
    let expanded = expand_quad(&quad, offset);
    let (quad, vertical) = sort_quad_points(&expanded);
    Some((quad, vertical))
}

fn minimum_area_rect(points: &[[f32; 2]]) -> Option<Quad> {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        return None;
    }

    let mut best_area = f32::MAX;
    let mut best_quad = None;
    for index in 0..hull.len() {
        let next = (index + 1) % hull.len();
        let edge = [
            hull[next][0] - hull[index][0],
            hull[next][1] - hull[index][1],
        ];
        let edge_norm = vector_norm(edge);
        if edge_norm <= 1e-6 {
            continue;
        }

        let axis_u = [edge[0] / edge_norm, edge[1] / edge_norm];
        let axis_v = [-axis_u[1], axis_u[0]];
        let mut min_u = f32::MAX;
        let mut max_u = f32::MIN;
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;

        for point in &hull {
            let proj_u = dot(*point, axis_u);
            let proj_v = dot(*point, axis_v);
            min_u = min_u.min(proj_u);
            max_u = max_u.max(proj_u);
            min_v = min_v.min(proj_v);
            max_v = max_v.max(proj_v);
        }

        let width = max_u - min_u;
        let height = max_v - min_v;
        let area = width * height;
        if area >= best_area {
            continue;
        }

        best_area = area;
        best_quad = Some([
            [
                axis_u[0] * min_u + axis_v[0] * min_v,
                axis_u[1] * min_u + axis_v[1] * min_v,
            ],
            [
                axis_u[0] * max_u + axis_v[0] * min_v,
                axis_u[1] * max_u + axis_v[1] * min_v,
            ],
            [
                axis_u[0] * max_u + axis_v[0] * max_v,
                axis_u[1] * max_u + axis_v[1] * max_v,
            ],
            [
                axis_u[0] * min_u + axis_v[0] * max_v,
                axis_u[1] * min_u + axis_v[1] * max_v,
            ],
        ]);
    }

    best_quad
}

fn expand_quad(quad: &Quad, offset: f32) -> Quad {
    if offset <= 0.0 {
        return *quad;
    }

    let axis_u = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let axis_v = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let norm_u = vector_norm(axis_u).max(1e-6);
    let norm_v = vector_norm(axis_v).max(1e-6);
    let unit_u = [axis_u[0] / norm_u, axis_u[1] / norm_u];
    let unit_v = [axis_v[0] / norm_v, axis_v[1] / norm_v];
    let signs = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    let mut out = *quad;
    for (index, point) in out.iter_mut().enumerate() {
        point[0] += unit_u[0] * signs[index][0] * offset + unit_v[0] * signs[index][1] * offset;
        point[1] += unit_u[1] * signs[index][0] * offset + unit_v[1] * signs[index][1] * offset;
    }
    out
}

fn convex_hull(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut points = points.to_vec();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    if points.len() <= 2 {
        return points;
    }

    let mut lower: Vec<[f32; 2]> = Vec::new();
    for point in &points {
        while lower.len() >= 2
            && cross_2d(
                [
                    lower[lower.len() - 1][0] - lower[lower.len() - 2][0],
                    lower[lower.len() - 1][1] - lower[lower.len() - 2][1],
                ],
                [
                    point[0] - lower[lower.len() - 1][0],
                    point[1] - lower[lower.len() - 1][1],
                ],
            ) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }

    let mut upper: Vec<[f32; 2]> = Vec::new();
    for point in points.iter().rev() {
        while upper.len() >= 2
            && cross_2d(
                [
                    upper[upper.len() - 1][0] - upper[upper.len() - 2][0],
                    upper[upper.len() - 1][1] - upper[upper.len() - 2][1],
                ],
                [
                    point[0] - upper[upper.len() - 1][0],
                    point[1] - upper[upper.len() - 1][1],
                ],
            ) <= 0.0
        {
            upper.pop();
        }
        upper.push(*point);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn polygon_area(quad: &Quad) -> f32 {
    let mut area = 0.0;
    for index in 0..quad.len() {
        let next = (index + 1) % quad.len();
        area += quad[index][0] * quad[next][1] - quad[next][0] * quad[index][1];
    }
    area.abs() * 0.5
}

fn polygon_perimeter(quad: &Quad) -> f32 {
    let mut perimeter = 0.0;
    for index in 0..quad.len() {
        let next = (index + 1) % quad.len();
        perimeter += vector_norm([
            quad[next][0] - quad[index][0],
            quad[next][1] - quad[index][1],
        ]);
    }
    perimeter
}

fn contour_score_fast(image: &ScoreMap, polygon: &[[f32; 2]]) -> f32 {
    if polygon.is_empty() {
        return 0.0;
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for point in polygon {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }

    let x1 = min_x
        .floor()
        .clamp(0.0, image.width.saturating_sub(1) as f32) as u32;
    let y1 = min_y
        .floor()
        .clamp(0.0, image.height.saturating_sub(1) as f32) as u32;
    let x2 = max_x.ceil().clamp(x1 as f32 + 1.0, image.width as f32) as u32;
    let y2 = max_y.ceil().clamp(y1 as f32 + 1.0, image.height as f32) as u32;

    let mut mask = GrayImage::new(x2 - x1, y2 - y1);
    let shifted = polygon
        .iter()
        .map(|point| {
            Point::new(
                (point[0] - x1 as f32).round() as i32,
                (point[1] - y1 as f32).round() as i32,
            )
        })
        .collect::<Vec<_>>();
    draw_polygon_mut(&mut mask, &shifted, Luma([1u8]));

    let mut sum = 0.0;
    let mut count = 0.0;
    for y in y1..y2 {
        for x in x1..x2 {
            if mask.get_pixel(x - x1, y - y1)[0] > 0 {
                sum += image.get(x, y);
                count += 1.0;
            }
        }
    }

    if count <= 0.0 { 0.0 } else { sum / count }
}

fn group_output(
    lines: &[DetectedLine],
    mask: &GrayImage,
    image_width: u32,
    image_height: u32,
) -> Vec<TextBlock> {
    let mut scattered_horizontal = Vec::new();
    let mut scattered_vertical = Vec::new();

    for line in lines {
        let line_bbox = quad_bbox(&line.quad);
        if mean_mask_score(mask, &line_bbox) >= MASK_SCORE_THRESHOLD {
            let mut block = CtdBlock::from_line(line);
            examine_block(&mut block, image_width, image_height, false);
            if block.source_direction == TextDirection::Vertical {
                scattered_vertical.push(block);
            } else {
                scattered_horizontal.push(block);
            }
        }
    }

    let mut final_blocks = Vec::new();
    scattered_vertical.sort_by(|a, b| b.center()[0].total_cmp(&a.center()[0]));
    scattered_horizontal.sort_by(|a, b| a.center()[1].total_cmp(&b.center()[1]));

    final_blocks.extend(merge_text_lines(scattered_horizontal, 2.0));
    final_blocks.extend(merge_text_lines(scattered_vertical, 1.7));
    final_blocks = merge_paragraph_blocks(final_blocks, image_width, image_height);

    let mut sorted = sort_regions(final_blocks);
    dedupe_blocks(&mut sorted);
    sorted.into_iter().map(block_to_text_block).collect()
}

fn block_to_text_block(block: CtdBlock) -> TextBlock {
    let width = (block.bbox[2] - block.bbox[0]).max(1.0);
    let height = (block.bbox[3] - block.bbox[1]).max(1.0);
    TextBlock {
        x: block.bbox[0],
        y: block.bbox[1],
        width,
        height,
        confidence: block.confidence,
        line_polygons: Some(block.lines),
        source_direction: Some(block.source_direction),
        source_language: Some(block.source_language),
        rotation_deg: Some(block.angle_deg),
        detected_font_size_px: Some(block.detected_font_size_px.max(1.0)),
        detector: Some("ctd".to_string()),
        ..Default::default()
    }
}

fn examine_block(block: &mut CtdBlock, image_width: u32, image_height: u32, sort: bool) {
    if block.lines.is_empty() {
        block.detected_font_size_px = block.bbox_height().min(block.bbox_width()).max(1.0);
        return;
    }

    let mut centers = Vec::with_capacity(block.lines.len());
    let mut vec_v_sum = [0.0f32, 0.0f32];
    let mut vec_h_sum = [0.0f32, 0.0f32];
    let mut font_acc = 0.0f32;

    for line in &block.lines {
        let middle = quad_midpoints(line);
        let vec_v = [middle[2][0] - middle[0][0], middle[2][1] - middle[0][1]];
        let vec_h = [middle[1][0] - middle[3][0], middle[1][1] - middle[3][1]];
        vec_v_sum[0] += vec_v[0];
        vec_v_sum[1] += vec_v[1];
        vec_h_sum[0] += vec_h[0];
        vec_h_sum[1] += vec_h[1];
        centers.push([
            (line[0][0] + line[2][0]) * 0.5,
            (line[0][1] + line[2][1]) * 0.5,
        ]);
        font_acc += match block.source_direction {
            TextDirection::Vertical => vector_norm(vec_h),
            TextDirection::Horizontal => vector_norm(vec_v),
        };
    }

    let (primary_vec, primary_norm) = match block.source_direction {
        TextDirection::Vertical => (vec_v_sum, vector_norm(vec_v_sum)),
        TextDirection::Horizontal => (vec_h_sum, vector_norm(vec_h_sum)),
    };

    block.detected_font_size_px = (font_acc / block.lines.len() as f32).max(1.0);
    block.direction_vec = primary_vec;
    block.direction_norm = primary_norm.max(1.0);
    block.distances = centers
        .iter()
        .map(|center| {
            let origin = match block.source_direction {
                TextDirection::Vertical => [image_width as f32, 0.0],
                TextDirection::Horizontal => [0.0, 0.0],
            };
            perpendicular_distance(
                [center[0] - origin[0], center[1] - origin[1]],
                primary_vec,
                image_height as f32,
            )
        })
        .collect();

    let mut angle = primary_vec[1].atan2(primary_vec[0]).to_degrees();
    if block.source_direction == TextDirection::Vertical {
        angle -= 90.0;
    }
    if angle.abs() < 3.0 {
        angle = 0.0;
    }
    block.angle_deg = angle;
    if sort {
        block.sort_lines();
    }
}

fn merge_text_lines(blocks: Vec<CtdBlock>, font_size_tol: f32) -> Vec<CtdBlock> {
    if blocks.len() < 2 {
        return blocks;
    }

    let mut blocks = blocks;
    let mut merged = Vec::new();
    for index in 0..blocks.len() {
        if blocks[index].merged {
            continue;
        }
        let mut current = blocks[index].clone();
        for other in blocks.iter_mut().skip(index + 1) {
            try_merge_text_line(&mut current, other, font_size_tol);
        }
        current.adjust_bbox(false);
        merged.push(current);
    }
    merged
}

fn merge_paragraph_blocks(
    blocks: Vec<CtdBlock>,
    image_width: u32,
    image_height: u32,
) -> Vec<CtdBlock> {
    if blocks.len() < 2 {
        return blocks;
    }

    let mut blocks = sort_regions(blocks);
    let mut merged = Vec::new();
    for index in 0..blocks.len() {
        if blocks[index].merged {
            continue;
        }
        let mut current = blocks[index].clone();
        while let Some(candidate_index) = find_paragraph_merge_candidate(&current, &blocks, index) {
            let other = &mut blocks[candidate_index];
            merge_paragraph_block(&mut current, other, image_width, image_height);
        }
        current.adjust_bbox(false);
        merged.push(current);
    }
    merged
}

fn find_paragraph_merge_candidate(
    current: &CtdBlock,
    blocks: &[CtdBlock],
    current_index: usize,
) -> Option<usize> {
    let mut best: Option<(usize, f32, f32)> = None;
    for candidate_index in current_index + 1..blocks.len() {
        let candidate = &blocks[candidate_index];
        let Some(gap_y) = paragraph_merge_gap(current, candidate) else {
            continue;
        };
        if paragraph_merge_blocked(current, candidate, blocks, current_index, candidate_index) {
            continue;
        }

        let overlap_x = horizontal_overlap(&current.bbox, &candidate.bbox);
        match best {
            Some((_, best_gap, best_overlap)) => {
                if gap_y < best_gap - 1e-3
                    || ((gap_y - best_gap).abs() <= 1e-3 && overlap_x > best_overlap)
                {
                    best = Some((candidate_index, gap_y, overlap_x));
                }
            }
            None => best = Some((candidate_index, gap_y, overlap_x)),
        }
    }

    best.map(|(index, _, _)| index)
}

fn paragraph_merge_gap(block: &CtdBlock, other: &CtdBlock) -> Option<f32> {
    if other.merged
        || block.source_direction != TextDirection::Horizontal
        || other.source_direction != TextDirection::Horizontal
        || block.lines.is_empty()
        || other.lines.is_empty()
    {
        return None;
    }

    let count_a = block.lines.len() as f32;
    let count_b = other.lines.len() as f32;
    let font_avg = (block.detected_font_size_px * count_a + other.detected_font_size_px * count_b)
        / (count_a + count_b).max(1.0);
    if font_avg <= 0.0 {
        return None;
    }

    let font_ratio = block.detected_font_size_px / other.detected_font_size_px.max(1e-6);
    if font_ratio > 2.0 || font_ratio.recip() > 2.0 {
        return None;
    }

    let (upper, lower) = if block.center()[1] <= other.center()[1] {
        (block.bbox, other.bbox)
    } else {
        (other.bbox, block.bbox)
    };
    let gap_y = lower[1] - upper[3];
    if gap_y < -font_avg * 0.25 || gap_y > font_avg * 0.9 {
        return None;
    }

    let left_diff = (block.bbox[0] - other.bbox[0]).abs();
    let right_diff = (block.bbox[2] - other.bbox[2]).abs();
    let overlap_x = horizontal_overlap(&block.bbox, &other.bbox);
    let width_similarity = overlap_x / block.bbox_width().min(other.bbox_width()).max(1.0);
    let aligned_left = left_diff <= font_avg * 1.1;
    let aligned_right = right_diff <= font_avg * 1.1;
    let ragged_continue = left_diff <= font_avg * 1.8 && width_similarity >= 0.75;
    if !(aligned_left || aligned_right || ragged_continue) {
        return None;
    }

    let vec_prod = dot(block.direction_vec, other.direction_vec);
    let cos_vec = vec_prod / (block.direction_norm * other.direction_norm).max(1e-6);
    if cos_vec.abs() < 0.95 {
        return None;
    }

    let angle_diff = (block.angle_deg - other.angle_deg).abs();
    if angle_diff > 8.0 {
        return None;
    }

    Some(gap_y.max(0.0))
}

fn paragraph_merge_blocked(
    block: &CtdBlock,
    other: &CtdBlock,
    blocks: &[CtdBlock],
    current_index: usize,
    candidate_index: usize,
) -> bool {
    let column_left = block.bbox[0].max(other.bbox[0]);
    let column_right = block.bbox[2].min(other.bbox[2]);
    if column_right <= column_left {
        return false;
    }

    let (upper, lower) = if block.center()[1] <= other.center()[1] {
        (block.bbox, other.bbox)
    } else {
        (other.bbox, block.bbox)
    };
    let gap_top = upper[3];
    let gap_bottom = lower[1];
    if gap_bottom <= gap_top {
        return false;
    }

    for (index, candidate) in blocks.iter().enumerate() {
        if index == current_index || index == candidate_index || candidate.merged {
            continue;
        }
        if candidate.source_direction != TextDirection::Horizontal {
            continue;
        }
        if candidate.bbox[3] <= gap_top || candidate.bbox[1] >= gap_bottom {
            continue;
        }
        let candidate_overlap =
            (candidate.bbox[2].min(column_right) - candidate.bbox[0].max(column_left)).max(0.0);
        if candidate_overlap > 0.0 {
            return true;
        }
    }

    false
}

fn merge_paragraph_block(
    block: &mut CtdBlock,
    other: &mut CtdBlock,
    image_width: u32,
    image_height: u32,
) -> bool {
    let Some(_) = paragraph_merge_gap(block, other) else {
        return false;
    };
    let top = block.bbox[1].min(other.bbox[1]);
    let bottom = block.bbox[3].max(other.bbox[3]);
    let count_a = block.lines.len() as f32;
    let count_b = other.lines.len() as f32;
    let font_avg = (block.detected_font_size_px * count_a + other.detected_font_size_px * count_b)
        / (count_a + count_b).max(1.0);

    block.lines.extend(other.lines.iter().copied());
    block.direction_vec = [
        block.direction_vec[0] + other.direction_vec[0],
        block.direction_vec[1] + other.direction_vec[1],
    ];
    block.direction_norm = vector_norm(block.direction_vec).max(1.0);
    block.distances.extend(other.distances.iter().copied());
    block.detected_font_size_px = font_avg.max(1.0);
    block.confidence = block.confidence.max(other.confidence);
    block.bbox = [
        block.bbox[0].min(other.bbox[0]),
        top,
        block.bbox[2].max(other.bbox[2]),
        bottom,
    ];
    other.merged = true;
    examine_block(block, image_width, image_height, true);
    true
}

fn try_merge_text_line(block: &mut CtdBlock, other: &mut CtdBlock, font_size_tol: f32) -> bool {
    if other.merged || block.lines.is_empty() || other.lines.is_empty() {
        return false;
    }
    if block.detected_font_size_px <= 0.0 || other.detected_font_size_px <= 0.0 {
        return false;
    }

    let font_ratio = block.detected_font_size_px / other.detected_font_size_px;
    let count_a = block.lines.len() as f32;
    let count_b = other.lines.len() as f32;
    let font_avg = (block.detected_font_size_px * count_a + other.detected_font_size_px * count_b)
        / (count_a + count_b);
    let vec_prod = dot(block.direction_vec, other.direction_vec);
    let cos_vec = vec_prod / (block.direction_norm * other.direction_norm).max(1e-6);
    let line_a = block.lines[block.lines.len() - 1];
    let line_b = other.lines[0];
    let bbox_a = quad_bbox(&line_a);
    let bbox_b = quad_bbox(&line_b);
    let distance_x = bbox_a[0].max(bbox_b[0]) - bbox_a[2].min(bbox_b[2]);
    let distance_y = bbox_a[1].max(bbox_b[1]) - bbox_a[3].min(bbox_b[3]);
    let w1 = (bbox_a[2] - bbox_a[0]).max(1.0);
    let w2 = (bbox_b[2] - bbox_b[0]).max(1.0);
    let h1 = (bbox_a[3] - bbox_a[1]).max(1.0);
    let h2 = (bbox_b[3] - bbox_b[1]).max(1.0);

    if !quads_intersect(&line_a, &line_b) {
        match block.source_direction {
            TextDirection::Vertical => {
                if distance_y > 0.0 {
                    return false;
                }
                if distance_x > font_avg * 0.8 {
                    return false;
                }
                if distance_y.abs() / h1.min(h2) < 0.4 {
                    return false;
                }
            }
            TextDirection::Horizontal => {
                if distance_x > 0.0 {
                    return false;
                }
                let width_similarity = (w1.min(w2) / w1.max(w2)).clamp(0.0, 1.0);
                let mut font_threshold = if font_avg < 24.0 { 0.6 } else { 0.5 };
                if width_similarity > 0.95 {
                    font_threshold -= 0.08;
                } else if width_similarity < 0.88 {
                    font_threshold += 0.1;
                }
                if distance_y > font_avg * font_threshold {
                    return false;
                }
                if distance_x.abs() / w1.min(w2) < 0.3 {
                    return false;
                }
            }
        }

        if font_ratio > font_size_tol || font_ratio.recip() > font_size_tol {
            return false;
        }
        if cos_vec.abs() < 0.866 {
            return false;
        }
    }

    block.lines.extend(other.lines.iter().copied());
    block.direction_vec = [
        block.direction_vec[0] + other.direction_vec[0],
        block.direction_vec[1] + other.direction_vec[1],
    ];
    block.direction_norm = vector_norm(block.direction_vec).max(1.0);
    block.angle_deg = block.direction_vec[1]
        .atan2(block.direction_vec[0])
        .to_degrees();
    if block.source_direction == TextDirection::Vertical {
        block.angle_deg -= 90.0;
    }
    block.distances.extend(other.distances.iter().copied());
    block.detected_font_size_px = font_avg.max(1.0);
    block.confidence = block.confidence.max(other.confidence);
    other.merged = true;
    true
}

fn sort_regions(mut blocks: Vec<CtdBlock>) -> Vec<CtdBlock> {
    if blocks.len() < 2 {
        return blocks;
    }

    let vertical_blocks = blocks
        .iter()
        .filter(|block| block.source_direction == TextDirection::Vertical)
        .count();
    let right_to_left = !blocks.is_empty() && vertical_blocks * 2 >= blocks.len();
    blocks.sort_by(|a, b| compare_blocks_for_reading_order(a, b, right_to_left));
    blocks
}

fn compare_blocks_for_reading_order(
    a: &CtdBlock,
    b: &CtdBlock,
    right_to_left: bool,
) -> std::cmp::Ordering {
    let primary = if right_to_left {
        b.bbox[2].total_cmp(&a.bbox[2])
    } else {
        a.bbox[0].total_cmp(&b.bbox[0])
    };
    let tertiary = if right_to_left {
        b.bbox[0].total_cmp(&a.bbox[0])
    } else {
        a.bbox[2].total_cmp(&b.bbox[2])
    };

    primary
        .then_with(|| a.bbox[1].total_cmp(&b.bbox[1]))
        .then_with(|| tertiary)
        .then_with(|| a.bbox[3].total_cmp(&b.bbox[3]))
}

fn dedupe_blocks(blocks: &mut Vec<CtdBlock>) {
    if blocks.len() < 2 {
        return;
    }

    let mut deduped = vec![blocks[0].clone()];
    for block in blocks.iter().skip(1) {
        let area = bbox_area(&block.bbox).max(1e-6);
        let mut keep = true;
        for existing in &deduped {
            let intersection = overlap_area(&block.bbox, &existing.bbox);
            if intersection / area > 0.9 {
                keep = false;
                break;
            }
        }
        if keep {
            deduped.push(block.clone());
        }
    }
    *blocks = deduped;
}

fn refine_mask(image: &RgbImage, pred_mask: &GrayImage, blocks: &[TextBlock]) -> GrayImage {
    let mut refined = GrayImage::new(pred_mask.width(), pred_mask.height());
    for block in blocks {
        let bbox = [
            block.x,
            block.y,
            block.x + block.width,
            block.y + block.height,
        ];
        let [x1f, y1f, x2f, y2f] =
            enlarge_window(bbox, image.width() as f32, image.height() as f32);
        if x2f <= x1f || y2f <= y1f {
            continue;
        }

        let x1 = x1f as u32;
        let y1 = y1f as u32;
        let width = x2f as u32 - x1;
        let height = y2f as u32 - y1;
        let rgb_crop = imageops::crop_imm(image, x1, y1, width, height).to_image();
        let mask_crop = imageops::crop_imm(pred_mask, x1, y1, width, height).to_image();
        let mut candidates = topk_mask_candidates(&rgb_crop, &mask_crop);
        candidates.extend(otsu_mask_candidates(&rgb_crop, &mask_crop));
        let merged = merge_mask_candidates(candidates, &mask_crop);

        for local_y in 0..height {
            for local_x in 0..width {
                if merged.get_pixel(local_x, local_y)[0] > 0 {
                    refined.put_pixel(x1 + local_x, y1 + local_y, Luma([255]));
                }
            }
        }
    }
    refined
}

fn topk_mask_candidates(image: &RgbImage, pred_mask: &GrayImage) -> Vec<CandidateMask> {
    let eroded = erode(pred_mask, Norm::LInf, 1);
    let gray = DynamicImage::ImageRgb8(image.clone())
        .grayscale()
        .to_luma8();
    let mut histogram = [0u32; 256];
    let mut total = 0u32;
    for (pixel, mask_pixel) in gray.pixels().zip(eroded.pixels()) {
        if mask_pixel[0] > 127 {
            histogram[pixel[0] as usize] += 1;
            total += 1;
        }
    }
    if total == 0 {
        return Vec::new();
    }

    let mut colors: Vec<(u8, u32)> = histogram
        .iter()
        .enumerate()
        .filter_map(|(index, count)| {
            if *count > 0 {
                Some((index as u8, *count))
            } else {
                None
            }
        })
        .collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));

    let mut top_colors = Vec::new();
    let bin_tol = (total as f32 * 0.001).ceil() as u32;
    for (color, count) in colors {
        if top_colors
            .iter()
            .all(|existing: &u8| existing.abs_diff(color) > 10)
        {
            top_colors.push(color);
        }
        if top_colors.len() >= 3 || count < bin_tol {
            break;
        }
    }

    top_colors
        .into_iter()
        .map(|color| {
            let top = color.saturating_add(30);
            let bottom = color.saturating_sub(30);
            let thresholded = GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
                let value = gray.get_pixel(x, y)[0];
                if value >= bottom && value <= top {
                    Luma([255u8])
                } else {
                    Luma([0u8])
                }
            });
            minxor_threshold(thresholded, pred_mask)
        })
        .collect()
}

fn otsu_mask_candidates(image: &RgbImage, pred_mask: &GrayImage) -> Vec<CandidateMask> {
    let mut candidates = Vec::new();
    for channel in 0..3 {
        let channel_image = GrayImage::from_fn(image.width(), image.height(), |x, y| {
            Luma([image.get_pixel(x, y)[channel]])
        });
        let level = otsu_level(&channel_image);
        let thresholded =
            GrayImage::from_fn(channel_image.width(), channel_image.height(), |x, y| {
                if channel_image.get_pixel(x, y)[0] > level {
                    Luma([255u8])
                } else {
                    Luma([0u8])
                }
            });
        candidates.push(minxor_threshold(thresholded, pred_mask));
    }
    candidates.sort_by(|a, b| a.xor_sum.cmp(&b.xor_sum));
    candidates.into_iter().take(1).collect()
}

fn minxor_threshold(thresholded: GrayImage, pred_mask: &GrayImage) -> CandidateMask {
    let inverted = invert_binary(&thresholded);
    let regular_xor = xor_sum(&thresholded, pred_mask);
    let inverted_xor = xor_sum(&inverted, pred_mask);
    if inverted_xor < regular_xor {
        CandidateMask {
            mask: inverted,
            xor_sum: inverted_xor,
        }
    } else {
        CandidateMask {
            mask: thresholded,
            xor_sum: regular_xor,
        }
    }
}

fn merge_mask_candidates(mut candidates: Vec<CandidateMask>, pred_mask: &GrayImage) -> GrayImage {
    candidates.sort_by(|a, b| a.xor_sum.cmp(&b.xor_sum));
    let mut mask_merged = GrayImage::new(pred_mask.width(), pred_mask.height());
    let pred = threshold_binary(&erode(pred_mask, Norm::LInf, 1), 60);

    for candidate in candidates {
        let components = connected_components_stats(&candidate.mask, Connectivity::Eight);
        for component in components {
            if component.w * component.h < 3 {
                continue;
            }

            let current = imageops::crop_imm(
                &mask_merged,
                component.x,
                component.y,
                component.w,
                component.h,
            )
            .to_image();
            let mut combined = current.clone();
            for (x, y) in &component.pixels {
                combined.put_pixel(*x - component.x, *y - component.y, Luma([255]));
            }

            let pred_crop =
                imageops::crop_imm(&pred, component.x, component.y, component.w, component.h)
                    .to_image();
            if xor_sum(&combined, &pred_crop) < xor_sum(&current, &pred_crop) {
                for local_y in 0..component.h {
                    for local_x in 0..component.w {
                        let pixel = combined.get_pixel(local_x, local_y);
                        if pixel[0] > 0 {
                            mask_merged.put_pixel(
                                component.x + local_x,
                                component.y + local_y,
                                *pixel,
                            );
                        }
                    }
                }
            }
        }
    }

    let mut mask_merged = dilate(&mask_merged, Norm::LInf, 2);
    let inverted = invert_binary(&mask_merged);
    let holes = connected_components_stats(&inverted, Connectivity::Eight);
    if !holes.is_empty() {
        let mut areas: Vec<u32> = holes.iter().map(|component| component.area).collect();
        areas.sort_unstable();
        let area_threshold = if areas.len() > 1 {
            areas[areas.len() - 2]
        } else {
            areas[0]
        };

        for component in holes {
            if component.area >= area_threshold {
                continue;
            }

            let current = imageops::crop_imm(
                &mask_merged,
                component.x,
                component.y,
                component.w,
                component.h,
            )
            .to_image();
            let mut combined = current.clone();
            for (x, y) in &component.pixels {
                combined.put_pixel(*x - component.x, *y - component.y, Luma([255]));
            }
            let pred_crop =
                imageops::crop_imm(&pred, component.x, component.y, component.w, component.h)
                    .to_image();
            if xor_sum(&combined, &pred_crop) < xor_sum(&current, &pred_crop) {
                for local_y in 0..component.h {
                    for local_x in 0..component.w {
                        let pixel = combined.get_pixel(local_x, local_y);
                        if pixel[0] > 0 {
                            mask_merged.put_pixel(
                                component.x + local_x,
                                component.y + local_y,
                                *pixel,
                            );
                        }
                    }
                }
            }
        }
    }

    mask_merged
}

fn connected_components_stats(image: &GrayImage, connectivity: Connectivity) -> Vec<Component> {
    let labels = connected_components(image, connectivity, Luma([0u8]));
    let max_label = labels.pixels().map(|pixel| pixel[0]).max().unwrap_or(0);
    let mut components = vec![
        Component {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            area: 0,
            pixels: Vec::new(),
        };
        (max_label + 1) as usize
    ];

    for component in components.iter_mut().skip(1) {
        component.x = u32::MAX;
        component.y = u32::MAX;
    }

    for y in 0..labels.height() {
        for x in 0..labels.width() {
            let label = labels.get_pixel(x, y)[0];
            if label == 0 {
                continue;
            }
            let component = &mut components[label as usize];
            component.area += 1;
            component.x = component.x.min(x);
            component.y = component.y.min(y);
            component.w = component.w.max(x);
            component.h = component.h.max(y);
            component.pixels.push((x, y));
        }
    }

    for component in components.iter_mut().skip(1) {
        if component.pixels.is_empty() {
            continue;
        }
        component.w = component.w.saturating_sub(component.x) + 1;
        component.h = component.h.saturating_sub(component.y) + 1;
    }

    components
        .into_iter()
        .skip(1)
        .filter(|component| component.area > 0)
        .collect()
}

fn quad_midpoints(quad: &Quad) -> Quad {
    [
        midpoint(quad[0], quad[1]),
        midpoint(quad[1], quad[2]),
        midpoint(quad[2], quad[3]),
        midpoint(quad[3], quad[0]),
    ]
}

fn quad_axis_lengths(quad: &Quad) -> (f32, f32) {
    let midpoints = quad_midpoints(quad);
    let vec_v = [
        midpoints[2][0] - midpoints[0][0],
        midpoints[2][1] - midpoints[0][1],
    ];
    let vec_h = [
        midpoints[1][0] - midpoints[3][0],
        midpoints[1][1] - midpoints[3][1],
    ];
    (vector_norm(vec_v), vector_norm(vec_h))
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

fn quad_to_tuples(quad: &Quad) -> [(f32, f32); 4] {
    [
        (quad[0][0], quad[0][1]),
        (quad[1][0], quad[1][1]),
        (quad[2][0], quad[2][1]),
        (quad[3][0], quad[3][1]),
    ]
}

fn reorder_quad_horizontal(quad: &Quad) -> Quad {
    let mut points = *quad;
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    let mut left = [points[0], points[1]];
    let mut right = [points[2], points[3]];
    left.sort_by(|a, b| a[1].total_cmp(&b[1]));
    right.sort_by(|a, b| a[1].total_cmp(&b[1]));
    [left[0], right[0], right[1], left[1]]
}

fn reorder_quad_vertical(quad: &Quad) -> Quad {
    let mut points = *quad;
    points.sort_by(|a, b| a[1].total_cmp(&b[1]).then_with(|| a[0].total_cmp(&b[0])));
    let mut top = [points[0], points[1]];
    let mut bottom = [points[2], points[3]];
    top.sort_by(|a, b| a[0].total_cmp(&b[0]));
    bottom.sort_by(|a, b| b[0].total_cmp(&a[0]));
    [top[0], top[1], bottom[0], bottom[1]]
}

fn sort_quad_points(quad: &Quad) -> (Quad, bool) {
    let mut pairwise = Vec::with_capacity(16);
    for a in quad {
        for b in quad {
            let vec = [a[0] - b[0], a[1] - b[1]];
            pairwise.push((vec, vector_norm(vec)));
        }
    }

    let mut sorted_ids: Vec<usize> = (0..pairwise.len()).collect();
    sorted_ids.sort_by(|a, b| pairwise[*a].1.total_cmp(&pairwise[*b].1));
    let mut long_side_vecs = [pairwise[sorted_ids[8]].0, pairwise[sorted_ids[10]].0];
    if dot(long_side_vecs[0], long_side_vecs[1]) < 0.0 {
        long_side_vecs[0] = [-long_side_vecs[0][0], -long_side_vecs[0][1]];
    }

    let structure_vec = [
        (long_side_vecs[0][0] + long_side_vecs[1][0]).abs() * 0.5,
        (long_side_vecs[0][1] + long_side_vecs[1][1]).abs() * 0.5,
    ];
    let sorted_norms: Vec<f32> = sorted_ids.iter().map(|id| pairwise[*id].1).collect();
    let square = sorted_norms[4..12]
        .iter()
        .copied()
        .fold((f32::MAX, f32::MIN), |(min_v, max_v), value| {
            (min_v.min(value), max_v.max(value))
        });
    let mut vertical = structure_vec[0] * 1.2 <= structure_vec[1];
    if square.1 - square.0 < 1e-3 {
        vertical = false;
    }

    if vertical {
        (reorder_quad_vertical(quad), true)
    } else {
        (reorder_quad_horizontal(quad), false)
    }
}

fn clip_quad(quad: &Quad, width: f32, height: f32) -> Quad {
    let mut clipped = *quad;
    for point in &mut clipped {
        point[0] = point[0].clamp(0.0, width);
        point[1] = point[1].clamp(0.0, height);
    }
    clipped
}

fn scale_quad(quad: &Quad, scale_x: f32, scale_y: f32) -> Quad {
    let mut scaled = *quad;
    for point in &mut scaled {
        point[0] *= scale_x;
        point[1] *= scale_y;
    }
    scaled
}

fn quad_bbox(quad: &Quad) -> [f32; 4] {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for point in quad {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

fn bbox_area(bbox: &[f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn overlap_area(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    if x2 <= x1 || y2 <= y1 {
        return 0.0;
    }
    (x2 - x1) * (y2 - y1)
}

fn horizontal_overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    (a[2].min(b[2]) - a[0].max(b[0])).max(0.0)
}

fn mean_mask_score(mask: &GrayImage, bbox: &[f32; 4]) -> f32 {
    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().min(mask.width() as f32) as u32;
    let y2 = bbox[3].ceil().min(mask.height() as f32) as u32;
    if x2 <= x1 || y2 <= y1 {
        return 0.0;
    }

    let mut sum = 0u64;
    let mut count = 0u64;
    for y in y1..y2 {
        for x in x1..x2 {
            sum += mask.get_pixel(x, y)[0] as u64;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum as f32 / count as f32) / 255.0
    }
}

fn enlarge_window(bbox: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    let w = bbox[2] - bbox[0];
    let h = bbox[3] - bbox[1];
    if w <= 0.0 || h <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }

    let a = 1.0f32;
    let b = w + h;
    let c = (1.0 - 2.5) * w * h;
    let delta = (b * b - 4.0 * a * c).max(0.0).sqrt();
    let grow = ((-b + delta) / (2.0 * a)).max(0.0) * 0.5;
    let grow_x = grow.min(bbox[0]).min(image_width - bbox[2]);
    let grow_y = grow.min(bbox[1]).min(image_height - bbox[3]);

    [
        (bbox[0] - grow_x).clamp(0.0, image_width),
        (bbox[1] - grow_y).clamp(0.0, image_height),
        (bbox[2] + grow_x).clamp(0.0, image_width),
        (bbox[3] + grow_y).clamp(0.0, image_height),
    ]
}

fn invert_binary(image: &GrayImage) -> GrayImage {
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        if image.get_pixel(x, y)[0] > 0 {
            Luma([0u8])
        } else {
            Luma([255u8])
        }
    })
}

fn threshold_binary(image: &GrayImage, threshold: u8) -> GrayImage {
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        if image.get_pixel(x, y)[0] > threshold {
            Luma([255u8])
        } else {
            Luma([0u8])
        }
    })
}

fn xor_sum(a: &GrayImage, b: &GrayImage) -> u64 {
    a.pixels()
        .zip(b.pixels())
        .map(|(left, right)| (left[0] ^ right[0]) as u64)
        .sum()
}

fn quads_intersect(a: &Quad, b: &Quad) -> bool {
    let mut axes = Vec::with_capacity(8);
    axes.extend(quad_axes(a));
    axes.extend(quad_axes(b));

    for axis in axes {
        let (a_min, a_max) = project_quad(a, axis);
        let (b_min, b_max) = project_quad(b, axis);
        if a_max < b_min || b_max < a_min {
            return false;
        }
    }
    true
}

fn quad_axes(quad: &Quad) -> Vec<[f32; 2]> {
    let mut axes = Vec::with_capacity(4);
    for index in 0..4 {
        let next = (index + 1) % 4;
        let edge = [
            quad[next][0] - quad[index][0],
            quad[next][1] - quad[index][1],
        ];
        let normal = [-edge[1], edge[0]];
        let norm = vector_norm(normal);
        if norm > 0.0 {
            axes.push([normal[0] / norm, normal[1] / norm]);
        }
    }
    axes
}

fn project_quad(quad: &Quad, axis: [f32; 2]) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for point in quad {
        let projection = point[0] * axis[0] + point[1] * axis[1];
        min = min.min(projection);
        max = max.max(projection);
    }
    (min, max)
}

fn perpendicular_distance(vector: [f32; 2], axis: [f32; 2], _unused: f32) -> f32 {
    let axis_norm = vector_norm(axis).max(1e-6);
    let dot = vector[0] * axis[0] + vector[1] * axis[1];
    let vector_norm = vector_norm(vector).max(1e-6);
    let cos = (dot / (vector_norm * axis_norm)).clamp(-1.0, 1.0);
    (1.0 - cos * cos).max(0.0).sqrt() * vector_norm
}

fn vector_norm(vector: [f32; 2]) -> f32 {
    (vector[0] * vector[0] + vector[1] * vector[1]).sqrt()
}

fn cross_2d(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

trait BboxExt {
    fn bbox_width(&self) -> f32;
    fn bbox_height(&self) -> f32;
}

impl BboxExt for CtdBlock {
    fn bbox_width(&self) -> f32 {
        self.bbox[2] - self.bbox[0]
    }

    fn bbox_height(&self) -> f32 {
        self.bbox[3] - self.bbox[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_block(bbox: [f32; 4], source_direction: TextDirection) -> CtdBlock {
        CtdBlock {
            bbox,
            confidence: 0.9,
            source_language: "unknown".to_string(),
            source_direction,
            lines: Vec::new(),
            angle_deg: 0.0,
            detected_font_size_px: 10.0,
            distances: Vec::new(),
            direction_vec: [1.0, 0.0],
            direction_norm: 1.0,
            merged: false,
        }
    }

    #[test]
    fn component_quad_tracks_orientation() {
        let component = Component {
            x: 0,
            y: 0,
            w: 30,
            h: 8,
            area: 40,
            pixels: (0..20).map(|x| (x, 2 + (x / 5))).collect(),
        };

        let (quad, vertical) = component_quad(&component).expect("quad");
        assert!(!vertical);
        let bbox = quad_bbox(&quad);
        assert!(bbox[2] > bbox[0]);
        assert!(bbox[3] > bbox[1]);
    }

    #[test]
    fn merge_text_lines_keeps_adjacent_horizontal_lines() {
        let line_a = DetectedLine {
            quad: [[0.0, 0.0], [20.0, 0.0], [20.0, 8.0], [0.0, 8.0]],
            vertical: false,
            score: 0.9,
        };
        let line_b = DetectedLine {
            quad: [[18.0, 0.0], [38.0, 0.0], [38.0, 8.0], [18.0, 8.0]],
            vertical: false,
            score: 0.9,
        };
        let mut block_a = CtdBlock::from_line(&line_a);
        let mut block_b = CtdBlock::from_line(&line_b);
        examine_block(&mut block_a, 100, 100, true);
        examine_block(&mut block_b, 100, 100, true);

        assert!(try_merge_text_line(&mut block_a, &mut block_b, 2.0));
        assert_eq!(block_a.lines.len(), 2);
    }

    #[test]
    fn transformed_regions_fall_back_to_bbox_without_ctd_metadata() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([255, 255, 255])));
        let block = TextBlock {
            x: 4.0,
            y: 6.0,
            width: 10.0,
            height: 12.0,
            ..Default::default()
        };

        let regions = extract_text_block_regions(&image, &block);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].width(), 10);
        assert_eq!(regions[0].height(), 12);
    }

    #[test]
    fn crop_text_block_bbox_expands_ctd_crop() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(48, 48, Rgb([255, 255, 255])));
        let block = TextBlock {
            x: 10.0,
            y: 12.0,
            width: 12.0,
            height: 8.0,
            line_polygons: Some(vec![[
                [10.0, 12.0],
                [22.0, 12.0],
                [22.0, 20.0],
                [10.0, 20.0],
            ]]),
            source_direction: Some(TextDirection::Horizontal),
            rotation_deg: Some(0.0),
            detected_font_size_px: Some(8.0),
            detector: Some("ctd".to_string()),
            ..Default::default()
        };

        let crop = crop_text_block_bbox(&image, &block);
        assert!(crop.width() > 12);
        assert!(crop.height() > 8);
    }

    #[test]
    fn group_output_builds_line_only_ctd_blocks() {
        let line = DetectedLine {
            quad: [[10.0, 10.0], [30.0, 10.0], [30.0, 18.0], [10.0, 18.0]],
            vertical: false,
            score: 0.95,
        };
        let mask = GrayImage::from_fn(48, 48, |x, y| {
            if (10..30).contains(&x) && (10..18).contains(&y) {
                Luma([255])
            } else {
                Luma([0])
            }
        });

        let blocks = group_output(&[line], &mask, 48, 48);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].detector.as_deref(), Some("ctd"));
        assert_eq!(blocks[0].source_direction, Some(TextDirection::Horizontal));
        assert_eq!(blocks[0].line_polygons.as_ref().map(Vec::len), Some(1));
        assert!(blocks[0].detected_font_size_px.unwrap_or_default() > 0.0);
    }

    #[test]
    fn transformed_regions_rotate_vertical_ctd_lines() {
        let mut image = RgbImage::from_pixel(48, 48, Rgb([255, 255, 255]));
        for y in 8..40 {
            for x in 20..28 {
                image.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let block = TextBlock {
            x: 18.0,
            y: 8.0,
            width: 12.0,
            height: 32.0,
            line_polygons: Some(vec![[[20.0, 8.0], [28.0, 8.0], [28.0, 40.0], [20.0, 40.0]]]),
            source_direction: Some(TextDirection::Vertical),
            rotation_deg: Some(0.0),
            detected_font_size_px: Some(8.0),
            detector: Some("ctd".to_string()),
            ..Default::default()
        };

        let regions = extract_text_block_regions(&DynamicImage::ImageRgb8(image), &block);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].width() > regions[0].height());
    }

    #[test]
    fn refine_mask_returns_pixels_for_ctd_blocks() {
        let mut image = RgbImage::from_pixel(32, 32, Rgb([255, 255, 255]));
        let pred_mask = GrayImage::from_fn(32, 32, |x, y| {
            if (10..22).contains(&x) && (12..18).contains(&y) {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        for y in 12..18 {
            for x in 10..22 {
                image.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let block = TextBlock {
            x: 10.0,
            y: 12.0,
            width: 12.0,
            height: 6.0,
            line_polygons: Some(vec![[
                [10.0, 12.0],
                [22.0, 12.0],
                [22.0, 18.0],
                [10.0, 18.0],
            ]]),
            source_direction: Some(TextDirection::Horizontal),
            rotation_deg: Some(0.0),
            detected_font_size_px: Some(6.0),
            detector: Some("ctd".to_string()),
            ..Default::default()
        };

        let refined = refine_mask(&image, &pred_mask, &[block]);
        assert_eq!(refined.get_pixel(0, 0)[0], 0);
        assert!(refined.get_pixel(16, 15)[0] > 0);
    }

    #[test]
    fn paragraph_merge_joins_stacked_horizontal_blocks() {
        let make_block = |lines: Vec<Quad>| {
            let mut block = CtdBlock {
                bbox: [0.0, 0.0, 0.0, 0.0],
                confidence: 0.9,
                source_language: "unknown".to_string(),
                source_direction: TextDirection::Horizontal,
                lines,
                angle_deg: 0.0,
                detected_font_size_px: 0.0,
                distances: Vec::new(),
                direction_vec: [1.0, 0.0],
                direction_norm: 1.0,
                merged: false,
            };
            block.adjust_bbox(false);
            examine_block(&mut block, 2000, 2000, true);
            block
        };

        let blocks = vec![
            make_block(vec![[
                [10.0, 10.0],
                [110.0, 10.0],
                [110.0, 30.0],
                [10.0, 30.0],
            ]]),
            make_block(vec![[
                [12.0, 42.0],
                [108.0, 42.0],
                [108.0, 60.0],
                [12.0, 60.0],
            ]]),
            make_block(vec![
                [[9.0, 74.0], [112.0, 74.0], [112.0, 94.0], [9.0, 94.0]],
                [[8.0, 106.0], [105.0, 106.0], [105.0, 126.0], [8.0, 126.0]],
            ]),
        ];

        let merged = merge_paragraph_blocks(blocks, 2000, 2000);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].lines.len(), 4);
    }

    #[test]
    fn paragraph_merge_does_not_skip_over_intervening_block() {
        let make_block = |lines: Vec<Quad>| {
            let mut block = CtdBlock {
                bbox: [0.0, 0.0, 0.0, 0.0],
                confidence: 0.9,
                source_language: "unknown".to_string(),
                source_direction: TextDirection::Horizontal,
                lines,
                angle_deg: 0.0,
                detected_font_size_px: 0.0,
                distances: Vec::new(),
                direction_vec: [1.0, 0.0],
                direction_norm: 1.0,
                merged: false,
            };
            block.adjust_bbox(false);
            examine_block(&mut block, 2000, 2000, true);
            block
        };

        let top = make_block(vec![[
            [10.0, 10.0],
            [110.0, 10.0],
            [110.0, 28.0],
            [10.0, 28.0],
        ]]);
        let blocker = make_block(vec![[
            [42.0, 32.0],
            [78.0, 32.0],
            [78.0, 76.0],
            [42.0, 76.0],
        ]]);
        let bottom = make_block(vec![[
            [12.0, 38.0],
            [108.0, 38.0],
            [108.0, 56.0],
            [12.0, 56.0],
        ]]);

        let merged = merge_paragraph_blocks(vec![top, blocker, bottom], 2000, 2000);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn reading_order_comparator_stays_transitive_across_row_boundaries() {
        let left_lower = test_block([0.0, 9.0, 2.0, 11.0], TextDirection::Horizontal);
        let middle = test_block([1.0, 4.0, 3.0, 6.0], TextDirection::Horizontal);
        let right_upper = test_block([2.0, -1.0, 4.0, 1.0], TextDirection::Horizontal);

        assert_eq!(
            compare_blocks_for_reading_order(&left_lower, &middle, false),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_blocks_for_reading_order(&middle, &right_upper, false),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_blocks_for_reading_order(&left_lower, &right_upper, false),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn sort_regions_orders_vertical_blocks_right_to_left_then_top_to_bottom() {
        let sorted = sort_regions(vec![
            test_block([10.0, 20.0, 18.0, 28.0], TextDirection::Vertical),
            test_block([30.0, 15.0, 38.0, 23.0], TextDirection::Vertical),
            test_block([30.0, 5.0, 38.0, 13.0], TextDirection::Vertical),
        ]);
        let bboxes = sorted.iter().map(|block| block.bbox).collect::<Vec<_>>();

        assert_eq!(
            bboxes,
            vec![
                [30.0, 5.0, 38.0, 13.0],
                [30.0, 15.0, 38.0, 23.0],
                [10.0, 20.0, 18.0, 28.0],
            ]
        );
    }
}
