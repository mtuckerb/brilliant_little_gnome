// Iroh transport: endpoint + gossip topic subscription.
//
// One Transport per device. Lifecycle:
//   start()      – binds an iroh Endpoint with the device's secret key,
//                  spawns the Gossip actor, derives the topic id from
//                  the shared sync_doc_secret, subscribes to it, and
//                  spins up two background tasks:
//                    * accept loop  — drains endpoint.accept() and hands
//                      each Connection to gossip.handle_connection
//                    * receiver pump — reads from the GossipReceiver
//                      stream and fans events out to a tokio broadcast
//                      channel (so multiple subscribers, e.g. the engine
//                      and any debug panel, can each see every event)
//   broadcast(b) – queues `b` onto the GossipSender; one gossip frame
//                  per call. Today the payload is raw `Vec<u8>`; T-007
//                  tightens the surface to `WireMsg` and moves
//                  postcard framing into this module.
//   shutdown()   – cancels both tasks, leaves the gossip topic, and
//                  closes the endpoint.
//
// Topic derivation lives in `topic_id_for(secret)` — a domain-separated
// blake3 of the sync_doc_secret. Bumping `TOPIC_DOMAIN` is a deliberate
// wire-break and should only happen at major version boundaries.
//
// API note: design.md and tickets.md predate iroh 0.98's `NodeId →
// EndpointId` rename. Externally we keep the spec's "peer / NodeId"
// language (TransportEvent uses `from: String` of a stringified
// EndpointId), internally we use the new types.

#![allow(dead_code)]

use crate::error::{AppError, Result};
use futures::StreamExt;
use hmac::{Hmac, Mac};
use iroh::{endpoint::presets, Endpoint, RelayMode, SecretKey};
use iroh_gossip::{
    api::{Event, GossipSender},
    net::{Gossip, GOSSIP_ALPN},
    proto::TopicId,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use tracing::{info, warn};

const TOPIC_DOMAIN: &[u8] = b"brilliant-sync-v1";

/// Channel capacity for the inbox broadcast. Consumers that lag past
/// this drop a window of events and recover via state-vector resync
/// (T-007 / T-008) — losing inbox messages is not data loss.
const INBOX_CAPACITY: usize = 1024;
const CREDENTIAL_ALPN: &[u8] = b"brilliant/credentials/1";
const MAX_CREDENTIAL_FRAME: usize = 64 * 1024;
const DIRECT_TIMEOUT: Duration = Duration::from_secs(15);

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
    /// Joiner→seed pairing handshake (T-013, design.md §6).
    /// The seed verifies `nonce` hasn't been consumed (one-shot per
    /// QR via `consumed_pairing_nonces`), records it, and replies
    /// with a `Snapshot`. Distinct from `StateRequest` because the
    /// nonce-check side effect only fires for the QR-pairing path —
    /// ongoing resync between already-paired devices uses StateRequest.
    PairingRequest { nonce: String },
    /// Seed→joiner one-shot Brightspace credential bootstrap. Sent right
    /// after the pairing `Snapshot` so a fresh device (e.g. iOS, which
    /// can't run the desktop child-webview login flow) can adopt the
    /// seed's already-authenticated session and skip first-time sign-in.
    ///
    /// Sent privately to the joiner after pairing, including explicit re-pairs.
    /// Credentials never enter the Loro doc, so they don't replicate
    /// onward beyond this one handshake.
    BootstrapCredentials {
        host: String,
        cookie: String,
        uid: Option<String>,
        user_id: Option<String>,
    },
    /// A device with a missing/invalid session asks one paired peer for its
    /// current session over a private authenticated connection.
    CredentialRequest { request_id: String, to: String },
    /// Reply to a targeted [`CredentialRequest`]. `credentials = None` is an
    /// explicit "mine is not usable" response, allowing the requester to move
    /// to the next peer immediately instead of waiting for a timeout.
    CredentialResponse {
        request_id: String,
        to: String,
        credentials: Option<PeerCredentials>,
    },
}

