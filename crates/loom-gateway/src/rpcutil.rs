//! JSON-RPC 2.0 helpers shared by the real gateway and fixture replay (contract 01 §1.2).

use loom_proto::envelope::ErrorBody;
use loom_proto::error::ErrorCode;
use loom_proto::rpc::{
    INVALID_PARAMS, INVALID_REQUEST, Id, MAX_MESSAGE_BYTES, PARSE_ERROR, Request, Response, RpcError,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// JSON-RPC application error code carrying a Loom [`ErrorBody`] in `data`.
pub const APP_ERROR: i64 = -32000;

/// Spec error.
pub fn rpc_error(code: i64, message: impl Into<String>) -> RpcError {
    RpcError {
        code,
        message: message.into(),
        data: None,
    }
}

/// Loom error as JSON-RPC error (`-32000`, `data` = `ErrorBody`).
pub fn app_error(code: ErrorCode, message: &str) -> RpcError {
    app_error_detail(code, message, Value::Null)
}

/// Loom error with detail.
pub fn app_error_detail(code: ErrorCode, message: &str, detail: Value) -> RpcError {
    let body = ErrorBody {
        code,
        message: message.into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail,
    };
    RpcError {
        code: APP_ERROR,
        message: message.into(),
        data: serde_json::to_value(body).ok(),
    }
}

/// Deserialize params (missing = `{}`), `-32602` on mismatch.
pub fn params<T: DeserializeOwned>(p: Option<Value>) -> Result<T, RpcError> {
    serde_json::from_value(p.unwrap_or_else(|| json!({})))
        .map_err(|e| rpc_error(INVALID_PARAMS, e.to_string()))
}

/// Serialize a response.
pub fn resp(r: Response) -> String {
    serde_json::to_string(&r).unwrap_or_default()
}

/// Parse one text frame into a request, or produce the error response to send.
pub fn parse_request(text: &str) -> Result<Request, String> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(resp(Response::err(
            None,
            rpc_error(INVALID_REQUEST, "message exceeds 64 KiB"),
        )));
    }
    let v: Value = serde_json::from_str(text)
        .map_err(|e| resp(Response::err(None, rpc_error(PARSE_ERROR, e.to_string()))))?;
    if v.is_array() {
        return Err(resp(Response::err(
            None,
            rpc_error(INVALID_REQUEST, "batch requests are not supported"),
        )));
    }
    let id: Option<Id> = v.get("id").cloned().and_then(|i| serde_json::from_value(i).ok());
    let req: Request = serde_json::from_value(v).map_err(|e| {
        resp(Response::err(
            id.clone(),
            rpc_error(INVALID_REQUEST, e.to_string()),
        ))
    })?;
    if req.jsonrpc != "2.0" {
        return Err(resp(Response::err(
            id,
            rpc_error(INVALID_REQUEST, "jsonrpc must be 2.0"),
        )));
    }
    Ok(req)
}

/// Final response text for a request result (`None` for notifications).
pub fn reply(id: Option<Id>, r: Result<Value, RpcError>) -> Option<String> {
    let id = id?;
    Some(resp(match r {
        Ok(v) => Response::ok(Some(id), v),
        Err(e) => Response::err(Some(id), e),
    }))
}

/// Loom error body carried by a JSON-RPC error (`-32000`), or an internal one.
pub fn error_body(e: &RpcError) -> ErrorBody {
    e.data
        .as_ref()
        .and_then(|d| serde_json::from_value::<ErrorBody>(d.clone()).ok())
        .unwrap_or(ErrorBody {
            code: ErrorCode::Internal,
            message: e.message.clone(),
            hint: None,
            retryable: false,
            request_id: None,
            detail: Value::Null,
        })
}
