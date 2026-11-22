use thiserror::Error;

/// Errors that can occur in LSP operations.
#[derive(Error, Debug)]
pub enum LspError {
    /// Failed to spawn a language server process.
    #[error("Failed to spawn language server for {language}: {reason}")]
    SpawnFailed {
        /// Language whose server could not be started.
        language: String,
        /// Reason the process could not be spawned.
        reason: String,
    },

    /// LSP initialize handshake failed.
    #[error("LSP handshake failed for {language}: {detail}")]
    HandshakeFailed {
        /// Language whose handshake failed.
        language: String,
        /// Failure detail reported by the server.
        detail: String,
    },

    /// An LSP request exceeded its timeout.
    #[error("LSP request timed out for {language} after {timeout_secs}s")]
    RequestTimeout {
        /// Language that handled the request.
        language: String,
        /// Timeout that was exceeded, in seconds.
        timeout_secs: u64,
    },

    /// The language server process exited unexpectedly.
    #[error("LSP server crashed: {language}")]
    ServerCrashed {
        /// Language whose server crashed.
        language: String,
    },

    /// An invalid state transition was attempted on the LSP client.
    #[error("Invalid state transition for {language}: {from_state} -> {to_state}")]
    InvalidTransition {
        /// Language whose client changed state.
        language: String,
        /// State the client was in.
        from_state: String,
        /// State the client was asked to enter.
        to_state: String,
    },

    /// No language server is registered for the given language.
    #[error("No language server registered for: {0}")]
    NotRegistered(String),

    /// An I/O error occurred during LSP communication.
    #[error("LSP I/O error: {0}")]
    Io(String),

    /// An LSP protocol error (malformed messages, unexpected responses).
    #[error("LSP protocol error: {0}")]
    Protocol(String),
}

impl From<LspError> for String {
    fn from(err: LspError) -> Self {
        err.to_string()
    }
}