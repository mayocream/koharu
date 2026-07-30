//! Real-GPU tests for the complete canvas pipeline.
//!
//! CPU tests should cover exact geometry and state transitions. These tests
//! instead render into the same offscreen texture used by the desktop host,
//! read it back, and make tolerant pixel assertions. That catches integration
//! mistakes between scene caching, Vello affines, texture composition, and the
//! WGPU overlay pass without relying on brittle whole-image byte equality.

use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

use image::{DynamicImage, ImageFormat, RgbaImage};
use koharu_scene::{Frame, PageAsset, Session};

use crate::{
    Camera, Canvas, CanvasGpu, DisplayState, Guide, Handle, HitTarget, OverlayState, PagePoint,
    PageView, PhysicalPoint, PhysicalSize, ViewState,
};

const VIEWPORT: PhysicalSize = PhysicalSize::new(64, 48);

fn rgba_png(size: (u32, u32), color: [u8; 4]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(size.0, size.1, image::Rgba(color)))
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * VIEWPORT.width + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn assert_orange(pixel: [u8; 4]) {
    assert!(
        pixel[0] > 180 && (80..170).contains(&pixel[1]) && pixel[2] < 110,
        "expected orange image content, got {pixel:?}"
    );
}

fn assert_clean_blue(pixel: [u8; 4]) {
    assert!(
        pixel[0] < 150 && pixel[1] > 110 && pixel[2] > 170,
        "expected blue clean-page content, got {pixel:?}"
    );
}

#[test]
#[ignore = "requires an explicitly provisioned WGPU adapter"]
fn renders_move_resize_and_rotate_previews_to_expected_pixels() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (device, queue) = pollster::block_on(async {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .expect("gpu-tests requires a WGPU adapter");
        adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("gpu-tests requires a WGPU device")
    });

    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands
        .add_page("page", rgba_png((16, 12), [21, 34, 55, 255]))
        .unwrap();
    commands
        .set_asset(
            page,
            PageAsset::Clean,
            Some(rgba_png((16, 12), [89, 144, 233, 255])),
        )
        .unwrap();
    session.apply(commands).unwrap();

    // Resource decoding happens on a worker. The wake channel lets the test
    // wait until both source and clean images are ready before rendering.
    let (wake, woke) = std::sync::mpsc::channel();
    let mut canvas = Canvas::new(
        CanvasGpu {
            device: Arc::new(device),
            queue: Arc::new(queue),
        },
        Arc::new(move || {
            let _ = wake.send(());
        }),
    )
    .unwrap();
    canvas.set_view(ViewState {
        size: VIEWPORT,
        camera: Camera::contain(VIEWPORT, session.page(page).unwrap().size),
        display: DisplayState::default(),
    });
    canvas.show_page(&session, page).unwrap();
    for _ in 0..2 {
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    let now = Instant::now();
    let frame = canvas.render(now).unwrap();
    assert_eq!(frame.size, VIEWPORT);
    assert!(frame.generation > 0);
    let generation = frame.generation;
    assert_eq!(
        canvas.render(Instant::now()).unwrap().generation,
        generation
    );

    // Complete the source-to-clean transition using injected Instants rather
    // than sleeping, keeping the rendering result deterministic.
    canvas.set_view(ViewState {
        size: VIEWPORT,
        camera: Camera::contain(VIEWPORT, session.page(page).unwrap().size),
        display: DisplayState {
            page: PageView::EditableClean,
            ..DisplayState::default()
        },
    });
    assert!(canvas.render(now).unwrap().needs_redraw);
    assert!(
        !canvas
            .render(now + Duration::from_millis(181))
            .unwrap()
            .needs_redraw
    );

    let mut edit = session.edit();
    let image = edit
        .page(page)
        .unwrap()
        .add_image(
            Frame::new(2.0, 2.0, 6.0, 4.0),
            "stamp",
            rgba_png((6, 4), [233, 121, 52, 255]),
        )
        .unwrap();
    let changes = edit.commit().unwrap();
    canvas.sync(&session, &changes).unwrap();
    assert_eq!(
        canvas.hit_test(canvas.page_to_screen(PagePoint::new(4.0, 3.0))),
        Some(HitTarget::Element(image))
    );
    woke.recv_timeout(Duration::from_secs(2)).unwrap();
    canvas.render(now + Duration::from_millis(182)).unwrap();

    // Moving must clear an old interior pixel and paint the new center. This
    // verifies the node pixels, not merely its selection outline.
    canvas
        .begin_transform(
            &[image],
            HitTarget::Element(image),
            canvas.page_to_screen(PagePoint::new(4.0, 3.0)),
        )
        .unwrap();
    canvas
        .update_transform(canvas.page_to_screen(PagePoint::new(8.0, 6.0)))
        .unwrap();
    canvas.render(now + Duration::from_millis(183)).unwrap();
    let pixels = canvas.read_output_for_test();
    assert_clean_blue(pixel(&pixels, 16, 12));
    assert_orange(pixel(&pixels, 36, 28));
    let moved = canvas.finish_transform().unwrap().unwrap();
    assert_eq!(moved.elements[0].frame, Frame::new(6.0, 5.0, 6.0, 4.0));

    // Resizing east exposes orange content beyond the committed right edge.
    canvas
        .begin_transform(
            &[image],
            HitTarget::Handle {
                element: image,
                handle: Handle::East,
            },
            canvas.page_to_screen(PagePoint::new(8.0, 4.0)),
        )
        .unwrap();
    canvas
        .update_transform(canvas.page_to_screen(PagePoint::new(11.0, 4.0)))
        .unwrap();
    canvas.render(now + Duration::from_millis(184)).unwrap();
    let pixels = canvas.read_output_for_test();
    assert_orange(pixel(&pixels, 40, 16));
    let resized = canvas.finish_transform().unwrap().unwrap();
    assert_eq!(resized.elements[0].frame, Frame::new(2.0, 2.0, 9.0, 4.0));

    // A 90-degree rotation paints above the original horizontal rectangle and
    // clears a point that was previously inside its left edge.
    canvas
        .begin_transform(
            &[image],
            HitTarget::Handle {
                element: image,
                handle: Handle::Rotate,
            },
            canvas.page_to_screen(PagePoint::new(5.0, 0.0)),
        )
        .unwrap();
    canvas
        .update_transform(canvas.page_to_screen(PagePoint::new(9.0, 4.0)))
        .unwrap();
    canvas.render(now + Duration::from_millis(185)).unwrap();
    let pixels = canvas.read_output_for_test();
    assert_orange(pixel(&pixels, 20, 6));
    assert_clean_blue(pixel(&pixels, 9, 16));
    let rotated = canvas.finish_transform().unwrap().unwrap();
    assert!((rotated.elements[0].frame.angle_degrees - 90.0).abs() < 1e-5);

    canvas.set_overlays(OverlayState {
        guides: vec![Guide::Vertical(4.0)],
        ..OverlayState::default()
    });
    assert!(canvas.render(Instant::now()).unwrap().generation > generation);
    canvas.clear_page();
    assert!(
        canvas
            .screen_to_page(PhysicalPoint::new(1.0, 1.0))
            .is_none()
    );
    assert!(canvas.hit_test(PhysicalPoint::new(1.0, 1.0)).is_none());
}
