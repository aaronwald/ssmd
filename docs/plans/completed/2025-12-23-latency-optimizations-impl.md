# Latency Optimizations Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate syscalls from hot path, reduce per-message latency from ~10-20μs to <500ns.

**Architecture:** TSC timestamps via quanta on hot path, wall-clock only at disk boundary. DashMap for lock-free channel lookups, AtomicU64 for sequences. SPSC mmap ring buffer decouples hot path from disk I/O. String interning via lasso for repeated tickers.

**Tech Stack:** quanta, dashmap, simd-json, lasso, arrayvec, memmap2, bytemuck

**Design Doc:** `docs/plans/2025-12-23-latency-optimizations-design.md`

---

## Task 1: Add Dependencies to Workspace

**Files:**
- Modify: `ssmd-rust/Cargo.toml`
- Modify: `ssmd-rust/crates/middleware/Cargo.toml`
- Modify: `ssmd-rust/crates/connector/Cargo.toml`

**Step 1: Add workspace dependencies**

Edit `ssmd-rust/Cargo.toml`, add to `[workspace.dependencies]`:

```toml
quanta = "0.12"
dashmap = "6"
simd-json = "0.14"
lasso = { version = "0.7", features = ["multi-threaded"] }
arrayvec = "0.7"
memmap2 = "0.9"
bytemuck = { version = "1.14", features = ["derive"] }
once_cell = "1.19"
```

**Step 2: Add middleware crate dependencies**

Edit `ssmd-rust/crates/middleware/Cargo.toml`, add to `[dependencies]`:

```toml
quanta = { workspace = true }
dashmap = { workspace = true }
lasso = { workspace = true }
once_cell = { workspace = true }
```

**Step 3: Add connector crate dependencies**

Edit `ssmd-rust/crates/connector/Cargo.toml`, add to `[dependencies]`:

```toml
simd-json = { workspace = true }
lasso = { workspace = true }
arrayvec = { workspace = true }
memmap2 = { workspace = true }
bytemuck = { workspace = true }
```

**Step 4: Verify dependencies resolve**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo check`
Expected: Compiles with no errors (warnings OK)

**Step 5: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/Cargo.toml ssmd-rust/crates/middleware/Cargo.toml ssmd-rust/crates/connector/Cargo.toml
git commit -m "chore: add latency optimization dependencies"
```

---

## Task 2: Create Latency Module with TSC Clock and String Interner

**Files:**
- Create: `ssmd-rust/crates/middleware/src/latency.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the test**

Create `ssmd-rust/crates/middleware/src/latency.rs`:

```rust
//! Low-latency primitives: TSC clock and string interning
//!
//! These avoid syscalls on the hot path.

use lasso::{Spur, ThreadedRodeo};
use once_cell::sync::Lazy;
use quanta::Clock;

/// Global TSC clock - zero syscall timestamp reads
pub static CLOCK: Lazy<Clock> = Lazy::new(Clock::new);

/// Global string interner - lock-free reads after interning
pub static INTERNER: Lazy<ThreadedRodeo> = Lazy::new(ThreadedRodeo::new);

/// Get current TSC timestamp (zero syscalls)
#[inline]
pub fn now_tsc() -> u64 {
    CLOCK.raw()
}

/// Intern a string, returning a Spur handle
#[inline]
pub fn intern(s: &str) -> Spur {
    INTERNER.get_or_intern(s)
}

/// Resolve a Spur back to &str
#[inline]
pub fn resolve(spur: Spur) -> &'static str {
    // SAFETY: INTERNER is static, so resolved strings live forever
    INTERNER.resolve(&spur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_tsc_returns_increasing_values() {
        let t1 = now_tsc();
        let t2 = now_tsc();
        assert!(t2 >= t1, "TSC should be monotonic");
    }

    #[test]
    fn test_intern_and_resolve() {
        let spur = intern("BTCUSD");
        let resolved = resolve(spur);
        assert_eq!(resolved, "BTCUSD");
    }

    #[test]
    fn test_intern_same_string_returns_same_spur() {
        let spur1 = intern("ETHUSD");
        let spur2 = intern("ETHUSD");
        assert_eq!(spur1, spur2);
    }

    #[test]
    fn test_intern_different_strings_return_different_spurs() {
        let spur1 = intern("AAPL");
        let spur2 = intern("GOOGL");
        assert_ne!(spur1, spur2);
    }
}
```

**Step 2: Export module from lib.rs**

Edit `ssmd-rust/crates/middleware/src/lib.rs`, add after line 12:

```rust
pub mod latency;
```

And add to exports:

```rust
pub use latency::{intern, now_tsc, resolve, CLOCK, INTERNER};
```

**Step 3: Run tests to verify they pass**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware latency`
Expected: 4 tests pass

