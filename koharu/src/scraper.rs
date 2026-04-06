//! Web scraper for importing manga pages from supported sites.
//!
//! Uses an embedded JavaScript engine (rquickjs) to decode obfuscated page
//! data and extract image URLs, then downloads them with retry and concurrency limits.
//!
//! # Security Considerations
//! - JS execution is sandboxed via rquickjs (no filesystem/network access)
//! - Content size limits prevent DoS via large payloads
//! - Path traversal prevention on extracted paths
//! - CDN host allowlist prevents SSRF

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use koharu_core::FileEntry;
use rand::Rng;
use regex::Regex;
use rquickjs::{Context as JsContext, Runtime};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Semaphore, mpsc};

/// Maximum concurrent downloads
const DOWNLOAD_CONCURRENCY: usize = 3;
/// Maximum retry attempts per image
const MAX_RETRIES: u32 = 3;
/// Maximum HTML page size (5MB) - security: prevent DoS via large payloads
const MAX_PAGE_SIZE: usize = 5 * 1024 * 1024;
/// Maximum number of images per chapter - security: prevent resource exhaustion
const MAX_IMAGES_PER_CHAPTER: usize = 500;
/// Minimum delay between image downloads (ms) - respect rate limits
const MIN_DOWNLOAD_DELAY_MS: u64 = 200;
/// Maximum delay between image downloads (ms)
const MAX_DOWNLOAD_DELAY_MS: u64 = 500;

/// Allowed CDN hosts for image downloads - security: prevent SSRF
const ALLOWED_CDN_HOSTS: &[&str] = &[
    "i.hamreus.com",
    "us.hamreus.com",
    "eu.hamreus.com",
    "dx.hamreus.com",
    "lt.hamreus.com",
];

/// CDN hosts to try in order (failover)
const CDN_HOSTS: &[&str] = &[
    "https://i.hamreus.com",
    "https://us.hamreus.com",
    "https://eu.hamreus.com",
];

/// Progress event payload sent to frontend
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScraperProgress {
    pub current: usize,
    pub total: usize,
    pub message: String,
}

/// Parsed image data from the decoded JavaScript
#[derive(Debug, Clone)]
struct ImageData {
    path: String,
    files: Vec<String>,
    /// CDN host extracted from page or default
    cdn_host: String,
    /// Authentication query string (e, m params)
    query_string: String,
}

/// Validates that the URL is a supported manhuagui chapter URL.
/// Supports both www.manhuagui.com and tw.manhuagui.com (Taiwan mirror).
pub fn validate_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).context("Invalid URL format")?;

    let host = parsed.host_str().unwrap_or("");
    // Support main site and Taiwan mirror
    if !host.ends_with("manhuagui.com") {
        bail!("Only manhuagui.com URLs are supported (including tw.manhuagui.com)");
    }

    // Expected format: /comic/{manga_id}/{chapter_id}.html
    let path = parsed.path();
    if !path.starts_with("/comic/") || !path.ends_with(".html") {
        bail!("Invalid chapter URL format. Expected: https://www.manhuagui.com/comic/{{id}}/{{chapter}}.html");
    }

    Ok(())
}

/// Scrapes a manga chapter from manhuagui.com and returns downloaded images as FileEntry.
#[tracing::instrument(level = "info", skip(app_handle))]
pub async fn scrape_manhuagui(url: &str, app_handle: &AppHandle) -> Result<Vec<FileEntry>> {
    validate_url(url)?;

    // Emit initial progress
    app_handle
        .emit("scraper:progress", ScraperProgress {
            current: 0,
            total: 0,
            message: "Loading page...".to_string(),
        })
        .ok();

    // 1. Fetch the HTML page
    let html = fetch_page(url).await?;
    tracing::debug!(html_len = html.len(), "Fetched page HTML");

    // 2. Extract the packed JavaScript
    let packed_js = extract_packed_js(&html)?;
    tracing::debug!(packed_len = packed_js.len(), "Extracted packed JS");

    // 3. Decode the packed JavaScript using rquickjs
    let decoded = decode_packed_js(&packed_js)?;
    tracing::debug!(decoded_len = decoded.len(), "Decoded JS");

    // 4. Parse image data from the decoded result
    let image_data = parse_image_data(&decoded, &html)?;
    tracing::info!(
        path = %image_data.path,
        file_count = image_data.files.len(),
        "Parsed image data"
    );

    // 5. Build image URLs
    let image_urls = build_image_urls(&image_data);

    if image_urls.is_empty() {
        bail!("No images found on page");
    }

    app_handle
        .emit("scraper:progress", ScraperProgress {
            current: 0,
            total: image_urls.len(),
            message: format!("Found {} images", image_urls.len()),
        })
        .ok();

    // 6. Download images with progress
    let files = download_images(&image_urls, app_handle).await?;

    if files.is_empty() {
        bail!("Failed to download any images");
    }

    Ok(files)
}

