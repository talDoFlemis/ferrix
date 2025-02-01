use std::sync::Arc;

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Diagnostic, Clone, Eq, PartialEq, Error)]
#[error("Failed to parse Ferrix Input")]
pub struct FerrixError<D: Diagnostic = FerrixDiagnostic> {
    /// Original input that this failure came from.
    #[source_code]
    pub input: Arc<String>,

    /// Sub-diagnostics for this failure.
    #[related]
    pub diagnostics: Vec<D>,
}

/// An individual diagnostic message for a Ferrix parsing issue.
#[derive(Debug, Diagnostic, Clone, Eq, PartialEq, Error)]
#[error("{}", message.clone().unwrap_or_else(|| "Unexpected error".into()))]
pub struct FerrixDiagnostic {
    /// Shared source for the diagnostic.
    #[source_code]
    pub input: Arc<String>,

    /// Offset in chars of the error.
    #[label("{}", label.clone().unwrap_or_else(|| "here".into()))]
    pub span: SourceSpan,

    /// Message for the error itself.
    pub message: Option<String>,

    /// Label text for this span. Defaults to `"here"`.
    pub label: Option<String>,

    /// Suggestion for fixing the parser error.
    #[help]
    pub help: Option<String>,

    /// Severity level for the Diagnostic.
    #[diagnostic(severity)]
    pub severity: miette::Severity,
}