**Step 4: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/crates/middleware/src/latency.rs ssmd-rust/crates/middleware/src/lib.rs
git commit -m "feat(middleware): add latency module with TSC clock and string interner"
```

---

## Task 3: Update InMemoryTransport with DashMap and AtomicU64

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/memory/transport.rs`

**Step 1: Update imports and struct**

Replace the entire file content with:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::error::TransportError;
use crate::latency::now_tsc;
use crate::transport::{Subscription, Transport, TransportMessage};

const CHANNEL_BUFFER_SIZE: usize = 1024;

pub struct InMemoryTransport {
    channels: DashMap<String, broadcast::Sender<TransportMessage>>,
    sequence: AtomicU64,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            sequence: AtomicU64::new(0),
        }
    }

    #[inline]
    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn get_or_create_channel(&self, subject: &str) -> broadcast::Sender<TransportMessage> {
        self.channels
            .entry(subject.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_BUFFER_SIZE).0)
            .clone()
    }
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

struct InMemorySubscription {
    rx: broadcast::Receiver<TransportMessage>,
}

#[async_trait]
impl Subscription for InMemorySubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        self.rx
            .recv()
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }

    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> {
        Ok(())
    }

    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError> {
        Ok(())
    }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.publish_with_headers(subject, payload, HashMap::new())
            .await
    }

    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError> {
        let tx = self.get_or_create_channel(subject);
        let seq = self.next_sequence();
        let msg = TransportMessage {
            subject: subject.to_string(),
            payload,
            headers,
            timestamp: now_tsc(),
            sequence: Some(seq),
        };
        let _ = tx.send(msg);
        Ok(())
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let tx = self.get_or_create_channel(subject);
        let rx = tx.subscribe();
        Ok(Box::new(InMemorySubscription { rx }))
    }

    async fn request(
        &self,
        _subject: &str,
        _payload: Bytes,
        _timeout: Duration,
    ) -> Result<TransportMessage, TransportError> {
        Err(TransportError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.subject").await.unwrap();
        transport
            .publish("test.subject", Bytes::from("hello"))
            .await
            .unwrap();
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "test.subject");
        assert_eq!(msg.payload, Bytes::from("hello"));
    }

    #[tokio::test]
    async fn test_sequence_numbers_increment() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.seq").await.unwrap();
        transport
            .publish("test.seq", Bytes::from("1"))
            .await
            .unwrap();
        transport
            .publish("test.seq", Bytes::from("2"))
            .await
            .unwrap();
        let msg1 = sub.next().await.unwrap();
        let msg2 = sub.next().await.unwrap();
        assert_eq!(msg1.sequence, Some(0));
        assert_eq!(msg2.sequence, Some(1));
    }

    #[tokio::test]
    async fn test_timestamp_is_tsc() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.ts").await.unwrap();

        let before = now_tsc();
        transport.publish("test.ts", Bytes::from("x")).await.unwrap();
        let after = now_tsc();

        let msg = sub.next().await.unwrap();
        assert!(msg.timestamp >= before && msg.timestamp <= after);
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware memory::transport`
Expected: 3 tests pass

**Step 3: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/crates/middleware/src/memory/transport.rs
git commit -m "perf(transport): use DashMap and AtomicU64 for lock-free hot path"
```

---

