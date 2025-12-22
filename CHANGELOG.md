## [0.23.1] - 2025-12-22

### 🐛 Bug Fixes

- Render brush
- Smaller color input button

### ⚙️ Miscellaneous Tasks

- Release 0.23.1
## [0.23.0] - 2025-12-22

### 🚀 Features

- Inpaint brush and erasor
- Smoother image transition
- Color brush and erasor
- Drag and zoom

### 🐛 Bug Fixes

- Clippy happy
- Add non-null assertion for segment
- More greedy partial inpaint for text block change

### ⚡ Performance

- Partial update mask
- Reduce unused partial inpaint

### ⚙️ Miscellaneous Tasks

- Do not cache cargo targets
- Update dependencies
- Align code style
- Release 0.23.0
## [0.22.0] - 2025-12-21

### 🚀 Features

- Add openai endpoint support
- Wgpu renderer (#105)
- Add some text shader styles
- Block font and color

### 🐛 Bug Fixes

- Activity bubble animation and per operation sub title
- Add missing ui/components/ResizableSidebar.tsx
- Integration tests
- Wgpu renderer minor issues

### 💼 Other

- Resizable navigator and panels

### 📚 Documentation

- Cleanup per-crate docs

### 🧪 Testing

- Ignore dylib tests
- Yolo ignore tests

### ⚙️ Miscellaneous Tasks

- Remove unused deppendencies
- Use format mode
- Update dependencies
- Mark rendering integration tests as ignored due to lack of fonts
- Update ui dependencies
- Hide debug window
- Make clippy happy
- Release 0.22.0
## [0.21.0] - 2025-12-19

### 🚀 Features

- Multi lang llm translate

### ⚙️ Miscellaneous Tasks

- Simplify bubble logic
- Release 0.21.0
- Bundled pack portable
## [0.20.0] - 2025-12-18

### 🚀 Features

- Double click or CLI to open a file

### ⚙️ Miscellaneous Tasks

- Release 0.20.0
## [0.19.0] - 2025-12-18

### 🚀 Features

- Add hunyuan to translate more languages
- Wysiwyg canvas
- Save and load projects
- Save and load projects
- Show version number
- .khr file association on windows
- Async update check
- Operation bubble and cancellable batch

### 🐛 Bug Fixes

- Stroke and font color normalization
- Dom text layer bug
- Cargo happy
- Add a textBlockSyncer to avoid block on mutex

### 🚜 Refactor

- New renderer (#99)

### 📚 Documentation

- Change demo image

### ⚡ Performance

- Improve renderer performance

### ⚙️ Miscellaneous Tasks

- Clippy happier
- Use workspace Cargo
- Release 0.19.0
## [0.18.0] - 2025-12-16

### 🚀 Features

- Reimplement load tokenizer from gguf
- Google fonts
- Font detection using yuzumaker.FontDetection
- Auto choose color and stroke
- Renderer to clamp near black

### 🐛 Bug Fixes

- Llm test running locally
- Avoid conflict with existing dlls on windows
- Layout CJK and English at same time in horizontal view
- Windows to search system32 for DLLs too
- Clippy happy
- Close bigger holes in ctd
- Add font detect test
- Remove stroke auto color, keep font auto color

### 🧪 Testing

- Fix tests
- Use google fonts
- Add layout line wrapping test
- Add vertical CJK layout test

### ⚙️ Miscellaneous Tasks

- Update cargo clippy command to treat warnings as errors
- Add ignore annotation for llm test requiring large model downloads
- Remove reviewdog
- Make clippy happy
- Only create span when cache is not valid
- Remove unnessary conditional deps
- Add layout tests
- Remove unused import in koharu-renderer
- Release 0.18.0
## [0.17.0] - 2025-12-14

### 🚀 Features

- Load tokenizer from gguf

### 🐛 Bug Fixes

- Llm template should add generation prompt

### ⚙️ Miscellaneous Tasks

- Release 0.17.0
## [0.16.0] - 2025-12-14

### 🚀 Features

- Add cpu only mode
- Predict word wrap in horizontal layout
- Normalize font sizes

### 🐛 Bug Fixes

- Cpu only mode ctd and model sorting
- Unload llm on change
- Better compatiblity with windows conhost
- Use cfg target_os in Cargo.toml
- Vntl incorrect template

### ⚡ Performance

- Batch inference for manga-ocr
- Speed up metal fft

### 🧪 Testing

- Add ctd integration test

### ⚙️ Miscellaneous Tasks

- Switch to cudarc 0.18.2
- Remove unused deps
- Add linter
- Make clippy happy
- Fix linter
- Make clippy happy again
- Add husky & lint-staged
- Add reviewdog
- Add cargo tests (#89)
- Release 0.16.0
## [0.15.0] - 2025-12-11

### 🐛 Bug Fixes

- Add top level tracing and mark others as debug

### 🚜 Refactor

- Move fft to seperate files

### ⚡ Performance

- Cache cufft plan

### ⚙️ Miscellaneous Tasks

- Format lama code with LF
- Release 0.15.0
## [0.14.5] - 2025-12-11

### 🐛 Bug Fixes

- Use templates from gguf file
- Format code and llm example
- Remove unused deserialize
- Sakura-1.5b-qwen2.5-v1.0 incorrect eos_token_id

### 📚 Documentation

- Update changelog

### ⚙️ Miscellaneous Tasks

- Add release script
- Remove unwrap
- Release 0.14.4
- Release 0.14.5
## [0.14.3] - 2025-12-11

### 🐛 Bug Fixes

- Support non-nvidia gpu on windows

### 💼 Other

- 0.14.3

### 📚 Documentation

- Add changelog.md

### ⚙️ Miscellaneous Tasks

- Add release-please
- *(ci)* Use release-plz
- Do not publish crates
- Remove release-please
## [0.14.2] - 2025-12-11

### 💼 Other

- 0.14.2
## [0.14.1] - 2025-12-10

### 🐛 Bug Fixes

- Macos to use local data dir

### 💼 Other

- 0.14.1
## [0.14.0] - 2025-12-09

### 🚀 Features

- I18n
- Delete and backspace key can delete text blocks
- Translate single textblock

### 🐛 Bug Fixes

- Set progress message is not awaited
- Try to make lfm2 output better results
- Menbar overlay
- Llm idle i18n
- Unable to add textblocks

### 💼 Other

- 0.14.0

### 📚 Documentation

- Fix broken words

### ⚙️ Miscellaneous Tasks

- Add version to cli
## [0.13.1] - 2025-12-09

### 🚀 Features

- Lfm2 iterration 2

### 🐛 Bug Fixes

- Lfm2 remove extra debugging code
- Prefer to use huggingface.co since hf-mirror is unstable

### 💼 Other

- 0.13.1

### 📚 Documentation

- Update readme on llm

### ⚙️ Miscellaneous Tasks

- Cleanup tracing logs
## [0.13.0] - 2025-12-08

### 🚀 Features

- Add task bar progress bar
- Lfm2 iterration 1
- Add experimental smaller model support

### 🐛 Bug Fixes

- Sort llm
- Correct llm loading status

### 💼 Other

- 0.13.0
## [0.12.1] - 2025-12-07

### 🚀 Features

- Add cli mode & download only

### 🐛 Bug Fixes

- Hf hub download also uses cache location

### 💼 Other

- Create bundle archive for windows
- 0.12.1
- Reduce bundled file size
- Upload correct artifact

### 🚜 Refactor

- Re-introduce koharu-core

### ⚙️ Miscellaneous Tasks

- Speed up cuda install
- Install nvcc and upload artifacts
- Add tracing logs for download
- Skip preloading llm models
## [0.12.0] - 2025-12-06

### 🚀 Features

- Select pypi mirror
- Add hf mirror

### 🐛 Bug Fixes

- Pypi mirror
- Refine url
- Portable mode libs and models location

### 💼 Other

- 0.12.0

### ⚙️ Miscellaneous Tasks

- Format code
- Format code
## [0.11.0] - 2025-12-04

### 🚀 Features

- Add cudnn support
- Use cufft
- Metal fft
- Batch process
- Batch show status
- Only inpaint mask inside boxes
- Export and batch export
- Auto detect font family order
- Use local directory if portal

### 🐛 Bug Fixes

- Preprocess and postprocess
- Lama
- Macos
- Metal
- Mask
- Fft precision
- Use gpu to postprocess
- Order llm by locale
- Fallback to Yahei
- UI fixes
- Font size bisearch fix
- Order font by preferred language
- Latin character in CJK text vertical hack
- Adjust vertical layout for small glyphs
- Vertical magic spacing
- Add macOS default fonts and skip non normal variants
- Latin should not in vertical
- Break on whitespace and centeralize text
- Horizontal latin min fontsize limit
- Only compensate on selected axis
- Resize, batch and UI fixes
- Center compensation bug in vertical layout
- Merge error
- More merge errors
- Center compensation should always be positive
- Wheels matching on Linux (#49)

### 💼 Other

- 0.11.0

### 🚜 Refactor

- Use pure FFT for lama
- Yolov5
- Dbnet & unet
- Replace onnx with candle
- Correct ctd behavor
- Replace onnx with candle
- Replace onnx with candle
- Rename models to ml
- Use koharu-ml

### 📚 Documentation

- Update license section in README files
- Add horizontal rule in README for better section separation

### ⚡ Performance

- Avoid cpu offload
- Use GPU to resize and postprocess
- Use GPU to resize
- Add instruments

### ⚙️ Miscellaneous Tasks

- Remove unused deps
- Code style
- Move sys-locale to workspace deps
- Update deps
## [0.10.1] - 2025-11-25

### 🐛 Bug Fixes

- Use onnxruntime-gpu when cuda enabled

### 💼 Other

- 0.10.1
## [0.10.0] - 2025-11-25

### 🚀 Features

- Dynamic loading cuda & onnxruntime

### 💼 Other

- 0.10.0

### 📚 Documentation

- Support metal

### ⚙️ Miscellaneous Tasks

- *(ci)* Only draft release on ci
- Remove profile
- Move update to init phase
## [0.9.13] - 2025-11-24

### 🐛 Bug Fixes

- Release

### 💼 Other

- 0.9.13
## [0.9.12] - 2025-11-24

### 🐛 Bug Fixes

- Ci

### 💼 Other

- 0.9.12
## [0.9.11] - 2025-11-24

### 🚀 Features

- Support metal
- Support coreml

### 🐛 Bug Fixes

- Support macos
- Update macOS platform tag to universal2
- Support macos
- Remove suffix
- Release

### 💼 Other

- 0.9.11

### 📚 Documentation

- Sponsorship
- Fix link

### ⚙️ Miscellaneous Tasks

- *(ci)* Setup macos
## [0.9.10] - 2025-11-23

### 💼 Other

- 0.9.10
## [0.9.9] - 2025-11-23

### 💼 Other

- 0.9.9
## [0.9.9-alpha.2] - 2025-11-23

### 💼 Other

- 0.9.9-alpha.2 (auto-update test)
## [0.9.9-alpha.1] - 2025-11-23

### 🚀 Features

- Auto-update

### 💼 Other

- 0.9.9-alpha.1
## [0.9.8] - 2025-11-23

### 💼 Other

- 0.9.8
## [0.9.8-alpha.1] - 2025-11-23

### 💼 Other

- 0.9.8-alpha.1
## [0.9.7] - 2025-11-23

### 💼 Other

- 0.9.7
## [0.9.6] - 2025-11-22

### 💼 Other

- 0.9.6
## [0.9.5] - 2025-11-22

### 💼 Other

- 0.9.5
## [0.9.4] - 2025-11-22

### 🐛 Bug Fixes

- Bundle cuda at build time

### 💼 Other

- 0.9.4
## [0.9.3] - 2025-11-22

### 🚀 Features

- Support English text rendering

### 💼 Other

- 0.9.3

### 📚 Documentation

- Update README for clarity and consistency in feature descriptions
- Add screenshots

### ⚙️ Miscellaneous Tasks

- UX improvements
## [0.9.2] - 2025-11-22

### 🐛 Bug Fixes

- Add --no-verify flag to cargo publish command in workflow

### 💼 Other

- 0.9.2
## [0.9.1] - 2025-11-22

### 🚀 Features

- Add vntl-llama3-8b-v2 support & remove Gemma3 and Qwen2.5
- Support SakuraLLM/Sakura-GalTransl-7B-v3.7
- Auto-prompt

### 🐛 Bug Fixes

- Correct enum variant casing for SakuraGalTransl7Bv3_7

### 💼 Other

- 0.9.1

### 🚜 Refactor

- Remove system prompt handling from LlmControls and store

### 📚 Documentation

- Update README

### ⚙️ Miscellaneous Tasks

- Publish to crates.io
- *(ci)* Add publish workflow
- Add Arial font to default font families in TextStyle
## [0.9.0] - 2025-11-21

### 🚀 Features

- Initial impl for text block renderer
- Add vertical layout features for text shaping
- Text rendering via Rust
- Initial implement for text redering on ui
- Auto font size

### 🐛 Bug Fixes

- Typo

### 💼 Other

- 0.9.0

### 🚜 Refactor

- Remove unnecessary componetns
- Better stucture
- Rewrite renderer
- Reorganize module imports and update dylib function calls
- Reorganize font module exports and add query method

### 🧪 Testing

- Add test to ensure vertical feature works
- Add vertical text rendering
- Place text to corner

### ⚙️ Miscellaneous Tasks

- Remove unused unicode dependencies from Cargo.toml
- *(scripts)* Auto-detect cuda
- Remove unused dependencies (backon, ndarray, unicode-linebreak, rayon)
- Make clippy happy
- Add cargo script to package.json
## [0.8.0] - 2025-11-12

### 🚀 Features

- Implement core application structure and functionality
- Add tracing events for download events
- Add default capability configuration
- Add update_text_blocks command to modify text blocks in documents
- Enhance TextBlockAnnotations with update functionality and transformer support
- Implement draft block functionality for text selection in Workspace
- Add context menu for text block management in Workspace
- Auto scale
- Update llmSystemPrompt for Japanese→English translation

### 🐛 Bug Fixes

- Update PROGRESS_WINDOW_EVENT URL to match naming convention
- Scroll bar
- Ui layout
- Ui issue

### 💼 Other

- 0.8.0

### 🚜 Refactor

- Simplify model load and offload commands
- Reorganize dylib handling and update HTTP client references
- Renmae koharu-app to koharu
- Use async api
- Remove error logging in setup and replace with panic
- Use stream for http request
- Enhance logging configuration in initialize function
- Improve UX
- Simplify MenuBar component structure
- Streamline Canvas component structure and remove unused functions
- Implement accordion for text blocks in Panels component
- Cleanup ui
- Update features section in README for clarity and consistency
- Use DOM-based canvas

### ⚡ Performance

- Download with http range

### ⚙️ Miscellaneous Tasks

- Make clippy happy
- Clippy style
- Move signing to .github
- Remove koharu_preview script
- Add configuration store for detection and inpainting parameters
- Replace updateBlock with updateTextBlocks for batch updates in TextBlocksPanel
- Update tool modes in Workspace and store, removing 'navigate' and 'text' options
- Rename koharu-app to koharu
- Enable segmentation mask and inpainted image display
## [0.7.6] - 2025-11-07

### 🚀 Features

- Retry on failures

### 💼 Other

- 0.7.6
## [0.7.5] - 2025-11-07

### 🐛 Bug Fixes

- Download while not preload

### 💼 Other

- 0.7.5
## [0.7.4] - 2025-11-07

### 🐛 Bug Fixes

- Hard-coded dylib loading order
- Improve ONNX Runtime dynamic library loading conditions
- Onnxruntime dynamic loading

### 💼 Other

- 0.7.4

### 🚜 Refactor

- Load dylib in data local dir

### ⚙️ Miscellaneous Tasks

- We don't need dll at bundle time
## [0.7.3] - 2025-11-06

### 🚀 Features

- Add zip via http
- Skip if hash matches
- Add tempfile dependency and implement tests for ensure_dylibs function
- Update dependencies and improve CUDA integration in app initialization
- Refactor CUDA package handling and improve dylib loading in app initialization

### 🐛 Bug Fixes

- Download cuda libs to current dir

### 💼 Other

- 0.7.3

### 🚜 Refactor

- Add cuda-rt crate
- Remove CUDA setup function from build script
- Simplify workspace members list in Cargo.toml
- Make it a runtime lib
- Mark RecordEntry struct as unused
- Switch reqwest client to blocking for synchronous operations
- Replace ureq with reqwest for HTTP requests in fetch_and_extract
- Replace ureq with reqwest in zip.rs and update dependencies in Cargo.toml and Cargo.lock
- Speedup cuda-rt
- Remove unused dependencies from Cargo.lock

### 📚 Documentation

- Remove optional Python dependency from installation instructions

### ⚙️ Miscellaneous Tasks

- Enable cuda on windows by default
- Set default workspace member to koharu
## [0.7.2] - 2025-11-05

### 💼 Other

- 0.7.2
## [0.7.1] - 2025-11-05

### 🐛 Bug Fixes

- Add CUDA_COMPUTE_CAP environment variable to build workflow
- Update artifact upload path to include all executable files
- Update resource paths in tauri.windows.conf.json to specify release directory

### 💼 Other

- 0.7.1

### 🚜 Refactor

- Switch to tauri nsis

### ⚙️ Miscellaneous Tasks

- Add code signing
- Add updater
- Enable bundle
- Enable bundle
- Remove unused crate
## [0.7.0] - 2025-11-05

### 🚀 Features

- Add dilate and erode param
- Llm crate
- Add Qwen2_5_1_5BInstruct
- Add Sakura1_5BQwen2_5_1_0
- Prompt format
- Implement llm in koharu
- Add llm commands
- LLM translation

### 🐛 Bug Fixes

- Centralize SplashScreen
- Use lowercase and remove prompt from output
- Correct model identifiers for Qwen2 and update README links
- Remove futures
- Update CUDA_COMPUTE_CAP to 121
- Add missing step for MSVC development command setup
- Disable default features of tokenizers to avoid build issue

### 💼 Other

- Llm crate

### 🚜 Refactor

- Make the llm ready to use
- Production-ready

### 📚 Documentation

- Update README to include CUDA installation instructions for candle
- Update README for improved clarity and structure
- Enhance README with detailed features and model descriptions
- Add logo to README for improved visual appeal

### ⚙️ Miscellaneous Tasks

- Update package version to 0.7.0 and add related projects section in README
- Remove unused futures dependency from Cargo.toml
- Build cuda features
- Add cuda setup to release workflow
## [0.6.1] - 2025-11-03

### 💼 Other

- 0.6.1
## [0.6.0] - 2025-11-03

### 🐛 Bug Fixes

- Thumbnails panel
- Add page number to thumbnails
- Canvas scollbar

### 💼 Other

- *(deps)* Bump clap from 4.5.50 to 4.5.51 (#15)
- 5.0.1

### 🚜 Refactor

- Remove Slint
- Add initial next.js
- Style components
- Setup Tauri
- Impl state
- Remove update
- Replace react context with zustand
- Simplify image.rs
- Make Rust the source of truth

### 📚 Documentation

- Update dev steps
- Update GUI framework reference from Slint to Tauri
- Update README to clarify installation instructions and dependencies

### 🎨 Styling

- Update splash screen text colors to pink

### ⚙️ Miscellaneous Tasks

- Stop publishing crates
- Modify GHA to adapt tauri
- Remove unused lib
- Add params to detect
- Bump version to 0.6.0
## [0.5.0] - 2025-10-28

### 🚀 Features

- Multiresolution blending with tiled inpainting
- Inpaint

### 💼 Other

- 0.5.0

### 📚 Documentation

- Enhance dev and cuda part
- Update README to clarify model downloading and ONNX conversion
- Rewrite README.md
- Reorganize README for clarity and remove outdated sections

### ⚙️ Miscellaneous Tasks

- Apply cargo clippy
- Add app ico
- Update deps
- Remove unused ONNX inference script
- Add publish workflow
## [0.4.0] - 2025-10-28

### 🚀 Features

- Bundle cuda and cudnn

### 🐛 Bug Fixes

- Ensure Spinner is indeterminate in InProgressOverlay

### 💼 Other

- 0.4.0

### ⚙️ Miscellaneous Tasks

- Rename channel to win
- Bump version to 0.3.1
- Add cuda flag
## [0.3.0] - 2025-10-27

### 💼 Other

- 0.3.0
## [0.2.4-alpha] - 2025-10-27

### 📚 Documentation

- Add dev steps
## [0.2.3] - 2025-10-27

### 💼 Other

- 0.2.3
## [0.2.2] - 2025-10-27

### 💼 Other

- 0.2.2

### 📚 Documentation

- Add installation guide

### ⚙️ Miscellaneous Tasks

- Bump version to 0.2.1
## [0.2.0] - 2025-10-25

### 🚀 Features

- Detection
- Ocr

### 🐛 Bug Fixes

- Update state management to useEditorStore in App component
- Update image handling in detection function and improve error logging
- Improve error handling in image opening function
- Images with wrong suffix not able to load

### 💼 Other

- 0.2.0

### 🚜 Refactor

- Remove onnxruntime-web
- Add commands.rs
- Rename Result type to CommandResult for consistency
- Replace RwLock with Mutex for AppState management and move AppState to its own module
- Single page editor
- Streamline operations

### ⚡ Performance

- Improve image loading time

### 🎨 Styling

- Format slint features for better readability

### ⚙️ Miscellaneous Tasks

- Update description and windows capabilities in default.json
- Update dependencies in package.json and bun.lock
- Update @types/node and @types/react to latest versions
- Remove unused wasm-bindgen dependency from Cargo.toml
- Update dependencies in package.json and bun.lock to latest versions
- Update deps
- Update deps
- Bump version for 0.2.0
- Bump version for sub-crates
- Publish sub-crates
- Add alias and env to support live preview
- Separate logic into its own module
- Update deps
- Add rust GHA
- Update deps
- Use winit with femtovg-wgpu render
- Add release action
## [0.1.11] - 2025-08-06

### 🐛 Bug Fixes

- Correct formatting of CUDA paths in README.md

### 🚜 Refactor

- Load cuda
- Update section headers in README.md

### ⚙️ Miscellaneous Tasks

- Update package versions to 0.1.10
## [0.1.10] - 2025-08-04

### 🚀 Features

- Add p-limit library for limiting concurrent inference requests
- Add cuda

### 🐛 Bug Fixes

- Initialize lib before using
- Use webgpu
- Debug inpaint
- Remove loop
- Ensure devIndicators is set to false in next.config.ts
- Improve error message formatting in model initialization
- Reduce app size
- Add tauri cli to root

### 🚜 Refactor

- Add splash screen
- Remove ort and use onnxruntime-web
- Remove unused Tauri import and clean up code
- Consolidate image and model utility functions into a single directory
- Remove hooks
- Rename util to utils
- Use Jimp
- Use ImageBitmap
- Move maskThreshold to inference parameters and relocate non-maximum suppression implementation
- Use ImageBitmap directly
- Remove settings page
- Add TODO for improved error handling in model initialization
- Streamline image resizing and tensor creation in inference function
- Remove VSCode extensions configuration file
- Remove unused settings button and related imports from Topbar component
- Rename selectedTool to tool in workflow store and related components
- Rename selectedTool to tool in Canvas component

### 📚 Documentation

- Update README to remove browser trial reference
- Update README to reflect technology stack and application hosting
- Remove ONNX model references from README
- Add Rust version requirement to prerequisites

### ⚙️ Miscellaneous Tasks

- Bump package versions to 0.1.2
- Update artifact upload condition to only trigger on tag pushes
- Update dependencies in package.json files
- Remove experimental directories from .gitignore
- Remove web deployment workflow
- Remove .gitattributes file
- Update prettier to version 3.6.2 in package.json and bun.lock
- Update Next.js configuration and dependencies
- Update onnxruntime-web to version 1.23.0-dev.20250703-7fc6235861
- Update workspace resolver to version 3
- Update dependencies in package.json
- Update cargo lock
- Update ort to rc10
- Reduce app size
- Rename web to next
- Update package version to 0.1.10
## [0.1.2] - 2025-05-29

### 🐛 Bug Fixes

- Update jq command to use raw output for version retrieval
- Place dll into root dir
- Fix Windows build
- Update condition for Windows Tauri app bundling
- Remove bundling step for Windows 2025 platform
- Streamline Rust installation step in build workflow

### ⚙️ Miscellaneous Tasks

- Fix windows
- Use underscore
- Use version from Cargo.toml
- Update version retrieval method in build workflow
- Support nsis bundle
- Remove unused setting
- Fix windows build
- Try to fix windows build
- Fix windows
- Fix windows bundle
- Refine cuda workflow
- Bump version to 0.1.2
## [0.1.1] - 2025-05-28

### 🚀 Features

- Add segmentation functionality and update state management
- Add lama
- Scroll mouse wheel to zoom the canvas
- Free-draggable canvas layout with floating toolbars
- Implement image resizing with padding and add revert functionality
- Add logging support and update run function to return a result
- Dilate segment mask
- Use correct exported onnx inpaint model
- Parallel inpaint
- Add notification
- Import react aria
- Use english for UI
- Split encoder and decoder to reuse encoder outputs for manga-ocr, close #7
- Integrate Radix UI themes and update components
- Add confidence and NMS threshold controls in DetectionPanel and update detect function to accept thresholds
- Enhance OCR and Translation panels with badge indicators for text items
- Update translation logic to use JSON format and improve response handling
- Add system prompt label to TranslationPanel for improved clarity
- Initial support of web
- Store model in indexeddb
- Add progress indicator to splash screen during initialization

### 🐛 Bug Fixes

- Right panel scrollbar
- Canvas cannot be fully displayed when scrollbar is scrolled to the bottom
- Update README to correct bundle size from less than 10MB to less than 20MB
- Handle null segmentData in Canvas and clear segment on new image load in Topbar
- Update mask interpretation logic for inpainting areas
- Use for-await-of to handle asynchronous text processing in inpainting
- Prevent operations from previous file continuing on new file open
- Update model path and optimize mask value extraction
- Update model path in CLI and add BallonsTranslator to .gitignore
- Canvas not automatically updated after inpaint operation
- Add thresholded for mask
- Adjust position of ScaleControl component for better visibility
- Ensure canvas dimensions default to zero when image data is unavailable
- Correct token ID calculation in OCR inference logic for accurate text extraction
- Inpaint pixel index
- Check for navigator before setting wasm.numThreads

### 💼 Other

- Use rwlock
- Add ocr to web
- Add inpaint inference

### 🚜 Refactor

- Extract canvas logic into hooks and utils
- Remove unused execution providers from model initialization
- Remove imageSrcHistory and related logic from canvas store
- Update inference triggers in DetectionPanel and OCRPanel, and adjust imageSrc handling in Topbar
- Improve layout and responsiveness of main components, remove unused window size hook
- Remove bugy wheel event handler from Canvas component to streamline functionality
- Add tauri-plugin-fs for file system access and update dependencies in package.json and Cargo.lock
- Streamline Canvas component by removing redundant stage reference and adding inpaint layer reference
- Update image handling in components to use imagePath instead of image for improved file management
- Optimize OCR processing logic in OCRPanel for improved text extraction
- Update ONNX runtime import to use 'onnxruntime-web' for consistency across detection and OCR modules
- Simplify Next.js configuration by removing unused image settings and asset prefix
- Remove unused output setting from Next.js configuration
- Remove unused projects and clean up workspace
- Remove unused packages from Cargo.lock
- Implement caching for model downloads to improve performance
- Use sub modules

### 📚 Documentation

- Add Discord support note and fix punctuation in development status note
- Add LaMa inpainting model to the models section of README
- Update workflow steps and model references in README
- Add Tauri prerequisites installation instructions to README
- Add instructions for creating a debug build in README
- Add CUDA acceleration feature instructions to README
- Update README to include CUDA acceleration instructions and remove obsolete guidance
- Update README to include instructions for enabling CUDA acceleration
- Update README to include browser usage information

### ⚡ Performance

- Use multi-thread wasm

### 🎨 Styling

- Update SplashScreen component with new design and branding elements
- Increase width of Tools component and adjust layout in main application for improved usability
- Update Canvas component layout for improved alignment and overflow handling
- Adjust height and overflow handling in OCRPanel and TranslationPanel for improved layout
- Refine scale adjustment increments in ScaleControl for smoother user experience
- Simplify button component in TranslationPanel for cleaner code
- Update height handling in OCRPanel and TranslationPanel for better layout consistency
- Simplify function calls and improve readability in DetectionPanel and Topbar components

### 🧪 Testing

- Add mask processing and saving functionality in main.rs

### ⚙️ Miscellaneous Tasks

- Remove unused useRef import in canvas component
- Switch to a generic dotenv file (#4)
- Cleanup lama inference code
- Move execution provider initialization to main function
- Set default feature to CUDA in Cargo.toml
- Rename model_path to model in CLI arguments for consistency
- Inference python scripts
- Add test batch
- Update deps
- Improve splashscreen
- Restructure files
- Remove obsolete Paperspace automation script
- Remove obsolete Paperspace credentials from example env file
- Use re-upload models
- Cleanup
- Update workspace members and add .gitattributes for language detection
- Remove unused dependencies from Cargo.lock
- Remove loading spinner from splash screen
- Adjust logging level for event loop runner to Error
- Remove unused download button from topbar
- Conditionally render inpaint image based on selected tool
- Replace textarea with TextArea component in TranslationPanel for consistency
- Update API settings documentation and improve placeholder text in form inputs
- Remove splashscreen window configuration and related initialization logic
- Add web deployment
- Update dependencies to latest versions in bun.lock and package.json
- Update dependencies and configuration files
- Add 'use client' directive to inpaint.ts
- Add icon for web
- Update deps
- Add sub modules
- Add example for ctd
- Format members list in Cargo.toml for consistency
- Add example for manga-ocr
- Add example for lama
- Use workspace dependencies
- Test bundle
- Trigger build
- Add distDir and output configuration to next.config.ts
- Fix windows bundle
- Fix version
- Add prefix for windows bundle
- Bump version to 0.1.1
## [0.1.1] - 2025-04-23

### 🚀 Features

- Highlight selected text item
- Use GPU acceleration when available
- Translate text using stream mode
- OCR text editable
- Trigger OCR automatically when detect complete
- Add CUDA feature support for ort dependency and update build configuration

### 🐛 Bug Fixes

- Correct JSON parsing for artifact paths in release workflow
- Update artifact paths formatting for GitHub release upload
- Refactor artifact upload step to improve handling of paths
- Specify shell for artifact processing step in release workflow
- Stream translate missing last piece
- Redetect when switch tools, settings
- Remove unnecessary blank line in README.md

### 🚜 Refactor

- Use available parallelism for intra-threads in model sessions

### 📚 Documentation

- Add note about development status and issue reporting in README.md

### ⚙️ Miscellaneous Tasks

- Add rust cache
- Upload portable executable
- Update README and configuration for Windows setup and Tauri CLI dependencies
- Add workflow for PR
- Rename job from publish-tauri to build-tauri in workflow
- Auto-trigger inference when imageSrc changes
- Add "prettier-plugin-tailwindcss" and format
- Update macOS and Windows platform specifications in build configuration
- Update publish workflow to trigger on version tags only
- Add tag trigger for versioned releases in build workflow
## [0.1.0] - 2025-04-22

### 🚀 Features

- Initialize Tauri + React application with basic greeting functionality
- Add manga109 to YOLO conversion script
- Add detection notebook for dataset preparation and training with YOLO
- Add Puppeteer script for Paperspace login and notebook access
- Enhance manga109 to YOLO conversion with 80/20 train/val split and YAML config
- Add script to download Blue Archive comics with asynchronous requests
- Simplify manga109 to YOLO conversion by removing book selection and updating class mapping
- Add comic-text-detector package with initial implementation
- Implement main functionality for comic-text-detector and update dependencies
- Update dependencies and enhance model session handling in main.rs
- Add image processing and object detection functionality to comic-text-detector
- Enhance image processing with configurable thresholds and bounding box drawing
- Add layout components and tools for layer management
- Integrate react-konva for canvas rendering in App component
- Add Tauri plugins for dialog and logging, enhance canvas functionality, and improve app structure
- Enhance canvas layout and improve image handling in Topbar component
- Add Tauri plugins for store and persisted scope, update dependencies in Cargo and package files
- Add scale control
- Add comic text detection functionality and update dependencies
- Implement comic text detection and update dependencies
- Add blocks state management and render rectangles in canvas
- Enhance canvas and detection panel with loading state and reset functionality
- Order bboxes

### 🐛 Bug Fixes

- Correct model import in main.rs
- Update gdown command to use placeholder for ID
- Add missing description for manga-ocr model in README
- Move stageRef.current.destroyChildren() call to ensure proper cleanup before loading new image
- Ensure file selection is validated before processing in Topbar component
- Ensure stage size is set correctly to match image dimensions
- Simplify ort dependency declaration by removing unnecessary features
- Clears blocks only when new image loaded
- Display detected texts in detection and OCR panels
- Ocr results got cutted
- Add max height and overflow to OCR panel for better layout

### 🚜 Refactor

- Remove execution output from validation cell in detection notebook
- Update project structure and dependencies
- Improve scale control component structure and functionality
- Update tool selection and improve topbar icon imports
- Use konva-react

### 📚 Documentation

- Update README with preview section and download instructions
- Update workflow section to use checklist format

### ⚙️ Miscellaneous Tasks

- Update dependencies to specific versions in bun.lock
- Update @types/react and vite to latest versions
- Reorganize README structure and add models section
- Update dependencies to latest versions
- Add prettier
- Tauri just output the executable file
