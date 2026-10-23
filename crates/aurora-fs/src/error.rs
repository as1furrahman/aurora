//! Safety errors. Variants mirror the Python exception hierarchy in `awb/safety/exceptions.py`.

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    /// Generic safety refusal (protected/sensitive target, `.system`, insecure boundary).
    #[error("{0}")]
    Safety(String),
    #[error("{0}")]
    PathTraversal(String),
    #[error("{0}")]
    SymlinkAncestor(String),
    #[error("{0}")]
    GlobalMutationBlocked(String),
    #[error("{0}")]
    SecurityPrecondition(String),
    #[error("{0}")]
    Descriptor(String),
    /// FIFO or other non-regular node where a regular file was required.
    #[error("{0}")]
    Fifo(String),
    #[error("{0}")]
    LockTimeout(String),
    #[error("{0}")]
    Lock(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl FsError {
    pub(crate) fn io(context: impl Into<String>, source: impl Into<std::io::Error>) -> Self {
        FsError::Io { context: context.into(), source: source.into() }
    }
}
