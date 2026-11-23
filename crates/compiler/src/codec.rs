//! Binary serialization and versioned wire envelope for [`crate::CompiledDocument`] (`preview` feature only).
//!
//! Used by `krui serve` to send freshly compiled `.krui`/`.css` IR bundles to a running preview
//! application over TCP or WebSocket.

use crate::CompiledDocument;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Magic 4-byte wire header (`"KRUI"`) prepended to serialized IR bundles.
pub const IR_WIRE_MAGIC: [u8; 4] = *b"KRUI";

/// Wire format version for [`CompiledDocument`] binary bundles.
pub const IR_FORMAT_VERSION: u32 = 8;

/// Versioned envelope wrapping a [`CompiledDocument`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VersionedIrBundle {
    /// IR wire format version (`IR_FORMAT_VERSION`).
    pub version: u32,
    /// Compiled document IR payload.
    pub document: CompiledDocument,
}

/// Error returned when encoding or decoding a [`CompiledDocument`] IR bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrCodecError {
    /// The wire payload did not start with `IR_WIRE_MAGIC` (`"KRUI"`).
    InvalidMagic,
    /// The wire payload's version header did not match [`IR_FORMAT_VERSION`].
    VersionMismatch {
        /// Version expected by the decoder (`IR_FORMAT_VERSION`).
        expected: u32,
        /// Version found in the wire header.
        found: u32,
    },
    /// Binary serialization or deserialization failed.
    Codec(String),
}

impl fmt::Display for IrCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => {
                f.write_str("Invalid KRui IR wire magic header (expected `KRUI`)")
            }
            Self::VersionMismatch { expected, found } => write!(
                f,
                "KRui IR format version mismatch: expected v{expected}, found v{found}"
            ),
            Self::Codec(msg) => write!(f, "KRui IR binary codec error: {msg}"),
        }
    }
}

impl std::error::Error for IrCodecError {}

const CODEC_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn run_on_codec_stack<T: Send, E: ToString + Send, F: FnOnce() -> Result<T, E> + Send>(
    f: F,
) -> Result<T, IrCodecError> {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("krui-ir-codec".to_string())
            .stack_size(CODEC_WORKER_STACK_BYTES)
            .spawn_scoped(scope, f)
            .map_err(|e| IrCodecError::Codec(format!("spawn codec worker: {e}")))?;
        match handle.join() {
            Ok(res) => res.map_err(|e| IrCodecError::Codec(e.to_string())),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

/// Serializes `document` into a versioned binary byte buffer (`[magic: 4B][version: u32 LE][postcard payload]`).
pub fn encode_compiled_document(document: &CompiledDocument) -> Result<Vec<u8>, IrCodecError> {
    encode_compiled_document_with_version(document, IR_FORMAT_VERSION)
}

/// Serializes `document` with an explicit `version` header (used for version-mismatch testing).
pub fn encode_compiled_document_with_version(
    document: &CompiledDocument,
    version: u32,
) -> Result<Vec<u8>, IrCodecError> {
    let payload = run_on_codec_stack(|| postcard::to_stdvec(document))?;
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&IR_WIRE_MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Deserializes a versioned binary byte buffer into a [`CompiledDocument`], verifying the magic
/// header and [`IR_FORMAT_VERSION`] before decoding the payload.
pub fn decode_compiled_document(bytes: &[u8]) -> Result<CompiledDocument, IrCodecError> {
    if bytes.len() < 8 || bytes[0..4] != IR_WIRE_MAGIC {
        return Err(IrCodecError::InvalidMagic);
    }
    let found_version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if found_version != IR_FORMAT_VERSION {
        return Err(IrCodecError::VersionMismatch {
            expected: IR_FORMAT_VERSION,
            found: found_version,
        });
    }
    run_on_codec_stack(|| postcard::from_bytes(&bytes[8..]))
}
