//! JSON-RPC 2.0 protocol types for the HIR daemon.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Response {
    /// Create a success response.
    pub fn success(id: Option<u64>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(id: Option<u64>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(ErrorObject {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

// Standard JSON-RPC 2.0 error codes.
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;

// Custom error codes.
pub const PATH_ESCAPE: i32 = -32001;
pub const NOT_INITIALIZED: i32 = -32002;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"hir/initialize","params":{"projectPath":"/tmp/proj"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, Some(1));
        assert_eq!(req.method, "hir/initialize");
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":}"#;
        let result: Result<Request, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_jsonrpc_field_fails() {
        let json = r#"{"id":1,"method":"hir/initialize"}"#;
        let result: Result<Request, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing jsonrpc field should fail to parse"
        );
    }

    #[test]
    fn serialize_success_response() {
        let resp = Response::success(Some(1), serde_json::json!({"status":"ready"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"status\":\"ready\""));
    }

    #[test]
    fn serialize_error_response() {
        let resp = Response::error(Some(1), METHOD_NOT_FOUND, "Method not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"code\":-32601"));
        assert!(json.contains("\"message\":\"Method not found\""));
    }

    #[test]
    fn method_not_found_error_code_is_correct() {
        assert_eq!(METHOD_NOT_FOUND, -32601);
    }

    #[test]
    fn parse_error_code_is_correct() {
        assert_eq!(PARSE_ERROR, -32700);
    }

    #[test]
    fn path_escape_error_code_is_correct() {
        assert_eq!(PATH_ESCAPE, -32001);
    }
}
