use anp_core::graph::validate::ValidationError;

#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("no API key found. Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variable")]
    NoApiKey,

    #[error("failed to parse LLM response as JSON: {0}")]
    ParseError(String),

    #[error("no JSON found in LLM response")]
    NoJsonInResponse,

    #[error("graph validation failed after {attempts} attempts: {errors:?}")]
    ValidationFailed {
        attempts: usize,
        errors: Vec<ValidationError>,
    },

    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
}

pub type PlannerResult<T> = Result<T, PlannerError>;
