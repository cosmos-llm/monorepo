//! The prompt plan: the IR between the instruction/environment producers and
//! whatever renders the system prompt (doc 190).
//!
//! Producers do not emit text. They emit a [`PromptPlan`], an ordered list of
//! typed [`PromptSection`]s, and rendering to a string happens last. Caching,
//! token estimates and compaction all read the plan, where the section
//! boundaries are still visible.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::environment::EnvironmentFacts;
use crate::instructions::Instructions;

/// What a section is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// The base agent prompt.
    Base,
    /// How to use the available tools.
    ToolGuidance,
    /// Project and user instructions (`AGENTS.md` and friends).
    Instructions,
    /// Facts about the working environment.
    Environment,
    /// A short note injected mid-session.
    Reminder,
}

/// How often a section's text changes, which drives where cache breakpoints go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    /// Fixed for a given build and toolset.
    Static,
    /// Stable within a session.
    Session,
    /// May change every turn.
    Turn,
}

/// A request that the provider cache the prompt up to and including a section.
///
/// A local stand-in until the client crate's cache hint (doc 030) lands; the
/// serde shape is the same tagged form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheHint {
    /// A short-lived cache breakpoint.
    Ephemeral,
}

/// One piece of a section's content.
///
/// Prompt text only needs text today; the tag leaves room for more (images in
/// instructions, say) without breaking the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptBlock {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
}

impl PromptBlock {
    /// A text block.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

/// A typed, ordered piece of the system prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptSection {
    /// What the section is for.
    pub kind: SectionKind,
    /// Where it came from, for `/context` and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// The content.
    pub content: Vec<PromptBlock>,
    /// Cache breakpoint after this section, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    /// How often the section changes.
    pub stability: Stability,
}

impl PromptSection {
    /// A one-block text section with the default stability for `kind`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::{PromptSection, SectionKind};
    ///
    /// let s = PromptSection::text(SectionKind::Base, "You are a coding agent.");
    /// assert_eq!(s.rendered(), "You are a coding agent.");
    /// ```
    pub fn text(kind: SectionKind, text: impl Into<String>) -> Self {
        let stability = match kind {
            SectionKind::Base | SectionKind::ToolGuidance => Stability::Static,
            SectionKind::Instructions | SectionKind::Environment => Stability::Session,
            SectionKind::Reminder => Stability::Turn,
        };
        Self {
            kind,
            source: None,
            content: vec![PromptBlock::text(text)],
            cache: None,
            stability,
        }
    }

    /// Sets the source path.
    pub fn with_source(mut self, source: impl Into<PathBuf>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Sets the cache hint.
    pub fn with_cache(mut self, cache: CacheHint) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Sets the stability.
    pub fn with_stability(mut self, stability: Stability) -> Self {
        self.stability = stability;
        self
    }

    /// The section's text, blocks joined by newlines.
    pub fn rendered(&self) -> String {
        self.content
            .iter()
            .map(|b| match b {
                PromptBlock::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// An ordered list of sections: the system prompt before it is text.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_context::{PromptPlan, PromptSection, SectionKind};
///
/// let plan = PromptPlan::new()
///     .with_section(PromptSection::text(SectionKind::Base, "Base."))
///     .with_section(PromptSection::text(SectionKind::Environment, "cwd: /x"));
/// assert_eq!(plan.render(), "Base.\n\ncwd: /x");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptPlan {
    /// The sections, in prompt order.
    pub sections: Vec<PromptSection>,
}

impl PromptPlan {
    /// An empty plan.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a section.
    pub fn with_section(mut self, section: PromptSection) -> Self {
        self.sections.push(section);
        self
    }

    /// Appends a section in place.
    pub fn push(&mut self, section: PromptSection) {
        self.sections.push(section);
    }

    /// Builds the default plan: base prompt, tool guidance, instructions,
    /// then the environment.
    ///
    /// The last instructions section carries the cache breakpoint, so the
    /// environment, which changes more often, sits after the cached prefix.
    /// Empty `base`/`tool_guidance` are left out.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::{Instructions, PromptPlan, SectionKind};
    ///
    /// let plan = PromptPlan::assemble("Base.", "", &Instructions::default(), None);
    /// assert_eq!(plan.sections.len(), 1);
    /// assert_eq!(plan.sections[0].kind, SectionKind::Base);
    /// ```
    pub fn assemble(
        base: &str,
        tool_guidance: &str,
        instructions: &Instructions,
        environment: Option<&EnvironmentFacts>,
    ) -> Self {
        let mut plan = Self::new();
        if !base.trim().is_empty() {
            plan.push(PromptSection::text(SectionKind::Base, base));
        }
        if !tool_guidance.trim().is_empty() {
            plan.push(PromptSection::text(
                SectionKind::ToolGuidance,
                tool_guidance,
            ));
        }
        plan.sections.extend(instructions.to_sections());
        if let Some(env) = environment {
            plan.push(PromptSection::text(SectionKind::Environment, env.render()));
        }
        plan
    }

    /// Renders the plan to text: section texts separated by blank lines.
    pub fn render(&self) -> String {
        self.sections
            .iter()
            .map(PromptSection::rendered)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Indices of sections that end a cached prefix.
    pub fn cache_breakpoints(&self) -> Vec<usize> {
        self.sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.cache.is_some())
            .map(|(i, _)| i)
            .collect()
    }

    /// Rough size of the rendered plan in bytes, by section kind in order.
    /// Useful for `/context` before real token counts (doc 160) exist.
    pub fn section_sizes(&self) -> Vec<(SectionKind, usize)> {
        self.sections
            .iter()
            .map(|s| (s.kind, s.rendered().len()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_shape_is_tagged_snake_case() {
        let plan = PromptPlan::new().with_section(
            PromptSection::text(SectionKind::ToolGuidance, "x").with_cache(CacheHint::Ephemeral),
        );
        let v = serde_json::to_value(&plan).unwrap();
        assert_eq!(v["sections"][0]["kind"], "tool_guidance");
        assert_eq!(v["sections"][0]["content"][0]["type"], "text");
        assert_eq!(v["sections"][0]["cache"]["type"], "ephemeral");
        assert_eq!(v["sections"][0]["stability"], "static");
        assert_eq!(serde_json::from_value::<PromptPlan>(v).unwrap(), plan);
    }

    #[test]
    fn render_skips_empty_sections() {
        let plan = PromptPlan::new()
            .with_section(PromptSection::text(SectionKind::Base, "a"))
            .with_section(PromptSection::text(SectionKind::Reminder, ""))
            .with_section(PromptSection::text(SectionKind::Base, "b"));
        assert_eq!(plan.render(), "a\n\nb");
    }
}
