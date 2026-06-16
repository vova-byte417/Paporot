//! Trace 模块错误类型。

use std::fmt;

/// Trace 模块的所有错误。
#[derive(Debug)]
pub enum TraceError {
    /// I/O 错误
    Io {
        message: String,
    },

    /// SQLite 数据库错误
    Database {
        message: String,
    },

    /// 解析错误
    ParseError {
        message: String,
        adapter: String,
    },

    /// 部分导入成功
    PartialImport {
        imported: usize,
        skipped: usize,
        reasons: Vec<String>,
    },

    /// trace 未找到
    NotFound {
        message: String,
    },

    /// 序列化/反序列化错误
    Serialize {
        message: String,
    },

    /// 不支持的格式
    UnsupportedFormat {
        format: String,
        adapter: String,
    },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceError::Io { message } => write!(f, "I/O error: {}", message),
            TraceError::Database { message } => write!(f, "Database error: {}", message),
            TraceError::ParseError { message, adapter } => {
                write!(f, "Parse error [{}]: {}", adapter, message)
            }
            TraceError::PartialImport {
                imported,
                skipped,
                reasons,
            } => {
                write!(
                    f,
                    "Partial import: {} ok, {} skipped. Reasons: {}",
                    imported,
                    skipped,
                    reasons.join("; ")
                )
            }
            TraceError::NotFound { message } => write!(f, "Not found: {}", message),
            TraceError::Serialize { message } => write!(f, "Serialize error: {}", message),
            TraceError::UnsupportedFormat { format, adapter } => {
                write!(
                    f,
                    "Unsupported format '{}' for adapter '{}'",
                    format, adapter
                )
            }
        }
    }
}

impl std::error::Error for TraceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_error_display() {
        let err = TraceError::NotFound {
            message: "trace not found".into(),
        };
        assert_eq!(format!("{}", err), "Not found: trace not found");
    }

    #[test]
    fn test_parse_error_display() {
        let err = TraceError::ParseError {
            message: "invalid JSON".into(),
            adapter: "deepseek".into(),
        };
        assert_eq!(format!("{}", err), "Parse error [deepseek]: invalid JSON");
    }

    #[test]
    fn test_partial_import_display() {
        let err = TraceError::PartialImport {
            imported: 5,
            skipped: 2,
            reasons: vec!["line 3: bad json".into(), "line 7: missing id".into()],
        };
        let msg = format!("{}", err);
        assert!(msg.contains("5 ok"));
        assert!(msg.contains("2 skipped"));
    }

    #[test]
    fn test_error_is_std_error() {
        fn takes_error(_: &dyn std::error::Error) {}
        let err = TraceError::Io {
            message: "test".into(),
        };
        takes_error(&err);
    }
}