## Task 4: Update InMemoryJournal with AtomicU64

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/memory/journal.rs`

**Step 1: Update imports and replace mutex with atomic**

Replace the entire file content with:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::RwLock;

use crate::error::JournalError;
use crate::journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};
use crate::latency::now_tsc;

pub struct InMemoryJournal {
    topics: Arc<RwLock<HashMap<String, Vec<JournalEntry>>>>,
    sequence: AtomicU64,
}

impl InMemoryJournal {
    pub fn new() -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            sequence: AtomicU64::new(0),
        }
    }

    #[inline]
    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn now_millis() -> u64 {
        now_tsc()
    }
}

impl Default for InMemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}

struct InMemoryJournalReader {
    entries: Vec<JournalEntry>,
    position: usize,
}

#[async_trait]
impl JournalReader for InMemoryJournalReader {
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError> {
        if self.position >= self.entries.len() {
            Ok(None)
        } else {
            let entry = self.entries[self.position].clone();
            self.position += 1;
            Ok(Some(entry))
        }
    }

    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError> {
        match position {
            JournalPosition::Beginning => self.position = 0,
            JournalPosition::End => self.position = self.entries.len(),
            JournalPosition::Sequence(seq) => {
                self.position = self
                    .entries
                    .iter()
                    .position(|e| e.sequence >= seq)
                    .unwrap_or(self.entries.len())
            }
            JournalPosition::Time(ts) => {
                self.position = self
                    .entries
                    .iter()
                    .position(|e| e.timestamp >= ts)
                    .unwrap_or(self.entries.len())
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Journal for InMemoryJournal {
    async fn append(&self, topic: &str, key: Option<Bytes>, payload: Bytes) -> Result<u64, JournalError> {
        self.append_with_headers(topic, key, payload, HashMap::new())
            .await
    }

    async fn append_with_headers(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<u64, JournalError> {
        let seq = self.next_sequence();
        let entry = JournalEntry {
            sequence: seq,
            timestamp: Self::now_millis(),
            topic: topic.to_string(),
            key,
            payload,
            headers,
        };
        let mut topics = self.topics.write().await;
        topics.entry(topic.to_string()).or_default().push(entry);
        Ok(seq)
    }

    async fn reader(
        &self,
        topic: &str,
        position: JournalPosition,
    ) -> Result<Box<dyn JournalReader>, JournalError> {
        let topics = self.topics.read().await;
        let entries = topics.get(topic).cloned().unwrap_or_default();
        let mut reader = InMemoryJournalReader {
            entries,
            position: 0,
        };
        reader.seek(position).await?;
        Ok(Box::new(reader))
    }

    async fn end_position(&self, topic: &str) -> Result<u64, JournalError> {
        let topics = self.topics.read().await;
        Ok(topics
            .get(topic)
            .and_then(|entries| entries.last().map(|e| e.sequence))
            .unwrap_or(0))
    }

    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError> {
        let mut topics = self.topics.write().await;
        topics.entry(config.name).or_default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_read() {
        let journal = InMemoryJournal::new();
        journal
            .append("topic", None, Bytes::from("msg1"))
            .await
            .unwrap();
        journal
            .append("topic", None, Bytes::from("msg2"))
            .await
            .unwrap();
        let mut reader = journal
            .reader("topic", JournalPosition::Beginning)
            .await
            .unwrap();
        let e1 = reader.next().await.unwrap().unwrap();
        let e2 = reader.next().await.unwrap().unwrap();
        assert_eq!(e1.sequence, 0);
        assert_eq!(e2.sequence, 1);
        assert!(reader.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_end_position() {
        let journal = InMemoryJournal::new();
        assert_eq!(journal.end_position("topic").await.unwrap(), 0);
        journal
            .append("topic", None, Bytes::from("x"))
            .await
            .unwrap();
        assert_eq!(journal.end_position("topic").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_sequence_is_atomic() {
        let journal = InMemoryJournal::new();
        let seq1 = journal.append("t1", None, Bytes::from("a")).await.unwrap();
        let seq2 = journal.append("t2", None, Bytes::from("b")).await.unwrap();
        assert_eq!(seq1, 0);
        assert_eq!(seq2, 1);
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware memory::journal`
Expected: 3 tests pass