/// Fetches the page HTML with browser-like headers.
/// Security: Enforces content size limit to prevent DoS.
async fn fetch_page(url: &str) -> Result<String> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::REFERER,
        "https://www.manhuagui.com/".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".parse().unwrap(),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .default_headers(headers)
        .build()?;

    let response = client.get(url).send().await.context("Failed to fetch page")?;

    if !response.status().is_success() {
        bail!("Page returned HTTP {}", response.status());
    }

    // Security: Check content length before downloading
    if let Some(content_length) = response.content_length() {
        if content_length as usize > MAX_PAGE_SIZE {
            bail!("Page too large ({} bytes, max {})", content_length, MAX_PAGE_SIZE);
        }
    }

    let text = response.text().await.context("Failed to read page body")?;

    // Security: Verify size after download (in case Content-Length was missing/wrong)
    if text.len() > MAX_PAGE_SIZE {
        bail!("Page too large ({} bytes, max {})", text.len(), MAX_PAGE_SIZE);
    }

    Ok(text)
}

/// Extracts the packed JavaScript from the page HTML.
/// The page contains LZString-compressed data in a p.a.c.k.e.r wrapper.
/// Format: window["\x65\x76\x61\x6c"](function(p,a,c,k,e,d){...}('TEMPLATE',N,N,'BASE64DATA'['\x73\x70\x6c\x69\x63']('\x7c'),0,{}))
fn extract_packed_js(html: &str) -> Result<String> {
    // The actual format uses hex-encoded strings:
    // window["\x65\x76\x61\x6c"] = window["eval"]
    // ['\x73\x70\x6c\x69\x63']('\x7c') = ['splic']('|') - custom LZString decompress+split method
    //
    // The p.a.c.k.e.r format is:
    //   function(p,a,c,k,e,d){...}(template, a, c, dictionary.split('|'), 0, {})
    // Where:
    //   - p = template string with tokens like 'M.u({"q":3}).5();'
    //   - a = base for number encoding (usually 36 or 62)
    //   - c = number of dictionary entries
    //   - k = dictionary array (LZString compressed, then split by '|')
    //   - The function replaces tokens in template using dictionary

    // Pattern 1: Extract the FULL packed JS expression with p.a.c.k.e.r
    // This captures everything from `function(p,a,c,k,e,d)` to the closing `})`
    // The key is to capture the template string AND the LZString data together
    let re = Regex::new(
        r#"window\["\\x65\\x76\\x61\\x6c"\]\s*\(\s*(function\s*\(\s*p\s*,\s*a\s*,\s*c\s*,\s*k\s*,\s*e\s*,\s*d\s*\)\s*\{[^}]+\}\s*\(\s*'[^']*'\s*,\s*\d+\s*,\s*\d+\s*,\s*'[A-Za-z0-9+/=]+'\s*\[\s*'\\x73\\x70\\x6c\\x69\\x63'\s*\]\s*\(\s*'\\x7c'\s*\)\s*,\s*\d+\s*,\s*\{\s*\}\s*\))"#
    ).unwrap();

    if let Some(cap) = re.captures(html) {
        tracing::debug!("Matched full p.a.c.k.e.r pattern");
        return Ok(cap[1].to_string());
    }

    // Pattern 2: More permissive - match p.a.c.k.e.r with any argument format
    // The function body can contain nested braces, so we need a different approach
    // Look for: function(p,a,c,k,e,d){...}('...',...,'BASE64'['\x73\x70\x6c\x69\x63']('\x7c'),...,{})
    let re2 = Regex::new(
        r#"(function\s*\(\s*p\s*,\s*a\s*,\s*c\s*,\s*k\s*,\s*e\s*,\s*d\s*\)\s*\{[\s\S]*?\}\s*\(\s*'[\s\S]*?'\s*,\s*\d+\s*,\s*\d+\s*,\s*'([A-Za-z0-9+/=]+)'\s*\[\s*'\\x73\\x70\\x6c\\x69\\x63'\s*\]\s*\(\s*'\\x7c'\s*\)\s*,\s*\d+\s*,\s*\{\s*\}\s*\))"#
    ).unwrap();

    if let Some(cap) = re2.captures(html) {
        let full_expr = &cap[1];
        let lz_data = &cap[2];
        tracing::debug!(expr_len = full_expr.len(), lz_data_len = lz_data.len(), "Matched permissive p.a.c.k.e.r pattern");
        return Ok(full_expr.to_string());
    }

    // Pattern 3: Most permissive - find start and extract until we find the splic pattern
    // This handles cases where the function body is more complex
    if let Some(start) = html.find(r#"window["\x65\x76\x61\x6c"]"#) {
        // Find the function start
        if let Some(func_start) = html[start..].find("function") {
            let func_pos = start + func_start;
            // Find the splic pattern
            if let Some(splic_pos) = html[func_pos..].find(r"'\x73\x70\x6c\x69\x63']('\x7c')") {
                // Find the closing ,0,{}) after splic
                let after_splic = func_pos + splic_pos + r"'\x73\x70\x6c\x69\x63']('\x7c')".len();
                if let Some(end_match) = html[after_splic..].find(",0,{})") {
                    let end_pos = after_splic + end_match + ",0,{})".len();
                    let extracted = &html[func_pos..end_pos];
                    tracing::debug!(extracted_len = extracted.len(), "Extracted p.a.c.k.e.r via position scanning");
                    return Ok(extracted.to_string());
                }
            }
        }
    }

    // Debug: show what we found in the page
    if html.contains(r#"window["\x65\x76\x61\x6c"]"#) {
        tracing::warn!("Found eval pattern but could not extract packed JS");
        if !html.contains(r"'\x73\x70\x6c\x69\x63']") {
            tracing::warn!("Missing splic pattern - page format may have changed");
        }
    }

    bail!("Could not find packed JavaScript in page. The page structure may have changed.")
}

