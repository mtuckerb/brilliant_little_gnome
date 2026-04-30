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
use tokio_util::bytes::Bytes;
use futures::StreamExt;
use iroh::{endpoint::presets, Endpoint, RelayMode, SecretKey};
use iroh_gossip::{
    api::{Event, GossipSender},
    net::{Gossip, GOSSIP_ALPN},
    proto::TopicId,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use tracing::warn;

const TOPIC_DOMAIN: &[u8] = b"brilliant-sync-v1";

/// Channel capacity for the inbox broadcast. Consumers that lag past
/// this drop a window of events and recover via state-vector resync
/// (T-007 / T-008) — losing inbox messages is not data loss.
const INBOX_CAPACITY: usize = 1024;

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

#[derive(Debug, Clone)]
pub enum TransportEvent {
    PeerConnected(String),
    PeerDisconnected(String),
    Message { from: String, payload: Vec<u8> },
}

pub struct Transport {
    endpoint: Endpoint,
    gossip: Gossip,
    topic_id: TopicId,
    sender: Arc<Mutex<GossipSender>>,
    inbox: broadcast::Sender<TransportEvent>,
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
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![GOSSIP_ALPN.to_vec()])
            .relay_mode(RelayMode::Default)
            .bind()
            .await
            .map_err(|e| AppError::Other(format!("iroh bind: {e}")))?;
        Self::wire_up(endpoint, sync_doc_secret, Vec::new()).await
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
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let topic_id = Self::topic_id_for(sync_doc_secret);

        let topic = gossip
            .subscribe(topic_id, bootstrap)
            .await
            .map_err(|e| AppError::Other(format!("gossip subscribe: {e}")))?;
        let (sender, mut receiver) = topic.split();

        let cancel = CancellationToken::new();
        let (inbox, _) = broadcast::channel::<TransportEvent>(INBOX_CAPACITY);

        // -- accept loop ----------------------------------------------------
        let accept_task = AbortOnDropHandle::new(tokio::spawn({
            let endpoint = endpoint.clone();
            let gossip = gossip.clone();
            let cancel = cancel.clone();
            async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => break,
                        maybe_inc = endpoint.accept() => {
                            let Some(incoming) = maybe_inc else { break };
                            let connecting = match incoming.accept() {
                                Ok(c) => c,
                                Err(e) => { warn!("incoming accept err: {e}"); continue; }
                            };
                            let conn = match connecting.await {
                                Ok(c) => c,
                                Err(e) => { warn!("connecting await err: {e}"); continue; }
                            };
                            if let Err(e) = gossip.handle_connection(conn).await {
                                warn!("gossip handle_connection: {e}");
                            }
                        }
                    }
                }
            }
        }));

        // -- receiver pump --------------------------------------------------
        let recv_task = AbortOnDropHandle::new(tokio::spawn({
            let inbox = inbox.clone();
            let cancel = cancel.clone();
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
                                Event::NeighborUp(id) =>
                                    Some(TransportEvent::PeerConnected(id.to_string())),
                                Event::NeighborDown(id) =>
                                    Some(TransportEvent::PeerDisconnected(id.to_string())),
                                Event::Received(msg) =>
                                    Some(TransportEvent::Message {
                                        from: msg.delivered_from.to_string(),
                                        payload: msg.content.to_vec(),
                                    }),
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

    pub async fn broadcast(&self, payload: Vec<u8>) -> Result<()> {
        self.sender
            .lock()
            .await
            .broadcast(Bytes::from(payload))
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

#[cfg(test)]
mod tests {
    use super::*;

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
            address_lookup::memory::MemoryLookup,
            endpoint::presets::Minimal,
            tls::CaRootsConfig,
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
            let t1 = Transport::start_with_endpoint(ep1, secret, vec![]).await.unwrap();
            let t2 = Transport::start_with_endpoint(ep2, secret, vec![id1]).await.unwrap();

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

            // Now broadcast from t1 and assert t2 sees the bytes.
            t1.broadcast(b"hello-from-t1".to_vec()).await.unwrap();
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
            assert_eq!(received.1, b"hello-from-t1");

            // Reverse direction sanity check.
            t2.broadcast(b"reply-from-t2".to_vec()).await.unwrap();
            let received = timeout(Duration::from_secs(2), async {
                loop {
                    if let Ok(TransportEvent::Message { payload, .. }) = rx1.recv().await {
                        if payload == b"reply-from-t2" {
                            return payload;
                        }
                    }
                }
            })
            .await
            .expect("t1 did not receive reply within 2s");
            assert_eq!(received, b"reply-from-t2");

            let _ = id2; // silence unused warn; we only bootstrap from id1.
            t1.shutdown().await.unwrap();
            t2.shutdown().await.unwrap();
        };

        timeout(Duration::from_secs(30), body)
            .await
            .expect("transport integration test exceeded 30s");
    }
}
