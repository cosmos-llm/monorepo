use cosmos_llm_virtual_filesystem::Filesystem;

use crate::builder::ContextBuilder;

/// The available rendering formats.
///
/// Each variant maps to a renderer that transforms a [`ContextBuilder`] into
/// a provider-appropriate string.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_context::{ContextBuilder, RenderFormat};
///
/// let out = ContextBuilder::new()
///     .block("system", "hi", Default::default())
///     .render(RenderFormat::Json);
///
/// assert!(out.contains("\"system\""));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderFormat {
    /// Plain text — content only, blocks separated by blank lines.
    Default,
    /// XML envelope compatible with Anthropic Claude.
    Xml,
    /// JSON array of `{name, content}` objects.
    Json,
    /// Anthropic-optimised: system blocks first, filesystem XML, then others.
    Anthropic,
    /// OpenAI-optimised: `ROLE:\ncontent` headers, filesystem as markdown.
    OpenAi,
}

impl RenderFormat {
    /// Renders `builder` according to this format.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::{ContextBuilder, RenderFormat};
    ///
    /// let ctx = ContextBuilder::new()
    ///     .block("user", "Hello", Default::default());
    ///
    /// let out = RenderFormat::Default.render(&ctx);
    /// assert!(out.contains("Hello"));
    /// ```
    pub fn render(&self, builder: &ContextBuilder) -> String {
        match self {
            Self::Default => render_default(builder),
            Self::Xml => render_xml(builder),
            Self::Json => render_json(builder),
            Self::Anthropic => render_anthropic(builder),
            Self::OpenAi => render_openai(builder),
        }
    }
}

fn render_default(builder: &ContextBuilder) -> String {
    let mut parts: Vec<String> = builder.blocks.iter().map(|b| b.content.clone()).collect();

    if let Some(fs) = &builder.filesystem {
        parts.push(fs.tree(0));
    }

    parts.join("\n\n")
}

fn render_xml(builder: &ContextBuilder) -> String {
    let mut out = vec!["<context>".to_string()];

    if let Some(fs) = &builder.filesystem {
        out.push(render_xml_filesystem(fs, 1));
    }

    for block in &builder.blocks {
        out.push(format!(
            "  <block type=\"{}\">\n    {}\n  </block>",
            escape_xml(&block.name),
            escape_xml(&block.content)
        ));
    }

    out.push("</context>".to_string());
    out.join("\n")
}

fn render_xml_filesystem(fs: &Filesystem, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let mut out = vec![format!(
        "{}<filesystem name=\"{}\">",
        pad,
        escape_xml(&fs.name)
    )];

    for file in &fs.files {
        out.push(format!("{}  <file name=\"{}\">", pad, escape_xml(&file.name)));
        if let Some(content) = &file.content {
            out.push(format!("{}    {}", pad, escape_xml(content)));
        }
        out.push(format!("{}  </file>", pad));
    }

    for child in &fs.children {
        out.push(render_xml_filesystem(child, indent + 1));
    }

    out.push(format!("{}</filesystem>", pad));
    out.join("\n")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn render_json(builder: &ContextBuilder) -> String {
    let items: Vec<serde_json::Value> = builder
        .blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "name": b.name,
                "content": b.content,
                "metadata": b.metadata,
            })
        })
        .collect();

    serde_json::to_string_pretty(&items).unwrap_or_default()
}

fn render_anthropic(builder: &ContextBuilder) -> String {
    let mut parts: Vec<String> = Vec::new();

    // System blocks first
    for b in builder.blocks.iter().filter(|b| b.is_type("system")) {
        parts.push(b.content.clone());
    }

    // Filesystem in XML
    if let Some(fs) = &builder.filesystem {
        parts.push("\n<filesystem>".to_string());
        parts.push(render_anthropic_filesystem(fs, ""));
        parts.push("</filesystem>".to_string());
    }

    // Remaining blocks
    for b in builder.blocks.iter().filter(|b| !b.is_type("system")) {
        parts.push(format!("\n<{}>", b.name));
        parts.push(b.content.clone());
        parts.push(format!("</{}>", b.name));
    }

    parts.join("\n")
}

fn render_anthropic_filesystem(fs: &Filesystem, prefix: &str) -> String {
    let current = if prefix.is_empty() {
        if fs.name == "/" {
            String::new()
        } else {
            format!("/{}", fs.name)
        }
    } else {
        format!("{}/{}", prefix, fs.name)
    };

    let mut out = Vec::new();

    for file in &fs.files {
        let path = if current.is_empty() {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", current, file.name)
        };
        if let Some(content) = &file.content {
            if !content.is_empty() {
                out.push(format!("\n<file path=\"{}\">", path));
                out.push(content.clone());
                out.push("</file>".to_string());
            } else {
                out.push(format!("<file path=\"{}\" />", path));
            }
        } else {
            out.push(format!("<file path=\"{}\" />", path));
        }
    }

    for child in &fs.children {
        out.push(render_anthropic_filesystem(child, &current));
    }

    out.join("\n")
}

fn render_openai(builder: &ContextBuilder) -> String {
    let mut parts: Vec<String> = Vec::new();

    for b in &builder.blocks {
        let upper = b.name.to_uppercase();
        if matches!(b.name.as_str(), "system" | "user" | "assistant") {
            parts.push(format!("{}:", upper));
            parts.push(b.content.clone());
            parts.push(String::new());
        } else {
            parts.push(b.content.clone());
        }
    }

    if let Some(fs) = &builder.filesystem {
        parts.push("## Project Structure".to_string());
        parts.push("```".to_string());
        parts.push(fs.tree(0).trim().to_string());
        parts.push("```".to_string());
        parts.push(String::new());

        for entry in fs.all_files("") {
            if let Some(content) = &entry.file.content {
                if !content.is_empty() {
                    parts.push(format!("### {}", entry.path.replace("//", "/")));
                    parts.push("```".to_string());
                    parts.push(content.clone());
                    parts.push("```".to_string());
                    parts.push(String::new());
                }
            }
        }
    }

    parts.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_blocks() -> ContextBuilder {
        ContextBuilder::new()
            .block("system", "sys content", Default::default())
            .block("user", "usr content", Default::default())
    }

    #[test]
    fn default_contains_content() {
        let out = RenderFormat::Default.render(&ctx_with_blocks());
        assert!(out.contains("sys content"));
        assert!(out.contains("usr content"));
    }

    #[test]
    fn xml_has_envelope() {
        let out = RenderFormat::Xml.render(&ctx_with_blocks());
        assert!(out.starts_with("<context>"));
        assert!(out.ends_with("</context>"));
        assert!(out.contains("sys content"));
    }

    #[test]
    fn json_is_array() {
        let out = RenderFormat::Json.render(&ctx_with_blocks());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn anthropic_system_first() {
        let out = RenderFormat::Anthropic.render(&ctx_with_blocks());
        let sys_pos = out.find("sys content").unwrap();
        let usr_pos = out.find("usr content").unwrap();
        assert!(sys_pos < usr_pos);
    }

    #[test]
    fn openai_has_role_header() {
        let out = RenderFormat::OpenAi.render(&ctx_with_blocks());
        assert!(out.contains("SYSTEM:"));
        assert!(out.contains("USER:"));
    }
}
