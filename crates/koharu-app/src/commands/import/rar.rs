use std::{
    cell::RefCell,
    io::{self, Write},
    path::Path,
    rc::Rc,
};

use anyhow::{Context as _, Result, bail};
use rars::ArchiveReader;

use super::{EncodedPage, Format};

struct PageWriter {
    name: String,
    bytes: Vec<u8>,
    images: Rc<RefCell<Vec<EncodedPage>>>,
}

impl Write for PageWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PageWriter {
    fn drop(&mut self) {
        self.images.borrow_mut().push(EncodedPage {
            name: std::mem::take(&mut self.name),
            bytes: std::mem::take(&mut self.bytes),
        });
    }
}

pub(super) fn extract(path: &Path) -> Result<Vec<EncodedPage>> {
    let archive = ArchiveReader::read_path(path)
        .with_context(|| format!("failed to open RAR archive {}", path.display()))?;
    let images = Rc::new(RefCell::new(Vec::new()));
    archive
        .extract_to(None, |entry| {
            let name = entry.name_lossy();
            let member_path = Path::new(&name);
            let supported = member_path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension.parse(), Ok(Format::Raster)));
            let metadata = member_path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .is_none_or(|file_name| {
                    file_name.starts_with("._")
                        || [".ds_store", "thumbs.db", "desktop.ini"]
                            .iter()
                            .any(|candidate| file_name.eq_ignore_ascii_case(candidate))
                })
                || member_path.components().any(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .is_some_and(|component| component.eq_ignore_ascii_case("__macosx"))
                });
            if entry.is_directory || !supported || metadata {
                Ok(Box::new(io::sink()))
            } else {
                Ok(Box::new(PageWriter {
                    name,
                    bytes: Vec::new(),
                    images: Rc::clone(&images),
                }))
            }
        })
        .with_context(|| format!("failed to extract RAR archive {}", path.display()))?;

    let mut images = std::mem::take(&mut *images.borrow_mut());
    alphanumeric_sort::sort_slice_by_os_str_key(&mut images, |image| {
        std::ffi::OsStr::new(&image.name)
    });
    if images.is_empty() {
        bail!(
            "RAR archive {} contains no supported raster images",
            path.display()
        );
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    use image::ImageFormat;
    use rars::{
        EntrySource, WriterResources,
        rar50::{ArchiveEntry, ArchiveExtras, WriterOptions, write_streaming_archive_to},
    };

    use super::*;

    #[test]
    fn extracts_images_in_natural_order_and_skips_metadata() {
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255]));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, ImageFormat::Png)
            .expect("encode PNG");
        let png = png.into_inner();
        let entries = [
            ArchiveEntry::new(
                "ComicInfo.xml",
                EntrySource::from_bytes(&b"<ComicInfo />"[..]),
            ),
            ArchiveEntry::new("pages/page10.png", EntrySource::from_bytes(png.clone())),
            ArchiveEntry::new("pages/page2.png", EntrySource::from_bytes(png.clone())),
            ArchiveEntry::new("__MACOSX/._page3.png", EntrySource::from_bytes(png)),
        ];
        let mut bytes = Vec::new();
        write_streaming_archive_to(
            &entries,
            WriterOptions::default(),
            ArchiveExtras::default(),
            &WriterResources::default(),
            &mut bytes,
        )
        .expect("encode RAR fixture");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "koharu-import-rar-{}-{timestamp}.rar",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write RAR fixture");

        let images = extract(&path).expect("extract RAR fixture");
        std::fs::remove_file(&path).expect("remove RAR fixture");
        assert_eq!(
            images
                .iter()
                .map(|image| image.name.as_str())
                .collect::<Vec<_>>(),
            ["pages/page2.png", "pages/page10.png"]
        );
    }
}