**Step 3: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/crates/middleware/src/memory/journal.rs
git commit -m "perf(journal): use AtomicU64 for lock-free sequence generation"
```

---

## Task 5: Create Ring Buffer Module

**Files:**
- Create: `ssmd-rust/crates/connector/src/ring_buffer.rs`
- Modify: `ssmd-rust/crates/connector/src/lib.rs`

**Step 1: Create ring buffer implementation**

Create `ssmd-rust/crates/connector/src/ring_buffer.rs`:

```rust
//! SPSC memory-mapped ring buffer for zero-copy message passing
//!
//! Single producer writes messages, single consumer reads and flushes to disk.
//! No locks on hot path - just atomic positions.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use memmap2::MmapMut;

/// Size of each message slot (4KB)
pub const SLOT_SIZE: usize = 4096;

/// Number of slots in the ring (1024 = 4MB total)
pub const RING_SLOTS: usize = 1024;

/// Total ring buffer size
pub const RING_SIZE: usize = SLOT_SIZE * RING_SLOTS;

/// Header at the start of each slot
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct SlotHeader {
    /// Payload length in bytes (0 = empty slot)
    pub len: u32,
    /// Reserved for future use (flags, checksums, etc.)
    pub flags: u32,
}

/// SPSC ring buffer backed by memory-mapped file
pub struct RingBuffer {
    #[allow(dead_code)]
    file: File,
    mmap: MmapMut,
    write_pos: AtomicU64,
    read_pos: AtomicU64,
}

impl RingBuffer {
    /// Create a new ring buffer backed by the given file path
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        file.set_len(RING_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        Ok(Self {
            file,
            mmap,
            write_pos: AtomicU64::new(0),
            read_pos: AtomicU64::new(0),
        })
    }

    /// Get current write position (for testing/debugging)
    pub fn write_position(&self) -> u64 {
        self.write_pos.load(Ordering::Acquire)
    }

    /// Get current read position (for testing/debugging)
    pub fn read_position(&self) -> u64 {
        self.read_pos.load(Ordering::Acquire)
    }

