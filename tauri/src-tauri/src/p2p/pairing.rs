// QR-code pairing flow. Design.md §6.
//
// T-012 implements the QR payload + parser + nonce table.
// T-013 implements the actual handshake protocol over the transport.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// JSON-serialised payload encoded into the pairing QR code. The fields
/// match design.md §6: NodeId in z32, candidate addrs (best-effort dialing
/// hint), optional relay URL, the shared sync_doc_secret, a one-shot
/// nonce, and an absolute expiry. Implemented in T-012.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingPayload {
    pub v: u8,                  // == 1
    pub node: String,           // NodeId in z32
    pub addrs: Vec<String>,     // direct addrs (best-effort hint)
    pub relay: Option<String>,  // relay url
    pub secret: String,         // sync_doc_secret hex
    pub nonce: String,          // 16 random bytes hex; one-shot
    pub exp: i64,               // unix ts; ~60s in the future
}
