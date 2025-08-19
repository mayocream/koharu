use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct AppState {
    pub comic_text_detector: Mutex<ComicTextDetector>,
    pub manga_ocr: Mutex<MangaOCR>,
    pub lama: Mutex<Lama>,
}
