use serde::{Deserialize, Serialize};

// OpenAPI schemas are only needed by the server binary (`server` feature).
#[derive(Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "server",
    schema(example = json!({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/list",
    "params": {}
}))
)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[cfg_attr(feature = "server", schema(value_type = Option<serde_json::Value>))]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    #[cfg_attr(feature = "server", schema(value_type = serde_json::Value))]
    pub params: serde_json::Value,
}

#[derive(Serialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[cfg_attr(feature = "server", schema(value_type = Option<serde_json::Value>))]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "server", schema(value_type = Option<serde_json::Value>))]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

pub fn success_response(request: &JsonRpcRequest, result: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(result),
        error: None,
    }
}

pub fn error_response(request: &JsonRpcRequest, code: i32, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
    }
}

pub fn method_not_found(request: &JsonRpcRequest) -> JsonRpcResponse {
    error_response(
        request,
        -32601,
        &format!("Method not found: {}", request.method),
    )
}
