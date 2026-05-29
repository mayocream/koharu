const CONTAINMENT_DUPLICATE_THRESHOLD: f32 = 0.85;
const IOU_DUPLICATE_THRESHOLD: f32 = 0.5;

pub(crate) fn bbox_area(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

pub(crate) fn bbox_overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let width = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let height = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    width * height
}

pub(crate) fn bbox_is_duplicate(a: [f32; 4], b: [f32; 4]) -> bool {
    let area_a = bbox_area(a);
    let area_b = bbox_area(b);
    if area_a <= 0.0 || area_b <= 0.0 {
        return false;
    }

    let overlap = bbox_overlap(a, b);
    overlap / area_a.min(area_b) >= CONTAINMENT_DUPLICATE_THRESHOLD
        || overlap / (area_a + area_b - overlap).max(1.0) >= IOU_DUPLICATE_THRESHOLD
}