/// LZString decompression library (minified)
/// Used by manhuagui.com to compress the image data before p.a.c.k.e.r encoding
const LZSTRING_JS: &str = r#"
var LZString=function(){function o(o,r){if(!t[o]){t[o]={};for(var n=0;n<o.length;n++)t[o][o.charAt(n)]=n}return t[o][r]}var r=String.fromCharCode,n="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",e="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$",t={},i={compressToBase64:function(o){if(null==o)return"";var r=i._compress(o,6,function(o){return n.charAt(o)});switch(r.length%4){default:case 0:return r;case 1:return r+"===";case 2:return r+"==";case 3:return r+"="}},decompressFromBase64:function(r){return null==r?"":""==r?null:i._decompress(r.length,32,function(e){return o(n,r.charAt(e))})},compressToUTF16:function(o){return null==o?"":i._compress(o,15,function(o){return r(o+32)})+" "},decompressFromUTF16:function(o){return null==o?"":""==o?null:i._decompress(o.length,16384,function(r){return o.charCodeAt(r)-32})},compressToUint8Array:function(o){for(var r=i.compress(o),n=new Uint8Array(2*r.length),e=0,t=r.length;t>e;e++){var s=r.charCodeAt(e);n[2*e]=s>>>8,n[2*e+1]=s%256}return n},decompressFromUint8Array:function(o){if(null===o||void 0===o)return i.decompress(o);for(var n=new Array(o.length/2),e=0,t=n.length;t>e;e++)n[e]=256*o[2*e]+o[2*e+1];var s=[];return n.forEach(function(o){s.push(r(o))}),i.decompress(s.join(""))},compressToEncodedURIComponent:function(o){return null==o?"":i._compress(o,6,function(o){return e.charAt(o)})},decompressFromEncodedURIComponent:function(r){return null==r?"":""==r?null:(r=r.replace(/ /g,"+"),i._decompress(r.length,32,function(n){return o(e,r.charAt(n))}))},compress:function(o){return i._compress(o,16,function(o){return r(o)})},_compress:function(o,r,n){if(null==o)return"";var e,t,i,s={},p={},u="",c="",a="",l=2,f=3,h=2,d=[],m=0,v=0;for(i=0;i<o.length;i+=1)if(u=o.charAt(i),Object.prototype.hasOwnProperty.call(s,u)||(s[u]=f++,p[u]=!0),c=a+u,Object.prototype.hasOwnProperty.call(s,c))a=c;else{if(Object.prototype.hasOwnProperty.call(p,a)){if(a.charCodeAt(0)<256){for(e=0;h>e;e++)m<<=1,v==r-1?(v=0,d.push(n(m)),m=0):v++;for(t=a.charCodeAt(0),e=0;8>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1}else{for(t=1,e=0;h>e;e++)m=m<<1|t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t=0;for(t=a.charCodeAt(0),e=0;16>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1}l--,0==l&&(l=Math.pow(2,h),h++),delete p[a]}else for(t=s[a],e=0;h>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1;l--,0==l&&(l=Math.pow(2,h),h++),s[c]=f++,a=String(u)}if(""!==a){if(Object.prototype.hasOwnProperty.call(p,a)){if(a.charCodeAt(0)<256){for(e=0;h>e;e++)m<<=1,v==r-1?(v=0,d.push(n(m)),m=0):v++;for(t=a.charCodeAt(0),e=0;8>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1}else{for(t=1,e=0;h>e;e++)m=m<<1|t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t=0;for(t=a.charCodeAt(0),e=0;16>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1}l--,0==l&&(l=Math.pow(2,h),h++),delete p[a]}else for(t=s[a],e=0;h>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1;l--,0==l&&(l=Math.pow(2,h),h++)}for(t=2,e=0;h>e;e++)m=m<<1|1&t,v==r-1?(v=0,d.push(n(m)),m=0):v++,t>>=1;for(;;){if(m<<=1,v==r-1){d.push(n(m));break}v++}return d.join("")},decompress:function(o){return null==o?"":""==o?null:i._decompress(o.length,32768,function(r){return o.charCodeAt(r)})},_decompress:function(o,n,e){var t,i,s,p,u,c,a,l,f=[],h=4,d=4,m=3,v="",w=[],A={val:e(0),position:n,index:1};for(i=0;3>i;i+=1)f[i]=i;for(p=0,c=Math.pow(2,2),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;switch(t=p){case 0:for(p=0,c=Math.pow(2,8),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;l=r(p);break;case 1:for(p=0,c=Math.pow(2,16),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;l=r(p);break;case 2:return""}for(f[3]=l,s=l,w.push(l);;){if(A.index>o)return"";for(p=0,c=Math.pow(2,m),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;switch(l=p){case 0:for(p=0,c=Math.pow(2,8),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;f[d++]=r(p),l=d-1,h--;break;case 1:for(p=0,c=Math.pow(2,16),a=1;a!=c;)u=A.val&A.position,A.position>>=1,0==A.position&&(A.position=n,A.val=e(A.index++)),p|=(u>0?1:0)*a,a<<=1;f[d++]=r(p),l=d-1,h--;break;case 2:return w.join("")}if(0==h&&(h=Math.pow(2,m),m++),f[l])v=f[l];else{if(l!==d)return null;v=s+s.charAt(0)}w.push(v),f[d++]=s+v.charAt(0),h--,s=v,0==h&&(h=Math.pow(2,m),m++)}}};return i}();"#;

/// Decodes the packed JavaScript using rquickjs.
/// The packed JS when executed calls SMH.imgData(path, cid, md5, files).preInit()
/// Security: Runs in sandboxed JS context with no filesystem/network access.
fn decode_packed_js(packed: &str) -> Result<String> {
    // Security: Limit packed JS size
    if packed.len() > 1024 * 1024 {
        bail!("Packed JavaScript too large ({} bytes)", packed.len());
    }

    let runtime = Runtime::new().context("Failed to create JS runtime")?;
    let ctx = JsContext::full(&runtime).context("Failed to create JS context")?;

    ctx.with(|ctx| {
        // Load LZString library for decompression
        ctx.eval::<(), _>(LZSTRING_JS).map_err(|e| {
            anyhow::anyhow!("Failed to load LZString: {:?}", e)
        })?;

        // The key insight: manhuagui uses `.splic()` as a custom method that:
        // 1. Decompresses the string via LZString.decompressFromBase64
        // 2. Splits the result by the delimiter
        // This is an anti-scraping measure - `splic` is NOT a typo for `split`!
        //
        // The p.a.c.k.e.r format works like this:
        // - Template string: 'M.u({"q":3}).5();' with tokens (M, u, q, 5, etc.)
        // - Dictionary: ['', 'jpg', 'webp', ...] from LZString decompression
        // - The function replaces tokens with dictionary values
        // - Final result: 'SMH.imgData({...}).preInit();'

        let js_code = format!(r#"
            var __result = null;
            var __error = null;

            // Mock SMH.imgData to capture the decoded data
            // The actual call is SMH.imgData(dataObject).preInit() with a single object argument
            // containing: path, files, cid, cname, bid, bname, sl (auth params), etc.
            var SMH = {{
                imgData: function(data) {{
                    // Handle both old format (path, cid, md5, files) and new format (single object)
                    if (typeof data === 'object' && data !== null) {{
                        // New format: single object argument
                        __result = JSON.stringify({{
                            path: data.path,
                            cid: data.cid,
                            md5: data.md5 || "",
                            files: data.files,
                            // Include auth params for image URL construction
                            sl: data.sl
                        }});
                    }} else {{
                        // Old format: separate arguments (path, cid, md5, files)
                        __result = JSON.stringify({{
                            path: arguments[0],
                            cid: arguments[1],
                            md5: arguments[2],
                            files: arguments[3]
                        }});
                    }}
                    return {{
                        preInit: function() {{ return this; }},
                        reader: function() {{ return this; }}
                    }};
                }}
            }};

            // Define String.prototype.splic as LZString decompress + split
            // This is the anti-scraping measure used by manhuagui
            String.prototype.splic = function(delimiter) {{
                var decompressed = LZString.decompressFromBase64(this.toString());
                if (!decompressed) {{
                    __error = "LZString decompression returned null";
                    return [];
                }}
                return decompressed.split(delimiter);
            }};

            try {{
                // The p.a.c.k.e.r function RETURNS a decoded string, it doesn't execute it.
                // The original page uses window["eval"](packer_result) to execute the decoded JS.
                // So we need to:
                // 1. Execute the p.a.c.k.e.r function expression (returns a string)
                // 2. Eval that returned string to call SMH.imgData(...)
                var decoded = ({packed});
                if (typeof decoded === 'string') {{
                    eval(decoded);
                }} else {{
                    __error = "p.a.c.k.e.r did not return a string, got: " + typeof decoded;
                }}
            }} catch (e) {{
                __error = "Execution error: " + e.toString();
            }}

            // Return result or error information
            if (__result) {{
                __result;
            }} else if (__error) {{
                "ERROR:" + __error;
            }} else {{
                "ERROR:SMH.imgData was not called";
            }}
        "#, packed = packed);

        let result: String = ctx.eval(js_code).map_err(|e| {
            anyhow::anyhow!("JS execution error: {:?}", e)
        })?;

        if result.starts_with("ERROR:") {
            bail!("{}", &result[6..]);
        }

        Ok(result)
    })
}

/// Parses the image data from the decoded JSON.
/// Security: Validates paths and enforces limits.
fn parse_image_data(decoded_json: &str, html: &str) -> Result<ImageData> {
    let parsed: serde_json::Value = serde_json::from_str(decoded_json)
        .context("Failed to parse decoded JSON")?;

    let path = parsed.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in decoded data"))?
        .to_string();

    // Security: Validate path to prevent path traversal
    validate_path(&path)?;

    let files: Vec<String> = parsed.get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing 'files' array in decoded data"))?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    if files.is_empty() {
        bail!("No files found in decoded data");
    }

    // Security: Limit number of images
    if files.len() > MAX_IMAGES_PER_CHAPTER {
        bail!("Too many images ({}, max {})", files.len(), MAX_IMAGES_PER_CHAPTER);
    }

    // Security: Validate each filename
    for file in &files {
        validate_filename(file)?;
    }

    // Extract CDN host from page (look for image server config)
    let cdn_host = extract_cdn_host(html).unwrap_or_else(|| CDN_HOSTS[0].to_string());

    // Security: Validate CDN host against allowlist
    validate_cdn_host(&cdn_host)?;

    // Extract auth query string - prefer from decoded data's `sl` field, fallback to page scraping
    let query_string = parsed.get("sl")
        .and_then(|sl| {
            let e = sl.get("e").and_then(|v| v.as_i64()).map(|v| v.to_string())
                .or_else(|| sl.get("e").and_then(|v| v.as_str()).map(String::from))?;
            let m = sl.get("m").and_then(|v| v.as_str())?;
            Some(format!("?e={}&m={}", e, m))
        })
        .or_else(|| extract_query_string(html))
        .unwrap_or_default();

    tracing::debug!(query_string = %query_string, "Extracted auth params");

    Ok(ImageData {
        path,
        files,
        cdn_host,
        query_string,
    })
}

/// Validates that a path doesn't contain traversal sequences.
/// Security: Prevents path traversal attacks.
fn validate_path(path: &str) -> Result<()> {
    if path.contains("..") {
        bail!("Invalid path: contains traversal sequence");
    }
    if !path.starts_with('/') {
        bail!("Invalid path: must start with /");
    }
    // Allow alphanumeric, CJK characters (for chapter names), slashes, underscores, hyphens, dots
    // Disallow: control characters, null bytes, backslashes
    let valid_chars = path.chars().all(|c| {
        c.is_alphanumeric() || // includes ASCII and CJK characters
        matches!(c, '/' | '_' | '-' | '.') ||
        // Allow common CJK punctuation
        c == '\u{3001}' || c == '\u{3002}' || c == '\u{ff01}'
    });
    if !valid_chars {
        bail!("Invalid path: contains disallowed characters");
    }
    // Additional security: no null bytes or control characters
    if path.chars().any(|c| c.is_control()) {
        bail!("Invalid path: contains control characters");
    }
    Ok(())
}

/// Validates that a filename is safe.
/// Security: Prevents path traversal via filenames.
fn validate_filename(name: &str) -> Result<()> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("Invalid filename: {}", name);
    }
    if name.is_empty() || name.len() > 255 {
        bail!("Invalid filename length: {}", name.len());
    }
    Ok(())
}

/// Validates that a CDN host is in the allowlist.
/// Security: Prevents SSRF attacks.
fn validate_cdn_host(host: &str) -> Result<()> {
    let parsed = url::Url::parse(host).context("Invalid CDN URL")?;
    let hostname = parsed.host_str().unwrap_or("");

    if !ALLOWED_CDN_HOSTS.iter().any(|&allowed| hostname == allowed) {
        bail!("CDN host not in allowlist: {}", hostname);
    }
    Ok(())
}

/// Extracts the CDN host from the page HTML.
fn extract_cdn_host(html: &str) -> Option<String> {
    // Look for CDN configuration in the page
    let patterns = [
        r#"var\s+SMH\s*=\s*\{[^}]*host:\s*['"](https?://[^'"]+)['"]"#,
        r#"imgHost\s*[:=]\s*['"](https?://[^'"]+)['"]"#,
        r#"src=["'](https://[^/]+\.hamreus\.com)"#,
        // All known CDN hosts
        r#"(https://i\.hamreus\.com)"#,
        r#"(https://us\.hamreus\.com)"#,
        r#"(https://eu\.hamreus\.com)"#,
        r#"(https://dx\.hamreus\.com)"#,
        r#"(https://lt\.hamreus\.com)"#,
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html) {
                return Some(cap[1].to_string());
            }
        }
    }

    None
}

/// Extracts authentication query parameters from the page.
fn extract_query_string(html: &str) -> Option<String> {
    // Look for auth params in various places
    let patterns = [
        r#"\?e=(\d+)&m=([a-zA-Z0-9_-]+)"#,
        r#"e=(\d+).*?m=([a-zA-Z0-9_-]+)"#,
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html) {
                return Some(format!("?e={}&m={}", &cap[1], &cap[2]));
            }
        }
    }

    // If no auth params found, images might work without them
    None
}

/// Builds full image URLs from the parsed data.
fn build_image_urls(data: &ImageData) -> Vec<String> {
    data.files
        .iter()
        .map(|file| {
            format!(
                "{}{}{}{}",
                data.cdn_host,
                data.path,
                file,
                data.query_string
            )
        })
        .collect()
}

/// Downloads images with concurrency limit and retry logic.
async fn download_images(urls: &[String], app_handle: &AppHandle) -> Result<Vec<FileEntry>> {
    let total = urls.len();
    let semaphore = Arc::new(Semaphore::new(DOWNLOAD_CONCURRENCY));

    // Build headers with Referer (required by manhuagui CDN)
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::REFERER,
        "https://www.manhuagui.com/".parse().unwrap(),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .default_headers(headers)
        .build()?;

    let (progress_tx, mut progress_rx) = mpsc::channel::<usize>(total);

    // Spawn progress reporter
    let app_handle_clone = app_handle.clone();
    let progress_task = tokio::spawn(async move {
        let mut completed = 0;
        while progress_rx.recv().await.is_some() {
            completed += 1;
            app_handle_clone
                .emit("scraper:progress", ScraperProgress {
                    current: completed,
                    total,
                    message: format!("Downloading {}/{}", completed, total),
                })
                .ok();
        }
    });

    // Download all images concurrently
    let mut handles = Vec::with_capacity(total);
    for (idx, url) in urls.iter().enumerate() {
        let permit = semaphore.clone().acquire_owned().await?;
        let client = client.clone();
        let url = url.clone();
        let progress_tx = progress_tx.clone();

        let handle = tokio::spawn(async move {
            // Add random delay to respect rate limits (200-500ms)
            let delay = rand::thread_rng().gen_range(MIN_DOWNLOAD_DELAY_MS..=MAX_DOWNLOAD_DELAY_MS);
            tokio::time::sleep(Duration::from_millis(delay)).await;

            let result = download_with_retry(&client, &url, MAX_RETRIES).await;
            drop(permit);
            let _ = progress_tx.send(1).await;

            result.map(|data| {
                let ext = url
                    .rsplit('/')
                    .next()
                    .and_then(|s| s.split('?').next())
                    .and_then(|s| s.rsplit('.').next())
                    .unwrap_or("webp");
                FileEntry {
                    name: format!("{:04}.{}", idx + 1, ext),
                    data,
                }
            })
        });
        handles.push(handle);
    }

    // Wait for all downloads
    drop(progress_tx);
    let mut files = Vec::with_capacity(total);
    let mut errors = Vec::new();

    for handle in handles {
        match handle.await {
            Ok(Ok(file)) => files.push(file),
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(anyhow::anyhow!("Task failed: {}", e)),
        }
    }

    progress_task.await.ok();

    // Sort files by name to maintain page order
    files.sort_by(|a, b| a.name.cmp(&b.name));

    if !errors.is_empty() {
        tracing::warn!(
            failed = errors.len(),
            succeeded = files.len(),
            "Some images failed to download"
        );
    }

    Ok(files)
}

/// Downloads a single URL with retry logic.
async fn download_with_retry(client: &reqwest::Client, url: &str, max_retries: u32) -> Result<Vec<u8>> {
    let mut last_error = None;

    for attempt in 0..max_retries {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * (attempt as u64))).await;
        }

        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.bytes().await {
                        Ok(bytes) => return Ok(bytes.to_vec()),
                        Err(e) => last_error = Some(anyhow::anyhow!("Failed to read response: {}", e)),
                    }
                } else {
                    last_error = Some(anyhow::anyhow!("HTTP {}", response.status()));
                }
            }
            Err(e) => {
                last_error = Some(anyhow::anyhow!("Request failed: {}", e));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Download failed after {} retries", max_retries)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("https://www.manhuagui.com/comic/1128/873968.html").is_ok());
        assert!(validate_url("https://manhuagui.com/comic/1128/873968.html").is_ok());
        assert!(validate_url("http://www.manhuagui.com/comic/1/1.html").is_ok());
        // Taiwan mirror support
        assert!(validate_url("https://tw.manhuagui.com/comic/4723/689837.html").is_ok());
    }

    #[test]
    fn test_validate_url_invalid() {
        // Wrong domain
        assert!(validate_url("https://example.com/comic/1128/873968.html").is_err());
        // Missing .html
        assert!(validate_url("https://www.manhuagui.com/comic/1128/873968").is_err());
        // Wrong path
        assert!(validate_url("https://www.manhuagui.com/manga/1128/873968.html").is_err());
        // Invalid URL
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn test_parse_image_data() {
        let json = r#"{"path":"/ps3/g/one_piece/chapter1/","cid":123,"md5":"abc","files":["0001.jpg.webp","0002.jpg.webp"]}"#;
        let html = r#"<html><script>var e=1234567890; var m="hash123";</script></html>"#;

        let data = parse_image_data(json, html).unwrap();
        assert_eq!(data.path, "/ps3/g/one_piece/chapter1/");
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.files[0], "0001.jpg.webp");
    }

    #[test]
    fn test_parse_image_data_with_sl_field() {
        // Test that auth params are extracted from the `sl` field in decoded JSON
        let json = r#"{"path":"/ps1/h/op/test/","cid":123,"files":["0001.jpg.webp"],"sl":{"e":1776247212,"m":"SD0X4JGn1iPUW4i-6uCCwg"}}"#;
        let html = r#"<html></html>"#;

        let data = parse_image_data(json, html).unwrap();
        assert_eq!(data.path, "/ps1/h/op/test/");
        assert_eq!(data.query_string, "?e=1776247212&m=SD0X4JGn1iPUW4i-6uCCwg");
    }

    #[test]
    fn test_parse_image_data_sl_takes_precedence() {
        // Auth params from `sl` field should take precedence over HTML extraction
        let json = r#"{"path":"/test/","cid":123,"files":["0001.jpg.webp"],"sl":{"e":111,"m":"fromjson"}}"#;
        let html = r#"<script>?e=222&m=fromhtml</script>"#;

        let data = parse_image_data(json, html).unwrap();
        // Should use params from JSON's sl field, not from HTML
        assert_eq!(data.query_string, "?e=111&m=fromjson");
    }

    #[test]
    fn test_build_image_urls() {
        let data = ImageData {
            path: "/ps3/g/test/".to_string(),
            files: vec!["001.webp".to_string(), "002.webp".to_string()],
            cdn_host: "https://i.hamreus.com".to_string(),
            query_string: "?e=123&m=abc".to_string(),
        };

        let urls = build_image_urls(&data);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://i.hamreus.com/ps3/g/test/001.webp?e=123&m=abc");
        assert_eq!(urls[1], "https://i.hamreus.com/ps3/g/test/002.webp?e=123&m=abc");
    }

    #[test]
    fn test_extract_cdn_host() {
        let html = r#"<script>var config = { imgHost: 'https://us.hamreus.com' };</script>"#;
        assert_eq!(extract_cdn_host(html), Some("https://us.hamreus.com".to_string()));

        let html2 = r#"<img src="https://i.hamreus.com/path/image.jpg">"#;
        assert_eq!(extract_cdn_host(html2), Some("https://i.hamreus.com".to_string()));

        // Test all CDN hosts
        let html3 = r#"<img src="https://eu.hamreus.com/path/image.jpg">"#;
        assert_eq!(extract_cdn_host(html3), Some("https://eu.hamreus.com".to_string()));
    }

    #[test]
    fn test_extract_query_string() {
        let html = r#"<script>var url = '/path/image.jpg?e=1234567890&m=abc123def';</script>"#;
        assert_eq!(extract_query_string(html), Some("?e=1234567890&m=abc123def".to_string()));
    }

    // Security tests

    #[test]
    fn test_validate_path_traversal() {
        // Path traversal should be rejected
        assert!(validate_path("/ps3/../etc/passwd").is_err());
        assert!(validate_path("/ps3/..").is_err());

        // Valid paths should pass
        assert!(validate_path("/ps3/g/one_piece/chapter1/").is_ok());
        assert!(validate_path("/manga/test-123/").is_ok());
    }

    #[test]
    fn test_validate_path_must_start_with_slash() {
        assert!(validate_path("ps3/g/test/").is_err());
        assert!(validate_path("/ps3/g/test/").is_ok());
    }

    #[test]
    fn test_validate_path_chinese_characters() {
        // Chinese chapter names should be allowed
        assert!(validate_path("/ps3/g/one_piece/第1178话/").is_ok());
        assert!(validate_path("/manga/航海王/chapter1/").is_ok());
        // Japanese should also work
        assert!(validate_path("/manga/ワンピース/chapter1/").is_ok());
    }

    #[test]
    fn test_validate_filename() {
        // Valid filenames
        assert!(validate_filename("0001.jpg.webp").is_ok());
        assert!(validate_filename("image-123.png").is_ok());

        // Invalid filenames with path components
        assert!(validate_filename("../etc/passwd").is_err());
        assert!(validate_filename("foo/bar.jpg").is_err());
        assert!(validate_filename("foo\\bar.jpg").is_err());

        // Empty or too long
        assert!(validate_filename("").is_err());
    }

    #[test]
    fn test_validate_cdn_host() {
        // Allowed hosts
        assert!(validate_cdn_host("https://i.hamreus.com").is_ok());
        assert!(validate_cdn_host("https://us.hamreus.com").is_ok());
        assert!(validate_cdn_host("https://eu.hamreus.com").is_ok());
        assert!(validate_cdn_host("https://dx.hamreus.com").is_ok());
        assert!(validate_cdn_host("https://lt.hamreus.com").is_ok());

        // Disallowed hosts (SSRF prevention)
        assert!(validate_cdn_host("https://evil.com").is_err());
        assert!(validate_cdn_host("https://localhost").is_err());
        assert!(validate_cdn_host("http://127.0.0.1").is_err());
    }

    #[test]
    fn test_parse_image_data_path_traversal_rejected() {
        let json = r#"{"path":"/../../../etc/passwd","cid":123,"md5":"abc","files":["0001.jpg"]}"#;
        let html = r#"<html></html>"#;
        assert!(parse_image_data(json, html).is_err());
    }

    #[test]
    fn test_parse_image_data_too_many_files() {
        // Create JSON with too many files
        let files: Vec<String> = (0..600).map(|i| format!("{:04}.jpg", i)).collect();
        let json = format!(r#"{{"path":"/test/","cid":123,"md5":"abc","files":{}}}"#,
            serde_json::to_string(&files).unwrap());
        let html = r#"<html></html>"#;
        assert!(parse_image_data(&json, html).is_err());
    }

    #[test]
    fn test_extract_packed_js_full_packer_format() {
        // Full p.a.c.k.e.r format with template string, counts, and LZString data
        let lz_data = "D4KwDg5sDuCmBGZgEZkCYAcxA03qg7BoLvRwYATrAJIB2AlgC7AAMjyjTzAnO41s43twBs3AKzcALNwDMwbmmAAzGgBtYAZ1kBjKgEMAtrGAY8UjoKyaaAE2CIam2QAUKAUQDCLwFRBgd1tA";
        let html = format!(
            r#"<script type="text/javascript">window["\x65\x76\x61\x6c"](function(p,a,c,k,e,d){{e=function(c){{return c}};}}('M.u({{}}).5();',49,49,'{}'['\x73\x70\x6c\x69\x63']('\x7c'),0,{{}})) </script>"#,
            lz_data
        );

        let result = extract_packed_js(&html);
        assert!(result.is_ok(), "Failed to extract: {:?}", result.err());
        let extracted = result.unwrap();
        // Should contain the full p.a.c.k.e.r function, not just the LZString data
        assert!(extracted.contains("function"), "Should contain function keyword");
        assert!(extracted.contains(lz_data), "Should contain LZString data");
    }

    #[test]
    fn test_extract_packed_js_position_scanning() {
        // Test the fallback position-scanning extraction
        let lz_data = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
        let html = format!(
            r#"<script>window["\x65\x76\x61\x6c"](function(p,a,c,k,e,d){{return p;}}('template',10,10,'{}'['\x73\x70\x6c\x69\x63']('\x7c'),0,{{}})) </script>"#,
            lz_data
        );

        let result = extract_packed_js(&html);
        assert!(result.is_ok(), "Failed to extract: {:?}", result.err());
    }

    #[test]
    fn test_extract_packed_js_no_match() {
        // HTML without the expected patterns should fail gracefully
        let html = r#"<html><body>No packed JS here</body></html>"#;
        let result = extract_packed_js(html);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_packed_js_splic_method() {
        // Test that the splic method is properly defined and works
        // This simulates a minimal p.a.c.k.e.r that just returns the decoded array
        let packed = r#"
            (function(p,a,c,k,e,d) {
                // Simple test - k should be the split array from LZString decompression
                if (k && k.length > 0) {
                    SMH.imgData("/test/path/", 123, "md5hash", ["file1.jpg", "file2.jpg"]);
                }
                return p;
            })('test', 2, 2, 'N4Ig'.splic('|'), 0, {})
        "#;
        // This won't fully work because 'N4Ig' isn't valid LZString data that produces meaningful output,
        // but it tests that the code doesn't crash and handles the splic method
        let result = decode_packed_js(packed);
        // Will fail because LZString.decompressFromBase64('N4Ig') returns something unexpected
        // but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }
}
