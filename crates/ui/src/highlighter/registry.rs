use anyhow::{Context, Result, anyhow};
use gpui::{App, FontWeight, HighlightStyle, Hsla, SharedString};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};
use tree_sitter::Query;

use crate::{
    ActiveTheme, DEFAULT_THEME_COLORS, ThemeMode,
    highlighter::{Language, languages},
};

pub(super) const HIGHLIGHT_NAMES: [&str; 41] = [
    "attribute",
    "boolean",
    "comment",
    "comment.doc",
    "constant",
    "constructor",
    "embedded",
    "emphasis",
    "emphasis.strong",
    "enum",
    "function",
    "hint",
    "keyword",
    "label",
    "link_text",
    "link_uri",
    "number",
    "operator",
    "predictive",
    "preproc",
    "primary",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.list_marker",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regex",
    "string.special",
    "string.special.symbol",
    "tag",
    "tag.doctype",
    "text.code.span",
    "text.literal",
    "title",
    "type",
    "variable",
    "variable.special",
    "variant",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageConfig {
    pub name: SharedString,
    pub language: tree_sitter::Language,
    pub injection_languages: Vec<SharedString>,
    pub highlights: SharedString,
    pub injections: SharedString,
    pub locals: SharedString,
}

impl LanguageConfig {
    pub fn new(
        name: impl Into<SharedString>,
        language: tree_sitter::Language,
        injection_languages: Vec<SharedString>,
        highlights: &str,
        injections: &str,
        locals: &str,
    ) -> Self {
        Self {
            name: name.into(),
            language,
            injection_languages,
            highlights: SharedString::from(highlights.to_string()),
            injections: SharedString::from(injections.to_string()),
            locals: SharedString::from(locals.to_string()),
        }
    }
}

#[derive(Debug)]
#[allow(unused)]
pub(crate) struct HighlightQueryMetadata {
    pub(crate) locals_pattern_index: usize,
    pub(crate) highlights_pattern_index: usize,
    pub(crate) non_local_variable_patterns: Vec<bool>,
    pub(crate) injection_content_capture_index: Option<u32>,
    pub(crate) injection_language_capture_index: Option<u32>,
    pub(crate) local_scope_capture_index: Option<u32>,
    pub(crate) local_def_capture_index: Option<u32>,
    pub(crate) local_def_value_capture_index: Option<u32>,
    pub(crate) local_ref_capture_index: Option<u32>,
}

/// Shared, immutable tree-sitter language assets.
///
/// This intentionally does not store parsers, trees, text, or injection layers;
/// those remain per [`SyntaxHighlighter`](super::SyntaxHighlighter).
pub struct CompiledLanguage {
    pub(crate) name: SharedString,
    pub(crate) language: tree_sitter::Language,
    pub(crate) query: Query,
    pub(crate) injections_query: Option<Arc<Query>>,
    pub(crate) injection_queries: HashMap<SharedString, Arc<Query>>,
    pub(crate) metadata: HighlightQueryMetadata,
}

impl CompiledLanguage {
    pub fn name(&self) -> &SharedString {
        &self.name
    }
}

/// Theme for Tree-sitter Highlight
///
/// https://docs.rs/tree-sitter-highlight/0.26.8/tree_sitter_highlight/
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
pub struct SyntaxColors {
    pub attribute: Option<ThemeStyle>,
    pub boolean: Option<ThemeStyle>,
    pub comment: Option<ThemeStyle>,
    pub comment_doc: Option<ThemeStyle>,
    pub constant: Option<ThemeStyle>,
    pub constructor: Option<ThemeStyle>,
    pub embedded: Option<ThemeStyle>,
    pub emphasis: Option<ThemeStyle>,
    #[serde(rename = "emphasis.strong")]
    pub emphasis_strong: Option<ThemeStyle>,
    #[serde(rename = "enum")]
    pub enum_: Option<ThemeStyle>,
    pub function: Option<ThemeStyle>,
    pub hint: Option<ThemeStyle>,
    pub keyword: Option<ThemeStyle>,
    pub label: Option<ThemeStyle>,
    #[serde(rename = "link_text")]
    pub link_text: Option<ThemeStyle>,
    #[serde(rename = "link_uri")]
    pub link_uri: Option<ThemeStyle>,
    pub number: Option<ThemeStyle>,
    pub operator: Option<ThemeStyle>,
    pub predictive: Option<ThemeStyle>,
    pub preproc: Option<ThemeStyle>,
    pub primary: Option<ThemeStyle>,
    pub property: Option<ThemeStyle>,
    pub punctuation: Option<ThemeStyle>,
    #[serde(rename = "punctuation.bracket")]
    pub punctuation_bracket: Option<ThemeStyle>,
    #[serde(rename = "punctuation.delimiter")]
    pub punctuation_delimiter: Option<ThemeStyle>,
    #[serde(rename = "punctuation.list_marker")]
    pub punctuation_list_marker: Option<ThemeStyle>,
    #[serde(rename = "punctuation.special")]
    pub punctuation_special: Option<ThemeStyle>,
    pub string: Option<ThemeStyle>,
    #[serde(rename = "string.escape")]
    pub string_escape: Option<ThemeStyle>,
    #[serde(rename = "string.regex")]
    pub string_regex: Option<ThemeStyle>,
    #[serde(rename = "string.special")]
    pub string_special: Option<ThemeStyle>,
    #[serde(rename = "string.special.symbol")]
    pub string_special_symbol: Option<ThemeStyle>,
    pub tag: Option<ThemeStyle>,
    #[serde(rename = "tag.doctype")]
    pub tag_doctype: Option<ThemeStyle>,
    #[serde(rename = "text.code.span")]
    pub text_code_span: Option<ThemeStyle>,
    #[serde(rename = "text.literal")]
    pub text_literal: Option<ThemeStyle>,
    pub title: Option<ThemeStyle>,
    #[serde(rename = "type")]
    pub type_: Option<ThemeStyle>,
    pub variable: Option<ThemeStyle>,
    #[serde(rename = "variable.special")]
    pub variable_special: Option<ThemeStyle>,
    pub variant: Option<ThemeStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Italic,
    Underline,
}

impl From<FontStyle> for gpui::FontStyle {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => gpui::FontStyle::Normal,
            FontStyle::Italic => gpui::FontStyle::Italic,
            FontStyle::Underline => gpui::FontStyle::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize_repr, Deserialize_repr, JsonSchema)]
