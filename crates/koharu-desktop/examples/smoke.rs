use std::{io::Cursor, time::Duration};

use anyhow::Result;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use koharu_canvas::Camera;
use koharu_desktop::{Application, DesktopContext, Frontend, Options};
use koharu_scene::{PageId, Session};
use serde_json::Value;

const HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    * { box-sizing: border-box; }
    html, body { width: 100%; height: 100%; margin: 0; overflow: hidden; background: transparent; color: #e7e9ee; font: 14px system-ui; }
    body { display: grid; grid-template: 48px 1fr / 190px 1fr 230px; }
    header, aside { background: #171a22; border-color: #303440; border-style: solid; }
    header { grid-column: 1 / 4; border-width: 0 0 1px; display: flex; align-items: center; padding: 0 16px; gap: 12px; }
    aside { padding: 16px; }
    .left { border-width: 0 1px 0 0; }
    .right { border-width: 0 0 0 1px; }
    #viewport { position: relative; background: transparent; }
    h1 { font-size: 15px; margin: 0 auto 0 0; }
    h2 { font-size: 12px; color: #9da4b5; text-transform: uppercase; letter-spacing: .08em; }
    button { color: inherit; background: #292e3a; border: 1px solid #414756; border-radius: 6px; padding: 6px 10px; }
    .swatch { height: 32px; margin: 8px 0; border-radius: 5px; background: #292e3a; }
    #status { color: #89dceb; }
  </style>
</head>
<body>
  <header><h1>Koharu desktop smoke test</h1><span id="status">starting…</span><button id="close">Close</button></header>
  <aside class="left"><h2>Tools</h2><div class="swatch"></div><div class="swatch"></div><div class="swatch"></div></aside>
  <main id="viewport"></main>
  <aside class="right"><h2>Properties</h2><p>The center is transparent DOM. Rust renders the page underneath it.</p></aside>
  <script>
    const viewport = document.querySelector('#viewport');
    function reportViewport() {
      const rect = viewport.getBoundingClientRect();
      window.koharu.send({ type: 'viewport', x: rect.x, y: rect.y, width: rect.width, height: rect.height, dpr: devicePixelRatio });
    }
    new ResizeObserver(reportViewport).observe(viewport);
    window.addEventListener('koharu:event', event => {
      if (event.detail.type === 'status') document.querySelector('#status').textContent = event.detail.payload;
    });
    document.querySelector('#close').addEventListener('click', () => window.koharu.send({ type: 'exit' }));
    window.koharu.send({ type: 'ready', dpr: devicePixelRatio, width: innerWidth, height: innerHeight });
    reportViewport();
  </script>
</body>
</html>"#;

struct Smoke {
    session: Session,
    page: PageId,
    auto_exit: bool,
}

impl Application for Smoke {
    type Event = ();

    fn started(&mut self, desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        desktop.show_page(&self.session, self.page)?;
        Ok(())
    }

    fn ready(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        _dpr: f64,
        _width: f64,
        _height: f64,
    ) -> Result<()> {
        desktop.emit(
            "status",
            format!("{} · WGPU", desktop.gpu().adapter_info().name),
        )?;
        if self.auto_exit {
            let desktop = desktop.handle();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(750));
                let _ = desktop.exit();
            });
        }
        Ok(())
    }

    fn viewport_changed(&mut self, desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        let mut view = desktop.view().clone();
        view.camera = Camera::contain(
            desktop.viewport().size(),
            self.session.page(self.page)?.size,
        );
        desktop.set_view(view);
        Ok(())
    }

    fn message(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        message: Value,
    ) -> Result<()> {
        if message.get("type").and_then(Value::as_str) == Some("exit") {
            desktop.handle().exit()?;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let auto_exit = std::env::args().any(|argument| argument == "--auto-exit");
    let mut session = Session::memory()?;
    let mut commands = session.commands();
    let page = commands.add_page("smoke.png", page_image())?;
    session.apply(commands)?;

    koharu_desktop::run(
        Options {
            frontend: Frontend::Html(HTML.into()),
            ..Options::default()
        },
        Smoke {
            session,
            page,
            auto_exit,
        },
    )
}

fn page_image() -> Vec<u8> {
    let image = RgbaImage::from_fn(720, 960, |x, y| {
        let paper = 235_u8.saturating_add(((x / 24 + y / 24) % 2) as u8 * 10);
        let accent = (x > 90 && x < 630 && y > 120 && y < 840) as u8 * 18;
        Rgba([
            paper.saturating_sub(accent),
            paper.saturating_sub(accent / 2),
            paper,
            255,
        ])
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("encoding an in-memory image cannot fail");
    bytes.into_inner()
}
