use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunboxError {
    #[error("VFS error: {0}")]
    Vfs(String),

    #[error("process error: {0}")]
    Process(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("shell error: {0}")]
    Shell(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),
}

pub type Result<T> = std::result::Result<T, RunboxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RunboxError::NotFound("file.txt".into());
        assert_eq!(err.to_string(), "not found: file.txt");

        let err2 = RunboxError::PermissionDenied("dir".into());
        assert_eq!(err2.to_string(), "permission denied: dir");
    }
}
