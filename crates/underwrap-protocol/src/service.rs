use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatusResponse {
    pub ok: bool,
    pub principal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadRequest {
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadResponse {
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceWriteRequest {
    pub secret_id: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceWriteResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceListRequest {
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceListResponse {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceDeleteRequest {
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceDeleteResponse {
    pub existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceErrorResponse {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_structs_roundtrip_with_json() {
        let req = ServiceWriteRequest {
            secret_id: "github/token".to_string(),
            material_b64: "Z2hwX3Rlc3Q=".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ServiceWriteRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }
}