/// Brightspace credentials carried only in an authenticated, targeted P2P
/// response. This value is never written into the replicated Loro document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCredentials {
    pub host: String,
    pub cookie: String,
    pub uid: Option<String>,
    pub user_id: Option<String>,
}

impl WireMsg {
    fn is_credential_message(&self) -> bool {
        matches!(
            self,
            Self::BootstrapCredentials { .. }
                | Self::CredentialRequest { .. }
                | Self::CredentialResponse { .. }
        )
    }

    /// Encode via `postcard` (the same compact, schema-evolution-friendly
    /// format the rest of the iroh ecosystem uses). Errors here mean a
    /// programmer bug — Vec<u8> fields can't fail to serialize — so we
    /// surface as AppError::Other rather than threading postcard's error
    /// type through the public surface.
    pub fn encode(&self) -> Result<Vec<u8>> {
        postcard::to_allocvec(self).map_err(|e| AppError::Other(format!("wire encode: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes).map_err(|e| AppError::Other(format!("wire decode: {e}")))
    }
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    PeerConnected(String),
    PeerDisconnected(String),
    /// A typed message. Credential messages carry the authenticated remote
    /// endpoint identity; gossip messages carry the forwarding neighbor.
    /// Malformed inbound bytes are logged at
    /// warn and dropped before they reach a subscriber — the consumer
    /// never has to handle "what if this isn't a WireMsg".
    Message {
        from: String,
        payload: WireMsg,
    },
}

pub struct Transport {
    endpoint: Endpoint,
    gossip: Gossip,
    topic_id: TopicId,
    sender: Arc<Mutex<GossipSender>>,
    inbox: broadcast::Sender<TransportEvent>,
    connected_peers: Arc<RwLock<HashSet<String>>>,
    credential_key: [u8; 32],
    cancel: CancellationToken,
    // Drop handles abort the spawned tasks on Transport drop, even
    // without an explicit shutdown() call. Belt-and-suspenders alongside
    // the CancellationToken.
    _accept_task: AbortOnDropHandle<()>,
    _recv_task: AbortOnDropHandle<()>,
}

impl Transport {
    /// Derive the gossip TopicId from the shared sync_doc_secret.
    pub fn topic_id_for(secret: &[u8]) -> TopicId {
        let mut h = blake3::Hasher::new();
        h.update(secret);
        h.update(TOPIC_DOMAIN);
        TopicId::from_bytes(*h.finalize().as_bytes())
    }

    /// Production constructor: n0 defaults (DNS lookup + n0 relay).
    pub async fn start(secret_key: SecretKey, sync_doc_secret: &[u8]) -> Result<Self> {
        Self::start_with_bootstrap(secret_key, sync_doc_secret, Vec::new()).await
    }

    /// Like [`start`] but also seeds the gossip subscription with peers
    /// to actively dial on join. Used by the QR-pairing joiner so the
    /// mesh forms with the seed device immediately. Relays route connections;
    /// they do not discover group members.
    pub async fn start_with_bootstrap(
        secret_key: SecretKey,
        sync_doc_secret: &[u8],
        peers: Vec<iroh::EndpointId>,
    ) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![GOSSIP_ALPN.to_vec(), CREDENTIAL_ALPN.to_vec()])
            // Native N0 in iroh 0.98 only resolves via DNS. HTTPS lookup
            // also works on cellular/campus networks that filter DNS queries.
            .address_lookup(iroh::address_lookup::PkarrResolver::n0_dns())
            .relay_mode(RelayMode::Default)
            .bind()
            .await
            .map_err(|e| AppError::Other(format!("iroh bind: {e}")))?;
        Self::wire_up(endpoint, sync_doc_secret, peers).await
    }

    /// Test hook: caller already built the endpoint (so the test can
    /// install a custom relay map / MemoryLookup before bind).
    /// `bootstrap_peers` are peers to dial proactively when joining
    /// the topic — without one of these, the second device sits
    /// alone forever in localhost integration tests.
    #[doc(hidden)]
    pub async fn start_with_endpoint(
        endpoint: Endpoint,
        sync_doc_secret: &[u8],
        bootstrap_peers: Vec<iroh::EndpointId>,
    ) -> Result<Self> {
        Self::wire_up(endpoint, sync_doc_secret, bootstrap_peers).await
    }

    async fn wire_up(
        endpoint: Endpoint,
        sync_doc_secret: &[u8],
        bootstrap: Vec<iroh::EndpointId>,
    ) -> Result<Self> {
        endpoint.set_alpns(vec![GOSSIP_ALPN.to_vec(), CREDENTIAL_ALPN.to_vec()]);
        let credential_key = blake3::derive_key("brilliant/credentials/1", sync_doc_secret);
        // iroh-gossip's default max_message_size is 4 KB, which silently
        // drops any wire payload larger than that — fine for tiny
        // PairingRequest pings but our pairing Snapshot can be tens to
        // hundreds of KB once a real semester's overlays land in the doc.
        // The seed broadcasts the snapshot, `Sender::broadcast` returns
        // Ok, but the message is rejected in the protocol layer and the
        // joiner times out. Bumping to 4 MiB covers any realistic snapshot
        // size and is well within the 8 MiB QUIC stream window.
        const MAX_GOSSIP_PAYLOAD: usize = 4 * 1024 * 1024;
        let gossip = Gossip::builder()
            .max_message_size(MAX_GOSSIP_PAYLOAD)
            .spawn(endpoint.clone());
        let topic_id = Self::topic_id_for(sync_doc_secret);
        info!(
            "transport up: endpoint={} topic={} bootstrap_peers={}",
            endpoint.id(),
            topic_id,
            bootstrap.len()
        );

        let topic = gossip
            .subscribe(topic_id, bootstrap)
            .await
            .map_err(|e| AppError::Other(format!("gossip subscribe: {e}")))?;
        let (sender, mut receiver) = topic.split();

        let cancel = CancellationToken::new();
        let (inbox, _) = broadcast::channel::<TransportEvent>(INBOX_CAPACITY);
        let connected_peers = Arc::new(RwLock::new(HashSet::new()));

        // -- accept loop ----------------------------------------------------
        let accept_task = AbortOnDropHandle::new(tokio::spawn({
            let endpoint = endpoint.clone();
            let gossip = gossip.clone();
            let cancel = cancel.clone();
            let inbox = inbox.clone();
            async move {
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => break,
                        _ = connections.join_next(), if !connections.is_empty() => {},
                        maybe_inc = endpoint.accept() => {
                            let Some(incoming) = maybe_inc else { break };
                            if connections.len() >= 64 {
                                incoming.refuse();
                                continue;
                            }
                            let gossip = gossip.clone();
                            let inbox = inbox.clone();
                            let our_id = endpoint.id();
                            connections.spawn(async move {
                                let _ = tokio::time::timeout(DIRECT_TIMEOUT, async move {
                                    let connecting = match incoming.accept() {
                                        Ok(c) => c,
                                        Err(e) => { warn!("incoming accept err: {e}"); return; }
                                    };
                                    let conn = match connecting.await {
                                        Ok(c) => c,
                                        Err(e) => { warn!("connecting await err: {e}"); return; }
                                    };
                                    if conn.alpn() == CREDENTIAL_ALPN {
                                        if let Err(e) = receive_credentials(conn, our_id, &credential_key, &inbox).await {
                                            warn!("credential connection rejected: {e}");
                                        }
                                    } else if let Err(e) = gossip.handle_connection(conn).await {
                                        warn!("gossip handle_connection: {e}");
                                    }
                                }).await;
                            });
                        }
                    }
                }
            }
        }));