#[repr(u16)]
pub enum FontWeightContent {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Normal = 400,
    Medium = 500,
    Semibold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

impl From<FontWeightContent> for FontWeight {
    fn from(value: FontWeightContent) -> Self {
        match value {
            FontWeightContent::Thin => FontWeight::THIN,
            FontWeightContent::ExtraLight => FontWeight::EXTRA_LIGHT,
            FontWeightContent::Light => FontWeight::LIGHT,
            FontWeightContent::Normal => FontWeight::NORMAL,
            FontWeightContent::Medium => FontWeight::MEDIUM,
            FontWeightContent::Semibold => FontWeight::SEMIBOLD,
            FontWeightContent::Bold => FontWeight::BOLD,
            FontWeightContent::ExtraBold => FontWeight::EXTRA_BOLD,
            FontWeightContent::Black => FontWeight::BLACK,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
pub struct ThemeStyle {
    color: Option<Hsla>,
    font_style: Option<FontStyle>,
    font_weight: Option<FontWeightContent>,
}

impl From<ThemeStyle> for HighlightStyle {
    fn from(style: ThemeStyle) -> Self {
        HighlightStyle {
            color: style.color,
            font_weight: style.font_weight.map(Into::into),
            font_style: style.font_style.map(Into::into),
            ..Default::default()
        }
    }
}

impl SyntaxColors {
    pub fn style(&self, name: &str) -> Option<HighlightStyle> {
        if name.is_empty() {
            return None;
        }

        let style = match name {
            "attribute" => self.attribute,
            "boolean" => self.boolean,
            "comment" => self.comment,
            "comment.doc" => self.comment_doc,
            "constant" => self.constant,
            "constructor" => self.constructor,
            "embedded" => self.embedded,
            "emphasis" => self.emphasis,
            "emphasis.strong" => self.emphasis_strong,
            "enum" => self.enum_,
            "function" => self.function,
            "hint" => self.hint,
            "keyword" => self.keyword,
            "label" => self.label,
            "link_text" => self.link_text,
            "link_uri" => self.link_uri,
            "number" => self.number,
            "operator" => self.operator,
            "predictive" => self.predictive,
            "preproc" => self.preproc,
            "primary" => self.primary,
            "property" => self.property,
            "punctuation" => self.punctuation,
            "punctuation.bracket" => self.punctuation_bracket,
            "punctuation.delimiter" => self.punctuation_delimiter,
            "punctuation.list_marker" => self.punctuation_list_marker,
            "punctuation.special" => self.punctuation_special,
            "string" => self.string,
            "string.escape" => self.string_escape,
            "string.regex" => self.string_regex,
            "string.special" => self.string_special,
            "string.special.symbol" => self.string_special_symbol,
            "tag" => self.tag,
            "tag.doctype" => self.tag_doctype,
            "text.code.span" => self.text_code_span,
            "text.literal" => self.text_literal,
            "title" => self.title,
            "type" => self.type_,
            "variable" => self.variable,
            "variable.special" => self.variable_special,
            "variant" => self.variant,
            _ => None,
        }
        .map(|s| s.into());

        if style.is_some() {
            style
        } else {
            // Fallback `keyword.modifier` to `keyword`
            if name.contains(".") {
                if let Some(prefix) = name.split(".").next() {
                    return self.style(prefix);
                }

                None
            } else {
                None
            }
        }
    }

    #[inline]
    pub fn style_for_index(&self, index: usize) -> Option<HighlightStyle> {
        HIGHLIGHT_NAMES.get(index).and_then(|name| self.style(name))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
pub struct StatusColors {
    #[serde(rename = "error")]
    error: Option<Hsla>,
    #[serde(rename = "error.background")]
    error_background: Option<Hsla>,
    #[serde(rename = "error.border")]
    error_border: Option<Hsla>,
    #[serde(rename = "warning")]
    warning: Option<Hsla>,
    #[serde(rename = "warning.background")]
    warning_background: Option<Hsla>,
    #[serde(rename = "warning.border")]
    warning_border: Option<Hsla>,
    #[serde(rename = "info")]
    info: Option<Hsla>,
    #[serde(rename = "info.background")]
    info_background: Option<Hsla>,
    #[serde(rename = "info.border")]
    info_border: Option<Hsla>,
    #[serde(rename = "success")]
    success: Option<Hsla>,
    #[serde(rename = "success.background")]
    success_background: Option<Hsla>,
    #[serde(rename = "success.border")]
    success_border: Option<Hsla>,
    #[serde(rename = "hint")]
    hint: Option<Hsla>,
    #[serde(rename = "hint.background")]
    hint_background: Option<Hsla>,
    #[serde(rename = "hint.border")]
    hint_border: Option<Hsla>,
}

impl StatusColors {
    #[inline]
    pub fn error(&self, cx: &App) -> Hsla {
        self.error.unwrap_or(cx.theme().red)
    }

    #[inline]
    pub fn error_background(&self, cx: &App) -> Hsla {
        let bg = cx.theme().background;
        self.error_background
            .unwrap_or(bg.blend(self.error(cx).alpha(0.2)))
    }

    #[inline]
    pub fn error_border(&self, cx: &App) -> Hsla {
        self.error_border.unwrap_or(self.error(cx))
    }

    #[inline]
    pub fn warning(&self, cx: &App) -> Hsla {
        self.warning.unwrap_or(cx.theme().yellow)
    }

    #[inline]
    pub fn warning_background(&self, cx: &App) -> Hsla {
        let bg = cx.theme().background;
        self.warning_background
            .unwrap_or(bg.blend(self.warning(cx).alpha(0.2)))
    }

    #[inline]
    pub fn warning_border(&self, cx: &App) -> Hsla {
        self.warning_border.unwrap_or(self.warning(cx))
    }

    #[inline]
    pub fn info(&self, cx: &App) -> Hsla {
        self.info.unwrap_or(cx.theme().blue)
    }

    #[inline]
    pub fn info_background(&self, cx: &App) -> Hsla {
        let bg = cx.theme().background;
        self.info_background
            .unwrap_or(bg.blend(self.info(cx).alpha(0.2)))
    }

    #[inline]
    pub fn info_border(&self, cx: &App) -> Hsla {
        self.info_border.unwrap_or(self.info(cx))
    }

    #[inline]
    pub fn success(&self, cx: &App) -> Hsla {
        self.success.unwrap_or(cx.theme().green)
    }

    #[inline]
    pub fn success_background(&self, cx: &App) -> Hsla {
        let bg = cx.theme().background;
        self.success_background
            .unwrap_or(bg.blend(self.success(cx).alpha(0.2)))
    }

    #[inline]
    pub fn success_border(&self, cx: &App) -> Hsla {
        self.success_border.unwrap_or(self.success(cx))
    }

    #[inline]
    pub fn hint(&self, cx: &App) -> Hsla {
        self.hint.unwrap_or(cx.theme().cyan)
    }

    #[inline]
    pub fn hint_background(&self, cx: &App) -> Hsla {
        let bg = cx.theme().background;
        self.hint_background
            .unwrap_or(bg.blend(self.hint(cx).alpha(0.2)))
    }

    #[inline]
    pub fn hint_border(&self, cx: &App) -> Hsla {
        self.hint_border.unwrap_or(self.hint(cx))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
pub struct HighlightThemeStyle {
    #[serde(rename = "editor.background")]
    pub editor_background: Option<Hsla>,
    #[serde(rename = "editor.foreground")]
    pub editor_foreground: Option<Hsla>,
    #[serde(rename = "editor.active_line.background")]
    pub editor_active_line: Option<Hsla>,
    #[serde(rename = "editor.line_number")]
    pub editor_line_number: Option<Hsla>,
    #[serde(rename = "editor.active_line_number")]
    pub editor_active_line_number: Option<Hsla>,
    #[serde(rename = "editor.invisible")]
    pub editor_invisible: Option<Hsla>,
    #[serde(flatten)]
    pub status: StatusColors,
    #[serde(rename = "syntax")]
    pub syntax: SyntaxColors,
}

/// Theme for Tree-sitter Highlight from JSON theme file.
///
/// This json is compatible with the Zed theme format.
///
/// https://zed.dev/docs/extensions/languages#syntax-highlighting
#[derive(Debug, Clone, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
pub struct HighlightTheme {
    pub name: String,
    #[serde(default)]
    pub appearance: ThemeMode,
    pub style: HighlightThemeStyle,
}

impl Deref for HighlightTheme {
    type Target = SyntaxColors;

    fn deref(&self) -> &Self::Target {
        &self.style.syntax
    }
}

impl HighlightTheme {
    pub fn default_dark() -> Arc<Self> {
        DEFAULT_THEME_COLORS[&ThemeMode::Dark].1.clone()
    }

    pub fn default_light() -> Arc<Self> {
        DEFAULT_THEME_COLORS[&ThemeMode::Light].1.clone()
    }
}

/// Registry for code highlighter languages.
pub struct LanguageRegistry {
    languages: Mutex<HashMap<SharedString, LanguageConfig>>,
    compiled_languages: Mutex<HashMap<SharedString, Arc<CompiledLanguage>>>,
}

impl LanguageRegistry {
    /// Returns the singleton instance of the `LanguageRegistry` with default languages and themes.
    pub fn singleton() -> &'static LazyLock<LanguageRegistry> {
        static INSTANCE: LazyLock<LanguageRegistry> = LazyLock::new(|| LanguageRegistry {
            languages: Mutex::new(
                languages::Language::all()
                    .map(|language| (language.name().into(), language.config()))
                    .collect(),
            ),
            compiled_languages: Mutex::new(HashMap::new()),
        });
        &INSTANCE
    }

    /// Registers a new language configuration to the registry.
    pub fn register(&self, lang: &str, config: &LanguageConfig) {
        self.languages
            .lock()
            .unwrap()
            .insert(lang.to_string().into(), config.clone());
        let mut compiled_languages = self.compiled_languages.lock().unwrap();
        let mut invalidated = 0;
        invalidated += compiled_languages.remove(lang).is_some() as usize;
        if config.name.as_ref() != lang {
            invalidated += compiled_languages.remove(&config.name).is_some() as usize;
        }
        tracing::debug!(
            operation = "syntax_language_cache",
            language = lang,
            status = "invalidated",
            invalidated_count = invalidated,
            "syntax language cache invalidated after registration"
        );
    }

    /// Returns a list of all registered language names.
    pub fn languages(&self) -> Vec<SharedString> {
        self.languages.lock().unwrap().keys().cloned().collect()
    }

    /// Returns the language configuration for the given language name.
    pub fn language(&self, name: &str) -> Option<LanguageConfig> {
        // Try to get by name first, there may have a custom language registered
        // Then try to get built-in language to support short language names, e.g. "js" for "javascript"
        self.resolve_language_config(name).map(|(_, config)| config)
    }

    /// Returns the compiled language assets for the given language name.
    ///
    /// Query compilation is expensive for some grammars, so this cache is shared
    /// by all highlighter instances. Parsers and trees are still per highlighter.
    pub fn compiled_language(&self, name: &str) -> Result<Arc<CompiledLanguage>> {
        let started = Instant::now();
        let (cache_key, config) = self.resolve_language_config(name).ok_or_else(|| {
            anyhow!(
                "language {:?} is not registered in `LanguageRegistry`",
                name
            )
        })?;

        if let Some(compiled) = self
            .compiled_languages
            .lock()
            .unwrap()
            .get(&cache_key)
            .cloned()
        {
            tracing::debug!(
                operation = "syntax_language_cache",
                language = name,
                resolved_language = %compiled.name,
                status = "hit",
                elapsed_ms = duration_ms(started.elapsed()),
                "syntax language cache hit"
            );
            return Ok(compiled);
        }

        tracing::info!(
            operation = "syntax_language_cache",
            language = name,
            resolved_language = %config.name,
            status = "miss",
            elapsed_ms = duration_ms(started.elapsed()),
            "syntax language cache miss"
        );

        let compiled = Arc::new(self.compile_language(&config)?);
        let mut compiled_languages = self.compiled_languages.lock().unwrap();
        if let Some(existing) = compiled_languages.get(&cache_key).cloned() {
            tracing::debug!(
                operation = "syntax_language_cache",
                language = name,
                resolved_language = %existing.name,
                status = "hit_after_compile",
                elapsed_ms = duration_ms(started.elapsed()),
                "syntax language cache filled while compiling"
            );
            return Ok(existing);
        }

        compiled_languages.insert(cache_key, compiled.clone());
        tracing::info!(
            operation = "syntax_language_cache",
            language = name,
            resolved_language = %compiled.name,
            status = "stored",
            elapsed_ms = duration_ms(started.elapsed()),
            "syntax language cached"
        );
        Ok(compiled)
    }

    /// Warms the compiled language cache and returns the cached assets.
    pub fn warm_language(&self, name: &str) -> Result<Arc<CompiledLanguage>> {
        self.compiled_language(name)
    }

    /// Clears a compiled language cache entry.
    pub fn clear_compiled_language(&self, name: &str) {
        let resolved = self
            .resolve_language_config(name)
            .map(|(cache_key, config)| (cache_key, config.name));
        let mut compiled_languages = self.compiled_languages.lock().unwrap();
        let mut removed = compiled_languages.remove(name).is_some();
        if let Some((cache_key, config_name)) = resolved {
            removed |= compiled_languages.remove(&cache_key).is_some();
            removed |= compiled_languages.remove(&config_name).is_some();
        }
        tracing::debug!(
            operation = "syntax_language_cache",
            language = name,
            status = "cleared",
            removed,
            "syntax language cache entry cleared"
        );
    }

    fn resolve_language_config(&self, name: &str) -> Option<(SharedString, LanguageConfig)> {
        let languages = self.languages.lock().unwrap();
        languages
            .get_key_value(name)
            .map(|(key, config)| (key.clone(), config.clone()))
            .or_else(|| {
                Language::from_name(name).and_then(|language| {
                    languages
                        .get_key_value(language.name())
                        .map(|(key, config)| (key.clone(), config.clone()))
                })
            })
    }

    fn compile_language(&self, config: &LanguageConfig) -> Result<CompiledLanguage> {
        let compile_started = Instant::now();

        let phase_started = Instant::now();
        let mut query_source = String::new();
        query_source.push_str(&config.injections);
        let locals_query_offset = query_source.len();
        query_source.push_str(&config.locals);
        let highlights_query_offset = query_source.len();
        query_source.push_str(&config.highlights);
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "query_concat",
            language = %config.name,
            injections_bytes = config.injections.len(),
            locals_bytes = config.locals.len(),
            highlights_bytes = config.highlights.len(),
            query_source_bytes = query_source.len(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let mut query = Query::new(&config.language, &query_source).context("new query")?;
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "main_query_new",
            language = %config.name,
            query_source_bytes = query_source.len(),
            pattern_count = query.pattern_count(),
            capture_count = query.capture_names().len(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let mut locals_pattern_index = 0;
        let mut highlights_pattern_index = 0;
        for i in 0..(query.pattern_count()) {
            let pattern_offset = query.start_byte_for_pattern(i);
            if pattern_offset < highlights_query_offset {
                if pattern_offset < highlights_query_offset {
                    highlights_pattern_index += 1;
                }
                if pattern_offset < locals_query_offset {
                    locals_pattern_index += 1;
                }
            }
        }
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "pattern_index_scan",
            language = %config.name,
            locals_pattern_index,
            highlights_pattern_index,
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let injections_query = if !config.injections.is_empty() {
            Query::new(&config.language, &config.injections)
                .ok()
                .map(Arc::new)
        } else {
            None
        };
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "injections_query_new",
            language = %config.name,
            injections_bytes = config.injections.len(),
            has_injections_query = injections_query.is_some(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        for pattern_index in 0..locals_pattern_index {
            query.disable_pattern(pattern_index);
        }
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "disable_injection_patterns",
            language = %config.name,
            disabled_patterns = locals_pattern_index,
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let non_local_variable_patterns = (0..query.pattern_count())
            .map(|i| {
                query
                    .property_predicates(i)
                    .iter()
                    .any(|(prop, positive)| !*positive && prop.key.as_ref() == "local")
            })
            .collect();
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "non_local_pattern_scan",
            language = %config.name,
            pattern_count = query.pattern_count(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let injection_content_capture_index = injections_query.as_ref().and_then(|q| {
            q.capture_names()
                .iter()
                .position(|name| *name == "injection.content")
                .map(|i| i as u32)
        });
        let injection_language_capture_index = injections_query.as_ref().and_then(|q| {
            q.capture_names()
                .iter()
                .position(|name| *name == "injection.language")
                .map(|i| i as u32)
        });
        let mut local_def_capture_index = None;
        let mut local_def_value_capture_index = None;
        let mut local_ref_capture_index = None;
        let mut local_scope_capture_index = None;
        for (i, name) in query.capture_names().iter().enumerate() {
            let i = Some(i as u32);
            match *name {
                "local.definition" => local_def_capture_index = i,
                "local.definition-value" => local_def_value_capture_index = i,
                "local.reference" => local_ref_capture_index = i,
                "local.scope" => local_scope_capture_index = i,
                _ => {}
            }
        }
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "capture_index_scan",
            language = %config.name,
            capture_count = query.capture_names().len(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        let phase_started = Instant::now();
        let mut injection_queries = HashMap::new();
        for inj_language in config.injection_languages.iter() {
            if let Some(inj_config) = self.language(inj_language) {
                match Query::new(&inj_config.language, &inj_config.highlights) {
                    Ok(q) => {
                        injection_queries.insert(inj_config.name.clone(), Arc::new(q));
                    }
                    Err(e) => {
                        tracing::error!(
                            "failed to build injection query for {:?}: {:?}",
                            inj_config.name,
                            e
                        );
                    }
                }
            }
        }
        tracing::info!(
            operation = "syntax_language_compile",
            phase = "injection_language_queries",
            language = %config.name,
            injection_language_count = config.injection_languages.len(),
            compiled_injection_query_count = injection_queries.len(),
            elapsed_ms = duration_ms(phase_started.elapsed()),
            "syntax language compile phase completed"
        );

        tracing::info!(
            operation = "syntax_language_compile",
            language = %config.name,
            status = "ok",
            pattern_count = query.pattern_count(),
            capture_count = query.capture_names().len(),
            injection_query_count = injection_queries.len(),
            elapsed_ms = duration_ms(compile_started.elapsed()),
            "syntax language compiled"
        );

        Ok(CompiledLanguage {
            name: config.name.clone(),
            language: config.language.clone(),
            query,
            injections_query,
            injection_queries,
            metadata: HighlightQueryMetadata {
                locals_pattern_index,
                highlights_pattern_index,
                non_local_variable_patterns,
                injection_content_capture_index,
                injection_language_capture_index,
                local_scope_capture_index,
                local_def_capture_index,
                local_def_value_capture_index,
                local_ref_capture_index,
            },
        })
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use crate::highlighter::LanguageConfig;
    use std::sync::Arc;

    #[test]
    fn test_registry() {
        use super::LanguageRegistry;
        let registry = LanguageRegistry::singleton();

        registry.register(
            "foo",
            &LanguageConfig::new("foo", tree_sitter_json::LANGUAGE.into(), vec![], "", "", ""),
        );

        assert!(registry.language("foo").is_some());
        assert!(registry.language("json").is_some());
        assert!(registry.language("text").is_some());
        assert!(registry.language("unknown").is_none());

        #[cfg(feature = "tree-sitter-rust")]
        {
            assert!(registry.language("rust").is_some());
            assert!(registry.language("rs").is_some());
        }
        #[cfg(not(feature = "tree-sitter-rust"))]
        {
            assert!(registry.language("rust").is_none());
            assert!(registry.language("rs").is_none());
        }

        #[cfg(feature = "tree-sitter-javascript")]
        {
            assert!(registry.language("javascript").is_some());
            assert!(registry.language("js").is_some());
        }
        #[cfg(not(feature = "tree-sitter-javascript"))]
        {
            assert!(registry.language("javascript").is_none());
            assert!(registry.language("js").is_none());
        }
    }

    #[test]
    fn compiled_language_cache_reuses_compiled_queries() {
        use super::LanguageRegistry;
        let registry = LanguageRegistry::singleton();

        let first = registry.warm_language("json").expect("json should compile");
        let second = registry
            .warm_language("json")
            .expect("json should come from cache");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.name().as_ref(), "json");
    }

    #[test]
    fn register_invalidates_compiled_language_cache() {
        use super::LanguageRegistry;
        let registry = LanguageRegistry::singleton();
        let language_name = "cache_invalidation_test_json";
        let config = LanguageConfig::new(
            language_name,
            tree_sitter_json::LANGUAGE.into(),
            vec![],
            "",
            "",
            "",
        );

        registry.register(language_name, &config);
        let first = registry
            .warm_language(language_name)
            .expect("custom language should compile");
        let second = registry
            .warm_language(language_name)
            .expect("custom language should come from cache");
        assert!(Arc::ptr_eq(&first, &second));

        registry.register(language_name, &config);
        let third = registry
            .warm_language(language_name)
            .expect("custom language should recompile after registration");
        assert!(!Arc::ptr_eq(&first, &third));
    }
}
