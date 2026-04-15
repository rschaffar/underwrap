use anyhow::{Result, anyhow};

use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::errors::BackendErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMgmtRequest {
    ListStores,
    ListExports,
    ExportSocket {
        store: String,
        socket: String,
        mode: u32,
    },
    UnexportSocket {
        socket: String,
    },
    GetPolicy {
        store: String,
    },
    PutPolicy {
        store: String,
        document_json: String,
    },
}

impl WireEncode for BackendMgmtRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ListStores => 0u8.encode(out),
            Self::ListExports => 1u8.encode(out),
            Self::ExportSocket {
                store,
                socket,
                mode,
            } => {
                2u8.encode(out)?;
                store.encode(out)?;
                socket.encode(out)?;
                mode.encode(out)
            }
            Self::UnexportSocket { socket } => {
                3u8.encode(out)?;
                socket.encode(out)
            }
            Self::GetPolicy { store } => {
                4u8.encode(out)?;
                store.encode(out)
            }
            Self::PutPolicy {
                store,
                document_json,
            } => {
                5u8.encode(out)?;
                store.encode(out)?;
                document_json.encode(out)
            }
        }
    }
}

impl WireDecode for BackendMgmtRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ListStores),
            1 => Ok(Self::ListExports),
            2 => Ok(Self::ExportSocket {
                store: String::decode(reader)?,
                socket: String::decode(reader)?,
                mode: u32::decode(reader)?,
            }),
            3 => Ok(Self::UnexportSocket {
                socket: String::decode(reader)?,
            }),
            4 => Ok(Self::GetPolicy {
                store: String::decode(reader)?,
            }),
            5 => Ok(Self::PutPolicy {
                store: String::decode(reader)?,
                document_json: String::decode(reader)?,
            }),
            other => Err(anyhow!(
                "invalid backend management request discriminant: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendExportStatus {
    pub store: String,
    pub socket: String,
    pub mode: u32,
}

impl WireEncode for BackendExportStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.store.encode(out)?;
        self.socket.encode(out)?;
        self.mode.encode(out)
    }
}

impl WireDecode for BackendExportStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            store: String::decode(reader)?,
            socket: String::decode(reader)?,
            mode: u32::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMgmtResponse {
    ListStoresOk {
        stores: Vec<String>,
    },
    ListExportsOk {
        exports: Vec<BackendExportStatus>,
    },
    ExportSocketOk,
    UnexportSocketOk {
        existed: bool,
    },
    GetPolicyOk {
        document_json: Option<String>,
    },
    PutPolicyOk,
    Error {
        code: BackendErrorCode,
        message: String,
    },
}

impl WireEncode for BackendMgmtResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ListStoresOk { stores } => {
                0u8.encode(out)?;
                stores.encode(out)
            }
            Self::ListExportsOk { exports } => {
                1u8.encode(out)?;
                exports.encode(out)
            }
            Self::ExportSocketOk => 2u8.encode(out),
            Self::UnexportSocketOk { existed } => {
                3u8.encode(out)?;
                existed.encode(out)
            }
            Self::GetPolicyOk { document_json } => {
                4u8.encode(out)?;
                document_json.encode(out)
            }
            Self::PutPolicyOk => 5u8.encode(out),
            Self::Error { code, message } => {
                6u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for BackendMgmtResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ListStoresOk {
                stores: Vec::<String>::decode(reader)?,
            }),
            1 => Ok(Self::ListExportsOk {
                exports: Vec::<BackendExportStatus>::decode(reader)?,
            }),
            2 => Ok(Self::ExportSocketOk),
            3 => Ok(Self::UnexportSocketOk {
                existed: bool::decode(reader)?,
            }),
            4 => Ok(Self::GetPolicyOk {
                document_json: Option::<String>::decode(reader)?,
            }),
            5 => Ok(Self::PutPolicyOk),
            6 => Ok(Self::Error {
                code: BackendErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!(
                "invalid backend management response discriminant: {other}"
            )),
        }
    }
}