        // -- receiver pump --------------------------------------------------
        let recv_task = AbortOnDropHandle::new(tokio::spawn({
            let inbox = inbox.clone();
            let cancel = cancel.clone();
            let connected_peers = connected_peers.clone();
            async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => break,
                        next = receiver.next() => {
                            let Some(item) = next else { break };
                            let ev = match item {
                                Ok(e) => e,
                                Err(e) => { warn!("gossip recv err: {e}"); continue; }
                            };
                            let out = match ev {
                                Event::NeighborUp(id) => {
                                    let id = id.to_string();
                                    connected_peers.write().insert(id.clone());
                                    Some(TransportEvent::PeerConnected(id))
                                }
                                Event::NeighborDown(id) => {
                                    let id = id.to_string();
                                    connected_peers.write().remove(&id);
                                    Some(TransportEvent::PeerDisconnected(id))
                                }
                                Event::Received(msg) => {
                                    match WireMsg::decode(&msg.content) {
                                        // Credentials must have an authenticated origin and
                                        // must never be forwarded through the gossip mesh.
                                        Ok(payload) if payload.is_credential_message() => None,
                                        Ok(payload) => Some(TransportEvent::Message {
                                            from: msg.delivered_from.to_string(),
                                            payload,
                                        }),
                                        Err(e) => {
                                            warn!(
                                                "dropping {}-byte message from {}: {e}",
                                                msg.content.len(),
                                                msg.delivered_from,
                                            );
                                            None
                                        }
                                    }
                                }
                                Event::Lagged => {
                                    warn!("gossip receiver lagged");
                                    None
                                }
                            };
                            if let Some(ev) = out {
                                // send returns Err only if there are zero
                                // active receivers — that's a normal state
                                // (engine hasn't subscribed yet, etc.), not
                                // an error worth logging on every message.
                                let _ = inbox.send(ev);
                            }
                        }
                    }
                }
            }
        }));

        Ok(Self {
            endpoint,
            gossip,
            topic_id,
            sender: Arc::new(Mutex::new(sender)),
            inbox,
            connected_peers,
            credential_key,
            cancel,
            _accept_task: accept_task,
            _recv_task: recv_task,
        })
    }

    pub fn endpoint_id(&self) -> iroh::EndpointId {
        self.endpoint.id()
    }

    pub fn topic_id(&self) -> TopicId {
        self.topic_id
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Subscribe to inbound events. Each call returns a fresh receiver;
    /// every subscriber sees a copy of every subsequent event.
    pub fn subscribe(&self) -> broadcast::Receiver<TransportEvent> {
        self.inbox.subscribe()
    }

    /// Whether the gossip topic currently has a route to at least one peer.
    /// Recovery waits briefly for this on cold start so its first request is
    /// not broadcast before relay/DNS discovery has formed the mesh.
    pub fn has_connected_peers(&self) -> bool {
        !self.connected_peers.read().is_empty()
    }

    pub fn is_peer_connected(&self, peer: &str) -> bool {
        self.connected_peers.read().contains(peer)
    }

    /// Dial the authenticated endpoint directly. Iroh can carry this QUIC
    /// connection through a relay; no LAN route or gossip neighbor is needed.
    pub async fn send_credentials(&self, peer: iroh::EndpointId, msg: WireMsg) -> Result<()> {
        if !msg.is_credential_message() {
            return Err(AppError::BadRequest("expected credential message".into()));
        }
        let payload = msg.encode()?;
        let mac = credential_mac(&self.credential_key, self.endpoint_id(), peer, &payload);
        let frame = postcard::to_allocvec(&(payload, mac.finalize().into_bytes().to_vec()))
            .map_err(|_| AppError::Other("credential frame encoding failed".into()))?;
        if frame.len() > MAX_CREDENTIAL_FRAME {
            return Err(AppError::BadRequest("credential frame too large".into()));
        }
        tokio::time::timeout(DIRECT_TIMEOUT, async {
            let conn = self
                .endpoint
                .connect(peer, CREDENTIAL_ALPN)
                .await
                .map_err(|_| AppError::Other("credential peer unavailable".into()))?;
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|_| AppError::Other("credential stream unavailable".into()))?;
            send.write_all(&frame)
                .await
                .map_err(|_| AppError::Other("credential send failed".into()))?;
            send.finish()
                .map_err(|_| AppError::Other("credential send failed".into()))?;
            let ack = recv
                .read_to_end(1)
                .await
                .map_err(|_| AppError::Other("credential acknowledgement failed".into()))?;
            conn.close(0u32.into(), b"done");
            if ack != [1] {
                return Err(AppError::Other("credential message rejected".into()));
            }
            Ok(())
        })
        .await
        .map_err(|_| AppError::Other("credential peer timed out".into()))?
    }

    /// Send a typed `WireMsg` over the gossip topic. Postcard framing
    /// is internal — callers never see encoded bytes.
    pub async fn broadcast(&self, msg: WireMsg) -> Result<()> {
        if msg.is_credential_message() {
            return Err(AppError::BadRequest(
                "credentials require a private connection".into(),
            ));
        }
        let bytes = msg.encode()?;
        self.sender
            .lock()
            .await
            .broadcast(Bytes::from(bytes))
            .await
            .map_err(|e| AppError::Other(format!("gossip broadcast: {e}")))
    }

    pub async fn shutdown(self) -> Result<()> {
        self.cancel.cancel();
        let _ = self.gossip.shutdown().await;
        self.endpoint.close().await;
        Ok(())
    }
}

