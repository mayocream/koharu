use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::{Parser, ValueHint};
use koharu_ml::cuda_is_available;
use koharu_runtime::{ensure_dylibs, preload_dylibs};
use once_cell::sync::Lazy;
use rfd::MessageDialog;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;
use tracing::warn;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::{
    command,
    khr::{deserialize_khr, has_khr_magic},
    llm, ml,
    renderer::Renderer,
    state::{Document, State},
    update,
};

#[cfg(not(target_os = "windows"))]
fn resolve_app_root() -> PathBuf {
    dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .unwrap_or(PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn resolve_app_root() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(parent_dir) = exe_dir.as_ref().and_then(|dir| dir.parent())
        && parent_dir.join(".portable").is_file()
    {
        return parent_dir.to_path_buf();
    }

    dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .or(exe_dir)
        .unwrap_or(PathBuf::from("."))
}

#[cfg(target_os = "windows")]
mod windows_ansi {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_PROCESSING;
    use windows::Win32::System::Console::GetConsoleMode;
    use windows::Win32::System::Console::GetStdHandle;
    use windows::Win32::System::Console::STD_OUTPUT_HANDLE;
    use windows::Win32::System::Console::SetConsoleMode;
    use windows::core::Result;

    pub fn enable_ansi_support() -> Result<()> {
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE)?;
            if handle == HANDLE::default() {
                println!("Failed to get console handle");
                return Ok(());
            }

            let mut mode = std::mem::zeroed();
            GetConsoleMode(handle, &mut mode)?;
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_file_assoc {
    use anyhow::Result;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const CLASS_NAME: &str = "Koharu.khr";
    // const THUMBNAIL_PROVIDER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

    pub fn register_khr() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu.create_subkey("Software\\Classes")?.0;

        let (ext_key, _) = classes.create_subkey(".khr")?;
        ext_key.set_value("", &CLASS_NAME)?;
        ext_key.set_value("Content Type", &"image/jpeg")?;
        ext_key.set_value("PerceivedType", &"image")?;
        // let (ext_thumb, _) = ext_key.create_subkey(format!("ShellEx\\{THUMBNAIL_PROVIDER}"))?;
        // ext_thumb.set_value("", &THUMBNAIL_PROVIDER)?;

        let (class_key, _) = classes.create_subkey(CLASS_NAME)?;
        class_key.set_value("", &"Koharu Document")?;
        // let (thumb_key, _) = class_key.create_subkey(format!("ShellEx\\{THUMBNAIL_PROVIDER}"))?;
        // thumb_key.set_value("", &THUMBNAIL_PROVIDER)?;

        if let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_owned()))
        {
            let (icon_key, _) = class_key.create_subkey("DefaultIcon")?;
            icon_key.set_value("", &format!("{exe},0"))?;
        }
        // add default open with
        let (shell_key, _) = class_key.create_subkey("shell\\open\\command")?;
        if let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_owned()))
        {
            shell_key.set_value("", &format!("\"{exe}\" \"%1\""))?;
        }

        Ok(())
    }
}

static APP_ROOT: Lazy<PathBuf> = Lazy::new(resolve_app_root);
static LIB_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("libs"));
static MODEL_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("models"));

#[derive(Parser)]
#[command(version = crate::version::APP_VERSION, about)]
struct Cli {
    #[arg(
        short,
        long,
        help = "Download dynamic libraries and exit",
        default_value_t = false
    )]
    download: bool,
    #[arg(
        long,
        help = "Force using CPU even if GPU is available",
        default_value_t = false
    )]
    cpu: bool,
    #[arg(
        value_name = "PATH",
        value_hint = ValueHint::FilePath,
        help = "Open file on startup"
    )]
    path: Option<PathBuf>,
}

fn load_documents_from_path(path: PathBuf) -> Result<Vec<Document>> {
    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", path.display()));
    }

    let bytes = std::fs::read(&path)?;

    if has_khr_magic(&bytes) {
        return deserialize_khr(&bytes)
            .map_err(|err| anyhow::anyhow!("Failed to load documents: {err}"));
    }

    Ok(vec![Document::open(path)?])
}

