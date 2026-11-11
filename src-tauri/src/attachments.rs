//! Files and web pages the user attaches to a message (the composer's + menu): images the agent sees, text files and
//! pages it reads. Limits keep a message small enough to send.

use crate::agent::Attachment;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use std::path::Path;
use std::time::Duration;

const MAX_IMAGE: u64 = 5 * 1024 * 1024;
const MAX_TEXT: u64 = 200 * 1024;
const MAX_PAGE_CHARS: usize = 30_000;

fn image_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// A file from this computer: an image (up to 5 MB) or a text file (up to 200 KB).
pub fn read_file(path: &Path) -> Result<Attachment, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{name}: {e}"))?
        .len();
    if let Some(media_type) = image_type(path) {
        if size > MAX_IMAGE {
            return Err(format!("{name} is over 5 MB; pick a smaller image."));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        return Ok(Attachment {
            name,
            kind: "image".into(),
            media_type: media_type.into(),
            data: B64.encode(bytes),
        });
    }
    if size > MAX_TEXT {
        return Err(format!(
            "{name} is over 200 KB; attach a smaller part, or mention it from the project."
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        format!("{name} is not text or an image (PNG, JPEG, GIF, WebP); it cannot be attached yet.")
    })?;
    Ok(Attachment {
        name,
        kind: "text".into(),
        media_type: String::new(),
        data: text,
    })
}

/// A web page as text (title and readable text, scripts and styles left out), for any agent to read.
pub fn read_page(url: &str) -> Result<Attachment, String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Give a web address starting with https:// or http://.".into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build(),
    );
    let mut res = agent
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Qanuvi page reader)")
        .call()
        .map_err(|e| format!("{url} could not be reached: {e}"))?;
    let status = res.status().as_u16();
    if status >= 400 {
        return Err(format!("{url} answered {status}."));
    }
    let html = res
        .body_mut()
        .with_config()
        .limit(5 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("{url} could not be read: {e}"))?;
    let (title, text) = page_text(&html);
    if text.trim().is_empty() {
        return Err("The page has no readable text (it may need a browser to show it).".into());
    }
    let name = if title.is_empty() {
        url.to_string()
    } else {
        format!("{title} ({url})")
    };
    Ok(Attachment {
        name,
        kind: "page".into(),
        media_type: String::new(),
        data: text,
    })
}

/// The title and readable text of an HTML page.
pub fn page_text(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .and_then(|start| {
            lower[start..]
                .find("</title>")
                .map(|end| html[start..start + end].trim().to_string())
        })
        .unwrap_or_default();
    // Drop scripts, styles and the like with their content, then the tags.
    let mut out = String::with_capacity(html.len() / 3);
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &lower[i..];
            let skip = ["script", "style", "noscript", "svg", "template"]
                .iter()
                .find(|t| rest.starts_with(&format!("<{t}")));
            if let Some(t) = skip {
                let close = format!("</{t}>");
                i = rest
                    .find(&close)
                    .map_or(bytes.len(), |e| i + e + close.len());
                out.push(' ');
                continue;
            }
            let block = [
                "<p", "<br", "<div", "<li", "<h1", "<h2", "<h3", "<h4", "<tr", "</p", "</div",
                "</li",
            ]
            .iter()
            .any(|t| rest.starts_with(t));
            i = rest.find('>').map_or(bytes.len(), |e| i + e + 1);
            out.push(if block { '\n' } else { ' ' });
            continue;
        }
        let next = lower[i..].find('<').map_or(bytes.len(), |e| i + e);
        out.push_str(&html[i..next]);
        i = next;
    }
    let text = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    let mut text = lines.join("\n");
    if text.len() > MAX_PAGE_CHARS {
        let mut cut = MAX_PAGE_CHARS;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n… (the rest of the page is left out)");
    }
    (title, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_become_readable_text() {
        let html =
            "<html><head><title>Shop</title><style>p{}</style><script>alert(1)</script></head>
            <body><h1>Prices</h1><p>Milk &amp; bread</p><div>Open <b>daily</b></div></body></html>";
        let (title, text) = page_text(html);
        assert_eq!(title, "Shop");
        assert!(
            text.contains("Prices") && text.contains("Milk & bread") && text.contains("Open daily")
        );
        assert!(!text.contains("alert") && !text.contains("p{}"));
    }

    #[test]
    fn files_are_images_or_text_within_limits() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png");
        std::fs::write(&png, [137u8, 80, 78, 71]).unwrap();
        let img = read_file(&png).unwrap();
        assert_eq!(
            (img.kind.as_str(), img.media_type.as_str()),
            ("image", "image/png")
        );
        let txt = dir.path().join("n.txt");
        std::fs::write(&txt, "milk").unwrap();
        assert_eq!(read_file(&txt).unwrap().data, "milk");
        let bin = dir.path().join("x.bin");
        std::fs::write(&bin, [0u8, 159, 146, 150]).unwrap();
        assert!(read_file(&bin).is_err());
        assert!(read_page("ftp://x").is_err());
    }
}