fn credential_mac(
    key: &[u8; 32],
    from: iroh::EndpointId,
    to: iroh::EndpointId,
    payload: &[u8],
) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("fixed-size HMAC key");
    mac.update(from.as_bytes());
    mac.update(to.as_bytes());
    mac.update(payload);
    mac
}

async fn receive_credentials(
    conn: iroh::endpoint::Connection,
    our_id: iroh::EndpointId,
    key: &[u8; 32],
    inbox: &broadcast::Sender<TransportEvent>,
) -> Result<()> {
    let from = conn.remote_id();
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|_| AppError::Other("credential stream unavailable".into()))?;
    let frame = recv
        .read_to_end(MAX_CREDENTIAL_FRAME)
        .await
        .map_err(|_| AppError::Other("invalid credential frame".into()))?;
    let (payload, tag): (Vec<u8>, Vec<u8>) = postcard::from_bytes(&frame)
        .map_err(|_| AppError::Other("invalid credential frame".into()))?;
    credential_mac(key, from, our_id, &payload)
        .verify_slice(&tag)
        .map_err(|_| AppError::Other("credential group authentication failed".into()))?;
    let msg = WireMsg::decode(&payload)?;
    if !msg.is_credential_message() {
        return Err(AppError::BadRequest("unexpected private message".into()));
    }
    inbox
        .send(TransportEvent::Message {
            from: from.to_string(),
            payload: msg,
        })
        .map_err(|_| AppError::Other("credential receiver unavailable".into()))?;
    send.write_all(&[1])
        .await
        .map_err(|_| AppError::Other("credential acknowledgement failed".into()))?;
    send.finish()
        .map_err(|_| AppError::Other("credential acknowledgement failed".into()))?;
    // Keep the connection alive until the sender has received the ACK.
    let _ = send.stopped().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_wire_round_trip(msg: WireMsg) {
        let bytes = msg.encode().unwrap();
        let decoded = WireMsg::decode(&bytes).unwrap();
        // Compare via debug repr so we don't have to derive PartialEq on
        // WireMsg (the inner Vec<u8> already compares fine, but keeping
        // the public type minimal is worth the assert acrobatics).
        assert_eq!(format!("{decoded:?}"), format!("{msg:?}"));
    }

    #[test]
    fn wire_msg_round_trip_update() {
        assert_wire_round_trip(WireMsg::Update { bytes: vec![] });
        assert_wire_round_trip(WireMsg::Update {
            bytes: vec![0; 4096],
        });
    }

    #[test]
    fn wire_msg_round_trip_snapshot() {
        assert_wire_round_trip(WireMsg::Snapshot {
            bytes: b"snap".to_vec(),
        });
    }

    #[test]
    fn wire_msg_round_trip_state_request() {
        assert_wire_round_trip(WireMsg::StateRequest {
            vv: vec![1, 2, 3, 4, 5],
        });
    }

    #[test]
    fn wire_msg_round_trip_state_response() {
        assert_wire_round_trip(WireMsg::StateResponse {
            vv: vec![9, 8, 7],
            bytes: vec![0xde, 0xad, 0xbe, 0xef],
        });
    }

    #[test]
    fn wire_msg_round_trip_pairing_request() {
        assert_wire_round_trip(WireMsg::PairingRequest {
            nonce: "deadbeef0123456789abcdef01234567".into(),
        });
        // Empty nonce (defensive — the protocol-level rejection will
        // happen at the consume_nonce gate, not at decode time).
        assert_wire_round_trip(WireMsg::PairingRequest {
            nonce: String::new(),
        });
    }

    #[test]
    fn wire_msg_round_trip_credential_exchange() {
        assert_wire_round_trip(WireMsg::CredentialRequest {
            request_id: "request-1".into(),
            to: "peer-a".into(),
        });
        assert_wire_round_trip(WireMsg::CredentialResponse {
            request_id: "request-1".into(),
            to: "requester".into(),
            credentials: Some(PeerCredentials {
                host: "courses.example.edu".into(),
                cookie: "d2lSessionVal=one; d2lSecureSessionVal=two".into(),
                uid: Some("uid".into()),
                user_id: Some("42".into()),
            }),
        });
        assert_wire_round_trip(WireMsg::CredentialResponse {
            request_id: "request-2".into(),
            to: "requester".into(),
            credentials: None,
        });
    }

    #[test]
    fn wire_msg_decode_rejects_garbage() {
        // Postcard rejects a discriminator outside the wire enum.
        let bad = vec![99u8, 0, 0, 0];
        assert!(WireMsg::decode(&bad).is_err());
        // And empty input.
        assert!(WireMsg::decode(&[]).is_err());
    }

    #[test]
    fn topic_id_is_deterministic_and_domain_separated() {
        let s = b"shared-secret";
        let t1 = Transport::topic_id_for(s);
        let t2 = Transport::topic_id_for(s);
        assert_eq!(t1, t2, "topic id must be deterministic for a given secret");

        // Same input, different domain (i.e. a hypothetical v2 protocol)
        // must produce a different topic — that's the point of the
        // domain separator.
        let mut h = blake3::Hasher::new();
        h.update(s);
        h.update(b"brilliant-sync-v2");
        let t_v2 = TopicId::from_bytes(*h.finalize().as_bytes());
        assert_ne!(t1, t_v2);
    }

    /// Ticket-mandated integration test: two Transports sharing a
    /// `sync_doc_secret`, broadcast from one, the other receives within 2s.
    ///
    /// Uses iroh's test-utils relay (dev-dep feature) and an in-memory
    /// address lookup so the two endpoints discover each other without
    /// hitting any production infrastructure.
    ///
    /// Setup pattern follows iroh-gossip's own `gossip_net_smoke` test:
    ///   - shared `MemoryLookup` registered on each endpoint POST-bind
    ///     (the runtime path, not the builder path — registering
    ///     pre-bind doesn't share state across endpoints)
    ///   - `CaRootsConfig::insecure_skip_verify()` so the test relay's
    ///     self-signed cert is accepted
    ///   - whole body wrapped in a 30s timeout so a regression fails
    ///     loud + fast instead of hanging forever in CI
    #[tokio::test]
    async fn two_transports_round_trip_a_broadcast() {
        use iroh::{
            address_lookup::memory::MemoryLookup, endpoint::presets::Minimal, tls::CaRootsConfig,
            EndpointAddr,
        };
        use std::time::Duration;
        use tokio::time::timeout;

        let (relay_map, relay_url, _relay_guard) =
            iroh::test_utils::run_relay_server().await.unwrap();

        let memory_lookup = MemoryLookup::new();

        async fn build_ep(relay_map: iroh::RelayMap) -> Endpoint {
            Endpoint::builder(Minimal)
                .secret_key(SecretKey::generate())
                .alpns(vec![GOSSIP_ALPN.to_vec()])
                .relay_mode(RelayMode::Custom(relay_map))
                .ca_roots_config(CaRootsConfig::insecure_skip_verify())
                .bind()
                .await
                .unwrap()
        }

        let body = async {
            let ep1 = build_ep(relay_map.clone()).await;
            let ep2 = build_ep(relay_map.clone()).await;
            // Register the shared MemoryLookup at runtime so both
            // endpoints resolve each other against the same store.
            ep1.address_lookup().unwrap().add(memory_lookup.clone());
            ep2.address_lookup().unwrap().add(memory_lookup.clone());
            ep1.online().await;
            ep2.online().await;

            let id1 = ep1.id();
            let id2 = ep2.id();

            // Cross-register addresses so each gossip can dial the other
            // by EndpointId without a real DNS lookup.
            memory_lookup
                .add_endpoint_info(EndpointAddr::new(id1).with_relay_url(relay_url.clone()));
            memory_lookup
                .add_endpoint_info(EndpointAddr::new(id2).with_relay_url(relay_url.clone()));

            let secret = b"shared-doc-secret-aabbccdd";
            // ep1 starts alone; ep2 bootstraps off ep1.
            let t1 = Transport::start_with_endpoint(ep1, secret, vec![])
                .await
                .unwrap();
            let t2 = Transport::start_with_endpoint(ep2, secret, vec![id1])
                .await
                .unwrap();

            assert_eq!(t1.topic_id(), t2.topic_id(), "shared secret → shared topic");

            let mut rx1 = t1.subscribe();
            let mut rx2 = t2.subscribe();

            // Wait for both ends to register a neighbor before broadcasting.
            // Without this, the broadcast can race ahead of the gossip mesh
            // and the receiver never sees it.
            let join = async {
                let mut joined1 = false;
                let mut joined2 = false;
                while !(joined1 && joined2) {
                    tokio::select! {
                        ev = rx1.recv(), if !joined1 => {
                            if let Ok(TransportEvent::PeerConnected(_)) = ev { joined1 = true; }
                        }
                        ev = rx2.recv(), if !joined2 => {
                            if let Ok(TransportEvent::PeerConnected(_)) = ev { joined2 = true; }
                        }
                    }
                }
            };
            timeout(Duration::from_secs(15), join)
                .await
                .expect("peers did not join the topic in time");

            // Now broadcast from t1 and assert t2 sees the typed message.
            t1.broadcast(WireMsg::Update {
                bytes: b"hello-from-t1".to_vec(),
            })
            .await
            .unwrap();
            let received = timeout(Duration::from_secs(2), async {
                loop {
                    if let Ok(TransportEvent::Message { from, payload }) = rx2.recv().await {
                        return (from, payload);
                    }
                }
            })
            .await
            .expect("t2 did not receive broadcast within 2s");

            assert_eq!(received.0, t1.endpoint_id().to_string());
            match received.1 {
                WireMsg::Update { bytes } => assert_eq!(bytes, b"hello-from-t1"),
                other => panic!("expected Update, got {other:?}"),
            }

            // Reverse direction sanity check with a different variant.
            t2.broadcast(WireMsg::StateRequest {
                vv: vec![0xaa, 0xbb],
            })
            .await
            .unwrap();
            let received = timeout(Duration::from_secs(2), async {
                loop {
                    if let Ok(TransportEvent::Message { payload, .. }) = rx1.recv().await {
                        if matches!(&payload, WireMsg::StateRequest { vv } if vv == &[0xaa, 0xbb]) {
                            return payload;
                        }
                    }
                }
            })
            .await
            .expect("t1 did not receive reply within 2s");
            assert!(matches!(received, WireMsg::StateRequest { .. }));

            let _ = id2; // silence unused warn; we only bootstrap from id1.
            t1.shutdown().await.unwrap();
            t2.shutdown().await.unwrap();
        };

        timeout(Duration::from_secs(30), body)
            .await
            .expect("transport integration test exceeded 30s");
    }
}