fn initialize() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows_ansi::enable_ansi_support().ok();
    }

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    // hook model cache dir
    koharu_ml::set_cache_dir(MODEL_ROOT.to_path_buf())?;

    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Panic")
            .set_description(&msg)
            .show();
        std::process::exit(1);
    }));

    #[cfg(feature = "bundle")]
    {
        // https://docs.velopack.io/integrating/overview#application-startup
        velopack::VelopackApp::build().run();
    }

    Ok(())
}

async fn prefetch() -> Result<()> {
    ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
    ml::prefetch().await?;
    // Skip for now as it's too big
    // llm::prefetch().await?;

    Ok(())
}

async fn setup(
    app: tauri::AppHandle,
    use_cpu: bool,
    startup_document: Option<PathBuf>,
) -> Result<()> {
    // Preload dynamic libraries only if CUDA is available.
    if cuda_is_available() {
        ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
        preload_dylibs(LIB_ROOT.to_path_buf())?;

        // Only search DLLs in the custom directory on Windows, this avoids potential
        // conflicts with existing DLLs in the system PATH.
        #[cfg(target_os = "windows")]
        {
            if let Err(err) = windows_file_assoc::register_khr() {
                warn!(?err, "Failed to register .khr file association");
            }

            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::System::LibraryLoader::{
                AddDllDirectory, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS,
                SetDefaultDllDirectories,
            };

            let wide = LIB_ROOT
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            unsafe {
                if SetDefaultDllDirectories(
                    LOAD_LIBRARY_SEARCH_USER_DIRS | LOAD_LIBRARY_SEARCH_SYSTEM32,
                ) == 0
                {
                    anyhow::bail!(
                        "Failed to set default DLL directories: {}",
                        std::io::Error::last_os_error()
                    );
                }
                if AddDllDirectory(wide.as_ptr()).is_null() {
                    anyhow::bail!(
                        "Failed to add DLL directory: {}",
                        std::io::Error::last_os_error()
                    );
                }
            }
        }

        tracing::info!(
            "CUDA is available, loaded dynamic libraries from {:?}",
            *LIB_ROOT
        );
    }

    let ml = Arc::new(ml::Model::new(use_cpu).await?);
    let llm = Arc::new(llm::Model::new(use_cpu));
    let renderer = Arc::new(Renderer::new()?);
    let state = Arc::new(RwLock::new(State::default()));

    app.manage(ml);
    app.manage(llm);
    app.manage(renderer);

    app.get_webview_window("splashscreen").unwrap().close()?;
    let main_window = app.get_webview_window("main").unwrap();
    main_window.show()?;

    if let Some(path) = startup_document {
        match load_documents_from_path(path) {
            Ok(documents) => {
                {
                    let mut guard = state.write().await;
                    guard.documents = documents.clone();
                }
                if let Err(err) = main_window.emit("documents:opened", &documents) {
                    warn!(?err, "Failed to emit documents:opened event");
                }
            }
            Err(err) => {
                warn!(?err, "Failed to open startup document");
                MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("Failed to open file")
                    .set_description(format!("{err:#}"))
                    .show();
            }
        }
    }

    app.manage(state);

    Ok(())
}

pub async fn run() -> Result<()> {
    initialize()?;

    let Cli {
        download,
        cpu,
        path,
    } = Cli::parse();

    if download {
        prefetch().await?;
        return Ok(());
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            command::app_version,
            command::open_external,
            command::get_documents,
            command::open_documents,
            command::save_documents,
            command::export_document,
            command::export_all_documents,
            command::detect,
            command::ocr,
            command::inpaint,
            command::inpaint_partial,
            command::render,
            command::update_brush_layer,
            command::update_text_blocks,
            command::update_inpaint_mask,
            command::list_font_families,
            command::llm_list,
            command::llm_load,
            command::llm_offload,
            command::llm_ready,
            command::llm_generate,
            update::apply_available_update,
            update::get_available_update,
            update::ignore_update,
        ])
        .setup(move |app| {
            app.manage(update::UpdateState::new(APP_ROOT.to_path_buf()));
            update::spawn_background_update_check(app.handle().clone());

            let handle = app.handle().clone();
            let startup_path = path.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = setup(handle, cpu, startup_path).await {
                    panic!("application setup failed: {err:#}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
