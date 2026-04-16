mod helpers;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, ErrorData, Implementation, ServerCapabilities, ServerInfo,
    ToolsCapability,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use tokio::sync::RwLock;

use koharu_app::{AppResources, edit, engine, io, llm, pipeline};
use koharu_core::commands::{
    AddTextBlockPayload, DocumentIdParam, DocumentIndexParam, ExportDocumentParams, FileEntry,
    InpaintRegionParams, LlmGenerateParams, LlmLoadParams, MaskMorphPayload, OpenDocumentsParams,
    OpenDocumentsPayload, ProcessParams, ProcessRequest, RemoveTextBlockPayload, RenderParams,
    UpdateTextBlockPayload, ViewImageParams, ViewTextBlockParams,
};
use koharu_core::views::to_doc_info;
use koharu_core::{LlmLoadRequest, PipelineLlmRequest, Region};

use crate::shared::SharedState;

use helpers::encode_png_base64;

#[derive(Clone)]
pub struct KoharuMcp {
    pub shared: SharedState,
    tool_router: ToolRouter<Self>,
}

impl KoharuMcp {
    pub fn new(shared: SharedState) -> Self {
        Self {
            shared,
            tool_router: Self::tool_router(),
        }
    }

    fn resources(&self) -> Result<AppResources, String> {
        self.shared
            .get()
            .ok_or_else(|| "Resources not initialized yet".to_string())
    }

    async fn document_id_for_index(
        &self,
        resources: &AppResources,
        index: usize,
    ) -> Result<String, String> {
        let entries = resources.storage.list_pages().await;
        entries
            .get(index)
            .map(|page| page.id.clone())
            .ok_or_else(|| format!("Document index {index} not found"))
    }
}

#[tool_router]
impl KoharuMcp {
    #[tool(description = "Get the application version")]
    async fn app_version(&self) -> Result<String, String> {
        let res = self.resources()?;
        io::app_version(res).await.map_err(|e| e.to_string())
    }

    #[tool(description = "Get device information (ML device, GPU info)")]
    async fn device(&self) -> Result<String, String> {
        let res = self.resources()?;
        let info = io::device(res).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
    }

    #[tool(description = "Get the number of loaded documents")]
    async fn get_documents(&self) -> Result<String, String> {
        let res = self.resources()?;
        let count = io::get_documents(res).await.map_err(|e| e.to_string())?;
        Ok(format!("{count} document(s) loaded"))
    }

