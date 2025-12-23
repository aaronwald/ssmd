# Latency Optimizations Design

**Date:** 2025-12-23
**Branch:** feature/latency-eval
**Status:** Approved

## Overview

Comprehensive latency optimization for the Rust market data pipeline, eliminating syscalls from the hot path and reducing per-message overhead from ~10-20μs to sub-microsecond.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Timestamps | TSC on hot path, wall-clock on disk | Syscalls OK at disk boundary |
| String interning | `lasso::ThreadedRodeo` | Lock-free reads, tickers repeat millions of times |
| File writer | SPSC mmap ring buffer | Zero-copy, no locks on hot path |
| Ring buffer model | Single-producer, single-consumer | Simplest, no atomics contention |

## Dependencies

```toml
# Cargo.toml additions
quanta = "0.12"           # TSC timestamps, zero syscalls
dashmap = "6"             # Lock-free concurrent HashMap
simd-json = "0.14"        # SIMD-accelerated JSON parsing
lasso = "0.7"             # String interning with ThreadedRodeo
arrayvec = "0.7"          # Stack-allocated vectors
memmap2 = "0.9"           # Memory-mapped files for ring buffer
bytemuck = "1.14"         # Safe transmutes for ring buffer headers
```

## Architecture

### Global Low-Latency Primitives

New module `middleware/src/latency.rs`:

```rust
use lasso::{ThreadedRodeo, Spur};
use once_cell::sync::Lazy;
use quanta::Clock;

pub static CLOCK: Lazy<Clock> = Lazy::new(Clock::new);
pub static INTERNER: Lazy<ThreadedRodeo> = Lazy::new(ThreadedRodeo::new);

#[inline]
pub fn now_tsc() -> u64 {
    CLOCK.raw()  // TSC ticks, zero syscalls
}

#[inline]
pub fn intern(s: &str) -> Spur {
    INTERNER.get_or_intern(s)
}
```

### Publisher Hot Path

Changes to `connector/src/publisher.rs`:

- `TradeData` uses `Spur` (interned) instead of `String` for ticker/trade_id
- Pre-allocated `Builder<HeapAllocator>` reused via `clear()`
- Pre-allocated output `Vec<u8>` reused via `clear()`
- Subject prefix pre-computed, only ticker appended per publish

```rust
pub struct TradeData {
    pub timestamp_nanos: u64,
    pub ticker: Spur,              // Interned, not String
    pub price: f64,
    pub size: u32,
    pub side: TradeSide,
    pub trade_id: Spur,            // Interned, not String
}

pub struct Publisher {
    transport: Arc<dyn Transport>,
    subject_prefix: String,        // Pre-computed: "{env}.{feed}.trade."
    capnp_builder: RefCell<Builder<HeapAllocator>>,
    output_buf: RefCell<Vec<u8>>,
}
```

### Transport & Sequences

Changes to `middleware/src/memory/transport.rs`:

- Replace `RwLock<HashMap>` with `DashMap` (lock-free reads)
- Replace `Mutex<u64>` with `AtomicU64` for sequence
- Use TSC for timestamps

```rust
pub struct InMemoryTransport {
    channels: DashMap<String, broadcast::Sender<TransportMessage>>,
    sequence: AtomicU64,
}

#[inline]
fn next_sequence(&self) -> u64 {
    self.sequence.fetch_add(1, Ordering::Relaxed)
}
```

Same pattern applies to `middleware/src/memory/journal.rs`.

### SPSC Ring Buffer

New file `connector/src/ring_writer.rs`:

```rust
const SLOT_SIZE: usize = 4096;        // 4KB per message slot
const RING_SLOTS: usize = 1024;       // 4MB total ring
const RING_SIZE: usize = SLOT_SIZE * RING_SLOTS;

#[repr(C)]
struct SlotHeader {
    len: u32,          // Payload length (0 = empty)
    flags: u32,        // Reserved
}

pub struct RingBuffer {
    mmap: MmapMut,
    write_pos: AtomicU64,   // Producer position
    read_pos: AtomicU64,    // Consumer position
}
```

Key operations:
- `try_write(&[u8]) -> bool`: Producer writes, returns false if full
- `try_read() -> Option<&[u8]>`: Consumer reads next slot

### Disk Flusher

New file `connector/src/flusher.rs`:

- Runs on dedicated `std::thread` (not tokio)
- Batches up to 64 messages per flush
- Wall-clock timestamp applied here (syscall amortized)
- 64KB BufWriter buffer

```rust
pub struct DiskFlusher {
    ring: Arc<RingBuffer>,
    base_dir: PathBuf,
    feed_name: String,
    current_writer: Option<BufWriter<File>>,
    current_date: String,
}
```

Shutdown handling:
1. Drain all remaining messages from ring
2. Final flush of BufWriter
3. `Drop` impl as safety net

### JSON Parsing

Changes to `connector/src/runner.rs`:

- Use `simd-json` instead of `serde_json`
- Pre-allocate parse buffer, reuse across messages
- Write to ring buffer instead of blocking writer

### Message Timestamps

Changes to `connector/src/message.rs`:

- Store TSC timestamp on creation (hot path)
- Wall-clock timestamp set lazily by disk flusher

## File Changes

**Modified files:**

| File | Changes |
|------|---------|
| `Cargo.toml` | Add dependencies |
| `middleware/src/lib.rs` | Export `latency` module |
| `middleware/src/memory/transport.rs` | DashMap + AtomicU64 |
| `middleware/src/memory/journal.rs` | AtomicU64 for sequence |
| `connector/src/publisher.rs` | Pre-alloc buffers, Spur strings |
| `connector/src/message.rs` | TSC timestamp, simd-json Value |
| `connector/src/runner.rs` | simd-json, ring buffer write |
| `connector/src/lib.rs` | Export new modules |

**New files:**

| File | Purpose |
|------|---------|
| `middleware/src/latency.rs` | CLOCK, INTERNER, `now_tsc()` |
| `connector/src/ring_writer.rs` | SPSC mmap ring buffer |
| `connector/src/flusher.rs` | Disk flush task |

## Expected Improvements

| Operation | Before | After |
|-----------|--------|-------|
| Timestamp | ~50ns (syscall) | ~10ns (TSC) |
| Sequence increment | ~1-10μs (mutex) | ~5ns (atomic) |
| Channel lookup | ~1-5μs (RwLock) | ~20ns (DashMap) |
| JSON parse | ~5μs (serde) | ~1μs (simd-json) |
| String alloc (ticker) | ~100ns | ~5ns (intern) |
| File write | ~1-10μs (mutex) | ~50ns (ring) |

**Total hot-path improvement:** ~10-20μs → <500ns per message