    /// Check if ring buffer is full
    pub fn is_full(&self) -> bool {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read) >= RING_SLOTS as u64
    }

    /// Check if ring buffer is empty
    pub fn is_empty(&self) -> bool {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        read >= write
    }

    /// Producer: write message to ring buffer
    /// Returns false if ring is full (backpressure)
    #[inline]
    pub fn try_write(&self, data: &[u8]) -> bool {
        let max_payload = SLOT_SIZE - std::mem::size_of::<SlotHeader>();
        if data.len() > max_payload {
            return false; // Message too large
        }

        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);

        // Check if full (writer caught up to reader)
        if write.wrapping_sub(read) >= RING_SLOTS as u64 {
            return false;
        }

        let slot_idx = (write as usize) % RING_SLOTS;
        let offset = slot_idx * SLOT_SIZE;

        // Write header
        let header = SlotHeader {
            len: data.len() as u32,
            flags: 0,
        };
        let header_bytes = bytemuck::bytes_of(&header);

        // SAFETY: We have exclusive write access to this slot
        let slot = &self.mmap[offset..offset + SLOT_SIZE];
        unsafe {
            let slot_ptr = slot.as_ptr() as *mut u8;
            std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), slot_ptr, header_bytes.len());
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                slot_ptr.add(header_bytes.len()),
                data.len(),
            );
        }

        // Release write position
        self.write_pos.store(write + 1, Ordering::Release);
        true
    }

    /// Consumer: read next message from ring buffer
    /// Returns None if ring is empty
    #[inline]
    pub fn try_read(&self) -> Option<Vec<u8>> {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);

        if read >= write {
            return None; // Empty
        }

        let slot_idx = (read as usize) % RING_SLOTS;
        let offset = slot_idx * SLOT_SIZE;

        // Read header
        let header_size = std::mem::size_of::<SlotHeader>();
        let header: SlotHeader = bytemuck::pod_read_unaligned(&self.mmap[offset..offset + header_size]);

        // Read payload
        let payload_start = offset + header_size;
        let payload_end = payload_start + header.len as usize;
        let payload = self.mmap[payload_start..payload_end].to_vec();

        // Release read position
        self.read_pos.store(read + 1, Ordering::Release);
        Some(payload)
    }

    /// Consumer: peek at next message without advancing position
    pub fn peek(&self) -> Option<Vec<u8>> {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);

        if read >= write {
            return None;
        }

        let slot_idx = (read as usize) % RING_SLOTS;
        let offset = slot_idx * SLOT_SIZE;

        let header_size = std::mem::size_of::<SlotHeader>();
        let header: SlotHeader = bytemuck::pod_read_unaligned(&self.mmap[offset..offset + header_size]);

        let payload_start = offset + header_size;
        let payload_end = payload_start + header.len as usize;
        Some(self.mmap[payload_start..payload_end].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_ring() -> (RingBuffer, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ring.buf");
        let ring = RingBuffer::new(&path).unwrap();
        (ring, tmp)
    }

    #[test]
    fn test_write_and_read_single_message() {
        let (ring, _tmp) = create_test_ring();

        assert!(ring.is_empty());
        assert!(ring.try_write(b"hello world"));
        assert!(!ring.is_empty());

        let msg = ring.try_read().unwrap();
        assert_eq!(msg, b"hello world");
        assert!(ring.is_empty());
    }

    #[test]
    fn test_write_and_read_multiple_messages() {
        let (ring, _tmp) = create_test_ring();

        ring.try_write(b"msg1");
        ring.try_write(b"msg2");
        ring.try_write(b"msg3");

        assert_eq!(ring.try_read().unwrap(), b"msg1");
        assert_eq!(ring.try_read().unwrap(), b"msg2");
        assert_eq!(ring.try_read().unwrap(), b"msg3");
        assert!(ring.try_read().is_none());
    }

    #[test]
    fn test_ring_full_returns_false() {
        let (ring, _tmp) = create_test_ring();

        // Fill the ring
        for i in 0..RING_SLOTS {
            assert!(ring.try_write(format!("msg{}", i).as_bytes()));
        }

        assert!(ring.is_full());
        assert!(!ring.try_write(b"overflow"));

        // Read one, then we can write one
        ring.try_read();
        assert!(ring.try_write(b"new msg"));
    }

    #[test]
    fn test_peek_does_not_advance() {
        let (ring, _tmp) = create_test_ring();

        ring.try_write(b"peek test");

        assert_eq!(ring.peek().unwrap(), b"peek test");
        assert_eq!(ring.peek().unwrap(), b"peek test");
        assert_eq!(ring.try_read().unwrap(), b"peek test");
        assert!(ring.peek().is_none());
    }

    #[test]
    fn test_message_too_large() {
        let (ring, _tmp) = create_test_ring();

        let large_msg = vec![0u8; SLOT_SIZE]; // Larger than max payload
        assert!(!ring.try_write(&large_msg));
    }

    #[test]
    fn test_positions_track_correctly() {
        let (ring, _tmp) = create_test_ring();

        assert_eq!(ring.write_position(), 0);
        assert_eq!(ring.read_position(), 0);

        ring.try_write(b"a");
        assert_eq!(ring.write_position(), 1);
        assert_eq!(ring.read_position(), 0);

        ring.try_read();
        assert_eq!(ring.write_position(), 1);
        assert_eq!(ring.read_position(), 1);
    }
}
```

**Step 2: Export module from lib.rs**

Edit `ssmd-rust/crates/connector/src/lib.rs`, add after line 14:

```rust
pub mod ring_buffer;
```

And add to exports after line 24:

```rust
pub use ring_buffer::{RingBuffer, RING_SIZE, RING_SLOTS, SLOT_SIZE};
```

**Step 3: Run tests to verify they pass**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-connector-lib ring_buffer`
Expected: 6 tests pass

