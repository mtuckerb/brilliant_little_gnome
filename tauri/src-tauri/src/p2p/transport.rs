// Iroh transport (endpoint + gossip topic).
//
// T-006 builds the actual `iroh::Endpoint` and `iroh_gossip::Gossip`
// instance. iroh's API has churned ~64 minor versions since the spec
// was written, so the ticket sketch in tickets.md is stale — read
// n0-computer/iroh-examples HEAD before implementing T-006.
//
// T-007 finalizes WireMsg framing (postcard).

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Owns the iroh endpoint, the gossip topic subscription, and the
/// inbound/outbound channels used by the engine. Built by T-006.
pub struct Transport {
    // T-006:
    //   endpoint: iroh::Endpoint,
    //   gossip:   iroh_gossip::net::Gossip (or Gossip handle),
    //   topic:    iroh_gossip::proto::TopicId,
    //   outbox:   tokio::sync::mpsc::Sender<Vec<u8>>,
    //   inbox:    tokio::sync::broadcast::Sender<TransportEvent>,
}

/// Wire-format messages exchanged across the gossip topic.
/// Frozen by T-007. Versioning is implicit in the enum tag — we will
/// only ever add new variants; renames or removals are wire-breaking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMsg {
    /// Loro incremental update (`doc.export(ExportMode::updates_since(..))`).
    Update { bytes: Vec<u8> },
    /// Full Loro snapshot (`ExportMode::Snapshot`). Sent on first connect,
    /// periodically, and on explicit request.
    Snapshot { bytes: Vec<u8> },
    /// Send your state vector to ask "what am I missing?"
    StateRequest { vv: Vec<u8> },
    /// Reply with the state vector + the missing-since-vv updates.
    StateResponse { vv: Vec<u8>, bytes: Vec<u8> },
}

/// Events surfaced by the transport task to the engine.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// NodeId is opaque to this layer; stringified for logging /
    /// crossing async boundaries without pulling iroh types up here.
    PeerConnected(String),
    PeerDisconnected(String),
    Message { from: String, payload: Vec<u8> },
}
