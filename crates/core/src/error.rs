use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("style '{0}' not loaded — call load_style() first")]
    StyleNotLoaded(String),

    #[error("locale '{0}' not loaded — call load_locale() first (formatting without it would silently drop every locale term: long month names, 'and', 'n.d.', …)")]
    LocaleNotLoaded(String),

    #[error("invalid CSL style XML: {0}")]
    InvalidStyle(String),

    #[error("invalid locale XML: {0}")]
    InvalidLocale(String),

    #[error("invalid CSL-JSON input: {0}")]
    InvalidCslJson(String),

    #[error("formatting failed: {0}")]
    FormatFailed(String),
}