**Step 4: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/crates/connector/src/ring_buffer.rs ssmd-rust/crates/connector/src/lib.rs
git commit -m "feat(connector): add SPSC memory-mapped ring buffer"
```

---

## Task 6: Create Disk Flusher Module

**Files:**
- Create: `ssmd-rust/crates/connector/src/flusher.rs`
- Modify: `ssmd-rust/crates/connector/src/lib.rs`

**Step 1: Create flusher implementation**

Create `ssmd-rust/crates/connector/src/flusher.rs`:

```rust
//! Disk flusher that drains ring buffer and writes to date-partitioned files
//!
//! Runs on dedicated std::thread to avoid tokio runtime overhead.
//! Wall-clock timestamps applied here (syscall OK - we're doing disk I/O anyway).

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::ring_buffer::RingBuffer;

/// Batch size: drain up to this many messages before yielding
const BATCH_SIZE: usize = 64;

/// Sleep duration when ring is empty (100μs)
const EMPTY_SLEEP_MICROS: u64 = 100;

/// BufWriter capacity (64KB)
const WRITE_BUFFER_SIZE: usize = 65536;

/// Disk flusher that consumes from ring buffer and writes to JSONL files
pub struct DiskFlusher {
    ring: Arc<RingBuffer>,
    base_dir: PathBuf,
    feed_name: String,
    current_writer: Option<BufWriter<File>>,
    current_date: String,
}

impl DiskFlusher {
    /// Create a new disk flusher
    pub fn new(ring: Arc<RingBuffer>, base_dir: PathBuf, feed_name: String) -> Self {
        Self {
            ring,
            base_dir,
            feed_name,
            current_writer: None,
            current_date: String::new(),
        }
    }

    /// Run the flusher loop until shutdown signal
    /// Call this from a dedicated std::thread
    pub fn run(&mut self, shutdown: Arc<AtomicBool>) {
        while !shutdown.load(Ordering::Relaxed) {
            let count = self.drain_batch();

            if count > 0 {
                self.flush();
            } else {
                // Ring empty, sleep briefly to avoid busy-spin
                std::thread::sleep(std::time::Duration::from_micros(EMPTY_SLEEP_MICROS));
            }
        }

        // Shutdown: drain all remaining messages
        self.drain_all();
        self.flush();
    }

    /// Drain up to BATCH_SIZE messages from ring
    fn drain_batch(&mut self) -> usize {
        let mut count = 0;
        while count < BATCH_SIZE {
            if let Some(payload) = self.ring.try_read() {
                self.write_message(&payload);
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Drain all remaining messages from ring
    fn drain_all(&mut self) {
        while let Some(payload) = self.ring.try_read() {
            self.write_message(&payload);
        }
    }

    /// Write a single message to the current file
    fn write_message(&mut self, payload: &[u8]) {
        // Wall-clock timestamp - syscall OK here, we're about to do disk I/O
        let now = chrono::Utc::now();
        let date = now.format("%Y-%m-%d").to_string();

        // Rotate file if date changed
        if date != self.current_date {
            self.rotate_file(&date);
        }

        if let Some(ref mut writer) = self.current_writer {
            // Write timestamp and payload as JSONL
            let ts = now.to_rfc3339();
            let _ = write!(writer, "{{\"ts\":\"{}\",\"data\":", ts);
            let _ = writer.write_all(payload);
            let _ = writeln!(writer, "}}");
        }
    }

    /// Flush current writer
    fn flush(&mut self) {
        if let Some(ref mut writer) = self.current_writer {
            let _ = writer.flush();
        }
    }

    /// Rotate to a new file for the given date
    fn rotate_file(&mut self, date: &str) {
        // Flush and close current writer
        self.flush();

        // Create date directory
        let dir = self.base_dir.join(date);
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::error!(error = %e, "Failed to create directory");
            return;
        }

        // Open new file
        let path = dir.join(format!("{}.jsonl", self.feed_name));
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                self.current_writer = Some(BufWriter::with_capacity(WRITE_BUFFER_SIZE, file));
                self.current_date = date.to_string();
            }
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "Failed to open file");
            }
        }
    }
}

