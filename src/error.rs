use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("namespace '{0}' is not in the allowed list")]
    NamespaceNotAllowed(String),

    #[error("kubernetes error: {0}")]
    Kube(#[from] kube::Error),

    #[error("{0}")]
    BadRequest(String),

    #[error("action '{action}' is not allowed on resource '{resource}'")]
    ActionNotAllowed { resource: String, action: String },
}

impl Error {
    pub fn to_tool_error(&self) -> String {
        self.to_string()
    }
}