    #[tool(
        description = "Get document metadata and text blocks (no images). Returns name, dimensions, processing state, and all text block details."
    )]
    async fn get_document(
        &self,
        Parameters(p): Parameters<DocumentIdParam>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let doc = io::get_document(res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;
        let info = to_doc_info(&doc);
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
    }

    #[tool(description = "List available font families for text rendering")]
    async fn list_font_families(&self) -> Result<String, String> {
        let res = self.resources()?;
        let renderer = engine::get_renderer(&res)
            .await
            .map_err(|e| e.to_string())?;
        let fonts = renderer.available_fonts().map_err(|e| e.to_string())?;
        Ok(fonts
            .into_iter()
            .map(|font| format!("{} ({})", font.family_name, font.post_script_name))
            .collect::<Vec<_>>()
            .join(", "))
    }

    #[tool(
        description = "List the grouped LLM catalog for local models and configured remote providers"
    )]
    async fn llm_list(&self) -> Result<String, String> {
        let res = self.resources()?;
        let catalog = llm::llm_catalog(res).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())
    }

    #[tool(description = "Check if an LLM model is loaded and ready")]
    async fn llm_ready(&self) -> Result<String, String> {
        let res = self.resources()?;
        let ready = llm::llm_ready(res).await.map_err(|e| e.to_string())?;
        Ok(if ready {
            "LLM is ready".to_string()
        } else {
            "LLM is not loaded".to_string()
        })
    }

    #[tool(
        description = "View a document image layer. Returns the image so you can see the manga page, detection mask, inpainted result, or final rendered output."
    )]
    async fn view_image(
        &self,
        Parameters(p): Parameters<ViewImageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let res = self
            .resources()
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let doc = io::get_document(res.clone(), &p.document_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let max_size = p.max_size.unwrap_or(1024);

        let blob_ref = match p.layer.as_str() {
            "original" => &doc.source,
            "segment" => doc.segment.as_ref().ok_or_else(|| {
                ErrorData::internal_error("No segment mask available. Run detect first.", None)
            })?,
            "inpainted" => doc.inpainted.as_ref().ok_or_else(|| {
                ErrorData::internal_error("No inpainted image available. Run inpaint first.", None)
            })?,
            "rendered" => doc.rendered.as_ref().ok_or_else(|| {
                ErrorData::internal_error("No rendered image available. Run render first.", None)
            })?,
            other => {
                return Err(ErrorData::internal_error(
                    format!(
                        "Unknown layer: {other}. Valid: original, segment, inpainted, rendered"
                    ),
                    None,
                ));
            }
        };

        let img = res
            .storage
            .images
            .load(blob_ref)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let b64 = encode_png_base64(&img, max_size);
        Ok(CallToolResult::success(vec![
            Content::text(format!(
                "Viewing '{}' layer of document '{}' ({}x{})",
                p.layer, doc.name, doc.width, doc.height
            )),
            Content::image(b64, "image/png"),
        ]))
    }

    #[tool(
        description = "View a cropped region of a specific text block. Useful for inspecting OCR results or rendered text quality."
    )]
    async fn view_text_block(
        &self,
        Parameters(p): Parameters<ViewTextBlockParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let res = self
            .resources()
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let doc = io::get_document(res.clone(), &p.document_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let block = doc.text_blocks.get(p.text_block_index).ok_or_else(|| {
            ErrorData::internal_error(format!("Text block {} not found", p.text_block_index), None)
        })?;

        let layer = p.layer.as_deref().unwrap_or("original");
        let blob_ref = match layer {
            "original" => &doc.source,
            "rendered" => doc.rendered.as_ref().ok_or_else(|| {
                ErrorData::internal_error("No rendered image. Run render first.", None)
            })?,
            other => {
                return Err(ErrorData::internal_error(
                    format!("Unknown layer: {other}. Valid: original, rendered"),
                    None,
                ));
            }
        };
        let source = res
            .storage
            .images
            .load(blob_ref)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let x = (block.x.max(0.0) as u32).min(doc.width.saturating_sub(1));
        let y = (block.y.max(0.0) as u32).min(doc.height.saturating_sub(1));
        let w = (block.width as u32).min(doc.width.saturating_sub(x));
        let h = (block.height as u32).min(doc.height.saturating_sub(y));

        if w == 0 || h == 0 {
            return Err(ErrorData::internal_error(
                "Text block has zero dimensions",
                None,
            ));
        }

        let crop = source.crop_imm(x, y, w, h);
        let b64 = encode_png_base64(&crop, 512);

        let mut desc = format!(
            "Text block [{}] at ({},{}) {}x{}",
            p.text_block_index, x, y, w, h
        );
        if let Some(ref text) = block.text {
            desc.push_str(&format!("\nOCR: {text}"));
        }
        if let Some(ref tr) = block.translation {
            desc.push_str(&format!("\nTranslation: {tr}"));
        }

        Ok(CallToolResult::success(vec![
            Content::text(desc),
            Content::image(b64, "image/png"),
        ]))
    }

    #[tool(
        description = "Open image files from disk paths. Replaces any currently loaded documents."
    )]
    async fn open_documents(
        &self,
        Parameters(p): Parameters<OpenDocumentsParams>,
    ) -> Result<String, String> {
        let res = self.resources()?;

        let files: Result<Vec<FileEntry>, String> = p
            .paths
            .iter()
            .map(|path| {
                let data =
                    std::fs::read(path).map_err(|e| format!("Failed to read {path}: {e}"))?;
                let name = PathBuf::from(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                Ok(FileEntry { name, data })
            })
            .collect();

        let count = io::open_documents(res.clone(), OpenDocumentsPayload { files: files? })
            .await
            .map_err(|e| e.to_string())?;

        let entries = res.storage.list_pages().await;
        let names: Vec<&str> = entries.iter().map(|d| d.name.as_str()).collect();
        Ok(format!("Loaded {count} document(s): {}", names.join(", ")))
    }

    #[tool(description = "Export the rendered document to a file on disk")]
    async fn export_document(
        &self,
        Parameters(p): Parameters<ExportDocumentParams>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let result = io::export_document(res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;

        std::fs::write(&p.output_path, &result.data)
            .map_err(|e| format!("Failed to write {}: {e}", p.output_path))?;

        Ok(format!("Exported to {}", p.output_path))
    }

    #[tool(
        description = "Detect text blocks and fonts in a manga page. Finds speech bubbles, text regions, and predicts font properties."
    )]
    async fn detect(&self, Parameters(p): Parameters<DocumentIdParam>) -> Result<String, String> {
        let res = self.resources()?;
        engine::run_many(
            &[
                "pp-doclayout-v3",
                "comic-text-detector-seg",
                "yuzumarker-font-detection",
            ],
            &res,
            &p.document_id,
        )
        .await
        .map_err(|e| e.to_string())?;

        let doc = io::get_document(res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut lines = vec![format!("Detected {} text block(s):", doc.text_blocks.len())];
        for (i, b) in doc.text_blocks.iter().enumerate() {
            lines.push(format!(
                "  [{}] ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}",
                i, b.x, b.y, b.width, b.height, b.confidence
            ));
        }
        Ok(lines.join("\n"))
    }

    #[tool(
        description = "Run OCR (optical character recognition) on detected text blocks to extract the original text."
    )]
    async fn ocr(&self, Parameters(p): Parameters<DocumentIdParam>) -> Result<String, String> {
        let res = self.resources()?;
        engine::run_one("paddle-ocr-vl-1.5", &res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;

        let doc = io::get_document(res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut lines = vec!["OCR results:".to_string()];
        for (i, b) in doc.text_blocks.iter().enumerate() {
            let text = b.text.as_deref().unwrap_or("(empty)");
            lines.push(format!("  [{i}] {text}"));
        }
        Ok(lines.join("\n"))
    }

    #[tool(
        description = "Inpaint (remove) text from the image using the detection mask. Fills text regions with surrounding background."
    )]
    async fn inpaint(&self, Parameters(p): Parameters<DocumentIdParam>) -> Result<String, String> {
        let res = self.resources()?;
        engine::run_one("lama-manga", &res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok("Inpainting complete".to_string())
    }

    #[tool(
        description = "Render translated text onto the inpainted image. Applies font styling, layout, and shader effects."
    )]
    async fn render(&self, Parameters(p): Parameters<RenderParams>) -> Result<String, String> {
        let res = self.resources()?;
        let effect = p
            .shader_effect
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|e: anyhow::Error| e.to_string())?;

        engine::render_document(&res, &p.document_id, p.text_block_index, effect, None, None)
            .await
            .map_err(|e| e.to_string())?;

        Ok("Render complete".to_string())
    }

    #[tool(
        description = "Load an LLM translation model. This downloads and initializes the model."
    )]
    async fn llm_load(&self, Parameters(p): Parameters<LlmLoadParams>) -> Result<String, String> {
        let res = self.resources()?;
        llm::llm_load(
            res,
            LlmLoadRequest {
                target: p.target.clone(),
                options: p.options.clone(),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(format!(
            "Loading {} target '{}'{}",
            match p.target.kind {
                koharu_core::LlmTargetKind::Local => "local",
                koharu_core::LlmTargetKind::Provider => "provider",
            },
            p.target.model_id,
            p.target
                .provider_id
                .as_deref()
                .map(|provider| format!(" from {provider}"))
                .unwrap_or_default()
        ))
    }

    #[tool(description = "Unload the current LLM model from memory")]
    async fn llm_offload(&self) -> Result<String, String> {
        let res = self.resources()?;
        llm::llm_offload(res).await.map_err(|e| e.to_string())?;
        Ok("LLM offloaded".to_string())
    }

    #[tool(
        description = "Generate translations for text blocks using the loaded LLM. Returns the translated text."
    )]
    async fn llm_generate(
        &self,
        Parameters(p): Parameters<LlmGenerateParams>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        llm::llm_generate(
            res.clone(),
            &p.document_id,
            p.text_block_index,
            p.language.as_deref(),
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

        let doc = io::get_document(res, &p.document_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut lines = vec!["Translations:".to_string()];
        for (i, b) in doc.text_blocks.iter().enumerate() {
            let src = b.text.as_deref().unwrap_or("?");
            let tr = b.translation.as_deref().unwrap_or("(none)");
            lines.push(format!("  [{i}] {src} -> {tr}"));
        }
        Ok(lines.join("\n"))
    }

    #[tool(
        description = "Run the full processing pipeline: detect -> OCR -> inpaint -> translate -> render. Processes all steps automatically."
    )]
    async fn process(&self, Parameters(p): Parameters<ProcessParams>) -> Result<String, String> {
        let res = self.resources()?;
        let effect = p
            .shader_effect
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|e: anyhow::Error| e.to_string())?;

        let jobs = Arc::new(RwLock::new(HashMap::new()));
        pipeline::process(
            res,
            ProcessRequest {
                document_id: p.document_id,
                llm: p.llm_target.map(|target| PipelineLlmRequest {
                    target,
                    options: None,
                }),
                language: p.language,
                system_prompt: p.system_prompt,
                shader_effect: effect,
                shader_stroke: None,
                default_font: None,
            },
            jobs,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok("Pipeline started".to_string())
    }

    #[tool(
        description = "Update a text block's properties. Only the fields you provide will be changed."
    )]
    async fn update_text_block(
        &self,
        Parameters(p): Parameters<UpdateTextBlockPayload>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        let info = edit::update_text_block(
            res,
            &document_id,
            edit::UpdateTextBlockArgs {
                text_block_index: p.text_block_index,
                translation: p.translation,
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
                font_families: p.font_families,
                font_size: p.font_size,
                color: p.color,
                shader_effect: p.shader_effect,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
    }

    #[tool(description = "Add a new empty text block at the specified position")]
    async fn add_text_block(
        &self,
        Parameters(p): Parameters<AddTextBlockPayload>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        let index = edit::add_text_block(res, &document_id, p.x, p.y, p.width, p.height)
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("Added text block at index {index}"))
    }

    #[tool(description = "Remove a text block by index")]
    async fn remove_text_block(
        &self,
        Parameters(p): Parameters<RemoveTextBlockPayload>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        let remaining = edit::remove_text_block(res, &document_id, p.text_block_index)
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("Removed text block. {remaining} remaining."))
    }

    #[tool(description = "Dilate the text detection mask by radius")]
    async fn dilate_mask(
        &self,
        Parameters(p): Parameters<MaskMorphPayload>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        edit::dilate_mask(res, &document_id, p.radius)
            .await
            .map_err(|e| e.to_string())?;
        Ok("Dilated mask".to_string())
    }

    #[tool(description = "Erode the text detection mask by radius")]
    async fn erode_mask(
        &self,
        Parameters(p): Parameters<MaskMorphPayload>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        edit::erode_mask(res, &document_id, p.radius)
            .await
            .map_err(|e| e.to_string())?;
        Ok("Eroded mask".to_string())
    }

    #[tool(description = "Re-inpaint a specific rectangular region")]
    async fn inpaint_region(
        &self,
        Parameters(p): Parameters<InpaintRegionParams>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        edit::inpaint_partial(
            res,
            &p.document_id,
            Region {
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
            },
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(format!(
            "Inpainted region ({},{}) {}x{}",
            p.x, p.y, p.width, p.height
        ))
    }

    #[tool(description = "Get the document ID for the document at index")]
    async fn get_document_id_for_index(
        &self,
        Parameters(p): Parameters<DocumentIndexParam>,
    ) -> Result<String, String> {
        let res = self.resources()?;
        let document_id = self.document_id_for_index(&res, p.index).await?;
        Ok(document_id)
    }
}

#[tool_handler]
impl ServerHandler for KoharuMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "koharu".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            instructions: Some(
                "Koharu manga translation tools. Use open_documents to load images, then detect -> ocr -> inpaint -> llm_generate -> render to translate.".to_string(),
            ),
            ..Default::default()
        }
    }
}