impl Drop for DiskFlusher {
    fn drop(&mut self) {
        // Safety net: flush on drop
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_setup() -> (Arc<RingBuffer>, DiskFlusher, TempDir) {
        let tmp = TempDir::new().unwrap();
        let ring_path = tmp.path().join("ring.buf");
        let ring = Arc::new(RingBuffer::new(&ring_path).unwrap());
        let flusher = DiskFlusher::new(ring.clone(), tmp.path().to_path_buf(), "test-feed".to_string());
        (ring, flusher, tmp)
    }

    #[test]
    fn test_flusher_writes_to_file() {
        let (ring, mut flusher, tmp) = create_test_setup();

        // Write some messages
        ring.try_write(b"{\"price\":100}");
        ring.try_write(b"{\"price\":101}");

        // Drain and flush
        flusher.drain_batch();
        flusher.flush();

        // Find the output file
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("test-feed.jsonl");

        assert!(output_path.exists(), "Output file should exist");

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"price\":100"), "Should contain first message");
        assert!(content.contains("\"price\":101"), "Should contain second message");
        assert!(content.contains("\"ts\":"), "Should have timestamps");
    }

    #[test]
    fn test_flusher_drains_on_shutdown() {
        let (ring, mut flusher, tmp) = create_test_setup();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Write messages
        for i in 0..10 {
            ring.try_write(format!("{{\"n\":{}}}", i).as_bytes());
        }

        // Signal shutdown immediately
        shutdown.store(true, Ordering::Relaxed);

        // Run flusher - should drain all and exit
        flusher.run(shutdown);

        // Verify all messages written
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("test-feed.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();
        let lines: Vec<_> = content.lines().collect();

        assert_eq!(lines.len(), 10, "All 10 messages should be written");
    }

    #[test]
    fn test_flusher_batches_writes() {
        let (ring, mut flusher, _tmp) = create_test_setup();

        // Write more than batch size
        for i in 0..100 {
            ring.try_write(format!("{{\"i\":{}}}", i).as_bytes());
        }

        // First batch should drain 64
        let count = flusher.drain_batch();
        assert_eq!(count, BATCH_SIZE);

        // Second batch should drain remaining 36
        let count = flusher.drain_batch();
        assert_eq!(count, 36);

        // Third batch should be empty
        let count = flusher.drain_batch();
        assert_eq!(count, 0);
    }
}
```

**Step 2: Export module from lib.rs**

Edit `ssmd-rust/crates/connector/src/lib.rs`, add after ring_buffer module:

```rust
pub mod flusher;
```

And add to exports:

```rust
pub use flusher::DiskFlusher;
```

**Step 3: Run tests to verify they pass**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-connector-lib flusher`
Expected: 3 tests pass

**Step 4: Commit**

```bash
cd /workspaces/ssmd
git add ssmd-rust/crates/connector/src/flusher.rs ssmd-rust/crates/connector/src/lib.rs
git commit -m "feat(connector): add disk flusher with batching and shutdown drain"
```

---

## Task 7: Run Full Test Suite

**Step 1: Run all middleware tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: All tests pass

**Step 2: Run all connector tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-connector-lib`
Expected: All tests pass

**Step 3: Run full Rust test suite**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All tests pass

**Step 4: Run clippy**

Run: `cd /workspaces/ssmd && make rust-clippy`
Expected: No errors (warnings OK)

**Step 5: Commit any fixes if needed**

If any tests fail or clippy reports errors, fix them and commit.

---

## Task 8: Final Validation

**Step 1: Run full validation**

Run: `cd /workspaces/ssmd && make all`
Expected: All lint, test, and build steps pass

**Step 2: Commit and push**

```bash
cd /workspaces/ssmd
git push -u origin feature/latency-eval
```

---

## Summary

After completing all tasks, the following optimizations are in place:

| Component | Before | After |
|-----------|--------|-------|
| Timestamps | syscall per message | TSC (zero syscall) |
| Transport channels | RwLock<HashMap> | DashMap (lock-free reads) |
| Sequences | Mutex<u64> | AtomicU64 |
| File writes | mutex + sync I/O | SPSC ring + async flush |

Future tasks (not in this plan):
- Update Publisher with pre-allocated buffers and Spur strings
- Update Runner with simd-json parsing
- Integration with existing Writer trait
