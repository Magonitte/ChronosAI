use base64::{engine::general_purpose::STANDARD, Engine};
use std::process::Command;

/// Capture the screen on Windows using PowerShell.
/// Returns the screenshot as a base64-encoded JPEG (resized to max 1280px).
pub fn take_screenshot(_monitor: Option<u32>) -> Result<String, String> {
    let tmp_raw = std::env::temp_dir().join("voice-assistant-screenshot-raw.png");
    let tmp_jpeg = std::env::temp_dir().join("voice-assistant-screenshot.jpg");
    let raw_str = tmp_raw.to_string_lossy().replace('\\', "\\\\");
    let _jpeg_str = tmp_jpeg.to_string_lossy().replace('\\', "\\\\");

    // PowerShell script to capture screen and save as PNG
    let ps_script = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$screen = [System.Windows.Forms.Screen]::PrimaryScreen
$bitmap = New-Object System.Drawing.Bitmap($screen.Bounds.Width, $screen.Bounds.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($screen.Bounds.X, $screen.Bounds.Y, 0, 0, $screen.Bounds.Size)
$bitmap.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
"#,
        raw_str
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .status()
        .map_err(|e| format!("Failed to run screenshot: {}", e))?;

    if !status.success() {
        return Err("Screenshot capture failed".to_string());
    }

    // Resize using the image crate and convert to JPEG
    let img = image::open(&tmp_raw)
        .map_err(|e| format!("Failed to open screenshot: {}", e))?;

    let (w, h) = (img.width(), img.height());
    let max_dim: u32 = 1280;
    let resized = if w > max_dim || h > max_dim {
        if w > h {
            let new_h = (h as f64 * max_dim as f64 / w as f64) as u32;
            img.resize(max_dim, new_h, image::imageops::FilterType::Lanczos3)
        } else {
            let new_w = (w as f64 * max_dim as f64 / h as f64) as u32;
            img.resize(new_w, max_dim, image::imageops::FilterType::Lanczos3)
        }
    } else {
        img
    };

    resized
        .save(&tmp_jpeg)
        .map_err(|e| format!("Failed to save JPEG: {}", e))?;

    let _ = std::fs::remove_file(&tmp_raw);

    let bytes = std::fs::read(&tmp_jpeg)
        .map_err(|e| format!("Failed to read screenshot JPEG: {}", e))?;
    let _ = std::fs::remove_file(&tmp_jpeg);

    Ok(STANDARD.encode(&bytes))
}

/// Describe a screenshot image using llama.cpp's OpenAI-compatible vision API.
pub async fn describe_screenshot(
    llm_url: &str,
    model: &str,
    image_b64: &str,
    question: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    // OpenAI-compatible vision format
    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": question
                },
                {
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/jpeg;base64,{}", image_b64)
                    }
                }
            ]
        }],
        "stream": false
    });

    let resp = client
        .post(format!("{}/v1/chat/completions", llm_url))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Vision request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Vision API error {}: {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse vision response: {}", e))?;

    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Could not describe the screenshot.")
        .to_string())
}

/// Read the system clipboard text on Windows using PowerShell.
pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "Get-Clipboard -Format Text"])
        .output()
        .map_err(|e| format!("Failed to read clipboard: {}", e))?;

    if !output.status.success() {
        return Err("Could not read clipboard".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Clipboard not valid UTF-8: {}", e))
}

/// Open a URL in the default browser on Windows.
pub fn open_url(url: &str) -> Result<String, String> {
    let status = Command::new("cmd")
        .args(["/c", "start", "", url])
        .status()
        .map_err(|e| format!("Failed to open URL: {}", e))?;

    if status.success() {
        Ok(format!("Opened {} in the default browser.", url))
    } else {
        Err("Failed to open URL".to_string())
    }
}

/// Get current date, time, and day of week.
pub fn get_current_time() -> String {
    let now = chrono::Local::now();
    now.format("%A, %B %e, %Y at %I:%M %p").to_string()
}

/// Fetch a URL and return its text content (HTML stripped to readable text).
pub async fn web_fetch(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    // Strip HTML to plain text
    let text = strip_html(&html);

    // Truncate to avoid flooding context
    let max_len = 6000;
    if text.len() > max_len {
        Ok(format!("{}...\n(truncated, {} total chars)", &text[..max_len], text.len()))
    } else {
        Ok(text)
    }
}

/// Naive HTML-to-text: strip tags, decode common entities, collapse whitespace.
fn strip_html(html: &str) -> String {
    // Remove script and style blocks entirely
    let mut s = html.to_string();
    for tag in &["script", "style", "noscript", "svg"] {
        loop {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            if let Some(start) = s.to_lowercase().find(&open) {
                if let Some(end) = s.to_lowercase()[start..].find(&close) {
                    s = format!("{}{}", &s[..start], &s[start + end + close.len()..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Replace block elements with newlines
    let block_tags = ["</p>", "</div>", "</li>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>", "<br>", "<br/>", "<br />", "</tr>", "</blockquote>"];
    for tag in block_tags {
        s = s.replace(tag, "\n");
    }

    // Strip remaining tags
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // Decode common entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/");

    // Collapse whitespace: multiple spaces -> one, multiple newlines -> two
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_newline = 0;
    let mut prev_space = false;
    for ch in result.chars() {
        if ch == '\n' || ch == '\r' {
            prev_newline += 1;
            prev_space = false;
            if prev_newline <= 2 {
                cleaned.push('\n');
            }
        } else if ch.is_whitespace() {
            prev_newline = 0;
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            prev_newline = 0;
            prev_space = false;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

/// List running applications on Windows using PowerShell.
pub fn list_running_apps() -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
            "Get-Process | Where-Object {$_.MainWindowTitle -ne ''} | Select-Object -ExpandProperty MainWindowTitle | Sort-Object"
        ])
        .output()
        .map_err(|e| format!("Failed to list apps: {}", e))?;

    if !output.status.success() {
        return Err("Could not list running apps".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Output not valid UTF-8: {}", e))
}
