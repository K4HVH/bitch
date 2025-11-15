use crate::rules::Action;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{sleep, Instant};
use tracing::{debug, info, warn};

/// Destination for forwarding packets
#[derive(Clone)]
pub enum Destination {
    /// Send to Router (TCP stream write half)
    Router(Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>),
}

/// Result of queuing a message to a batch
#[derive(Debug)]
pub enum BatchResult {
    /// Message queued, still waiting for more
    Queued,
    /// Threshold met or timeout occurred, release all packets with remaining actions
    Release {
        packets: Vec<Vec<u8>>,
        remaining_actions: Vec<Action>,
    },
}

/// A single queued packet
type QueuedPacket = Vec<u8>;

/// State for a single batch group
#[derive(Debug)]
struct BatchState {
    /// All queued packets
    packets: Vec<QueuedPacket>,
    /// Unique system IDs seen
    systems: HashSet<u8>,
    /// Target threshold
    threshold: usize,
    /// When this batch was created
    created_at: Instant,
    /// Whether to forward on timeout
    forward_on_timeout: bool,
    /// Remaining actions to apply after batch releases
    remaining_actions: Vec<Action>,
}

impl BatchState {
    fn new(threshold: usize, forward_on_timeout: bool, remaining_actions: Vec<Action>) -> Self {
        Self {
            packets: Vec::new(),
            systems: HashSet::new(),
            threshold,
            created_at: Instant::now(),
            forward_on_timeout,
            remaining_actions,
        }
    }

    fn add_packet(&mut self, system_id: u8, data: Vec<u8>) {
        self.systems.insert(system_id);
        self.packets.push(data);
    }

    fn is_ready(&self) -> bool {
        self.systems.len() >= self.threshold
    }

    fn release(self) -> (Vec<Vec<u8>>, Vec<Action>) {
        (self.packets, self.remaining_actions)
    }
}

/// Manager for batch operations
pub struct BatchManager {
    batches: Arc<RwLock<HashMap<String, BatchState>>>,
}

impl BatchManager {
    pub fn new() -> Self {
        Self {
            batches: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Queue a packet or release the batch if threshold is met
    #[allow(clippy::too_many_arguments)]
    pub async fn queue_or_release(
        &self,
        key: String,
        system_id: u8,
        packet: Vec<u8>,
        threshold: usize,
        timeout: Duration,
        forward_on_timeout: bool,
        remaining_actions: Vec<Action>,
        destination: Destination,
        state: Arc<crate::proxy::ProxyState>,
    ) -> BatchResult {
        let mut batches = self.batches.write().await;

        // Get or create batch state
        let batch = batches
            .entry(key.clone())
            .or_insert_with(|| {
                info!(
                    "Created new batch group '{}' (threshold={}, timeout={}s)",
                    key,
                    threshold,
                    timeout.as_secs()
                );

                // Spawn timeout handler
                let batches_clone = self.batches.clone();
                let key_clone = key.clone();
                let destination_clone = destination.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    sleep(timeout).await;
                    Self::handle_timeout(batches_clone, key_clone, destination_clone, state_clone).await;
                });

                BatchState::new(threshold, forward_on_timeout, remaining_actions.clone())
            });

        // Add packet to batch
        batch.add_packet(system_id, packet);

        let unique_count = batch.systems.len();
        let packet_count = batch.packets.len();

        debug!(
            "Batch '{}': added sysid={}, now {}/{} unique systems ({} total packets)",
            key, system_id, unique_count, threshold, packet_count
        );

        // Check if threshold is met
        if batch.is_ready() {
            let batch_state = batches.remove(&key).unwrap();
            let (packets, remaining_actions) = batch_state.release();
            info!(
                "Batch '{}' threshold met! Releasing {} packets from {} systems",
                key,
                packets.len(),
                unique_count
            );
            BatchResult::Release {
                packets,
                remaining_actions,
            }
        } else {
            BatchResult::Queued
        }
    }

    /// Handle batch timeout
    async fn handle_timeout(
        batches: Arc<RwLock<HashMap<String, BatchState>>>,
        key: String,
        destination: Destination,
        state: Arc<crate::proxy::ProxyState>,
    ) {
        let mut batches = batches.write().await;

        if let Some(batch) = batches.remove(&key) {
            let elapsed = batch.created_at.elapsed();
            let unique_count = batch.systems.len();
            let packet_count = batch.packets.len();

            if batch.forward_on_timeout {
                warn!(
                    "Batch '{}' timed out after {:?} with {}/{} systems ({} packets) - FORWARDING",
                    key, elapsed, unique_count, batch.threshold, packet_count
                );

                // Execute remaining actions on timed-out packets (including delay, etc.)
                let (packets, remaining_actions) = batch.release();
                info!(
                    "Forwarded {} timed-out packets, applying {} remaining action(s)",
                    packet_count,
                    remaining_actions.len()
                );

                // Continue the action chain for timed-out packets
                crate::proxy::execute_actions_impl(
                    remaining_actions,
                    packets,
                    destination,
                    state,
                )
                .await;
            } else {
                warn!(
                    "Batch '{}' timed out after {:?} with {}/{} systems ({} packets) - DROPPING",
                    key, elapsed, unique_count, batch.threshold, packet_count
                );
            }
        }
    }
}
