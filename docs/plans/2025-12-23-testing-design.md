# Ring Buffer & Flusher Testing Design

**Date:** 2025-12-23
**Branch:** feature/testing
**Status:** Approved

## Overview

Add comprehensive correctness tests for ring buffer and flusher, focusing on edge cases and defensive programming.

## Ring Buffer Tests (5 new tests)

### 1. SPSC Concurrent Access
Verify single-producer/single-consumer contract under concurrent access:
- Producer thread writes 10K messages
- Consumer thread reads simultaneously
- Verify no data corruption, all messages received in order

### 2. Position Wraparound
Test u64 position counter overflow:
- Set write_pos near u64::MAX
- Write enough to wrap around
- Verify reads still work correctly

### 3. Empty Buffer Operations
Edge cases on empty buffer:
- try_read on empty returns None
- peek on empty returns None
- Multiple empty reads don't corrupt state

### 4. Exactly Full Buffer
Boundary condition at capacity:
- Fill to exactly RING_SLOTS
- Verify is_full() == true, is_empty() == false
- Verify one read enables one write

### 5. Payload Size Boundaries
Message size edge cases:
- Empty payload (0 bytes)
- Max payload (SLOT_SIZE - header)
- One byte over max (should fail)

## Flusher Tests (5 new tests)

### 1. Empty Ring No Crash
Startup with empty ring:
- Start flusher with empty ring
- Immediately signal shutdown
- Verify no file created, no panic

### 2. Partial Batch on Shutdown
Less than BATCH_SIZE at shutdown:
- Write 10 messages (less than 64)
- Signal shutdown
- Verify all 10 written, not lost

### 3. Directory Creation
Missing directory handling:
- Point base_dir at non-existent path
- Verify rotate_file creates directories

### 4. Large Payload
Near slot-limit messages:
- Write message near max slot size
- Verify it flushes correctly with timestamp wrapper

### 5. Backpressure Handling
Full ring recovery:
- Fill ring buffer completely
- Start flusher, verify it drains
- Producer should be able to resume writing

## Integration Tests (4 new tests)

### 1. Full Pipeline
End-to-end: producer -> ring -> flusher -> disk:
- Create ring buffer, spawn flusher thread
- Producer writes 1000 messages
- Signal shutdown, join thread
- Verify all 1000 messages in output file

### 2. Concurrent Producer/Flusher
Simultaneous operation:
- Flusher running continuously
- Producer writing in bursts
- Verify no lost messages, correct ordering

### 3. Shutdown Under Load
Graceful termination:
- Producer writing continuously
- Signal shutdown mid-stream
- Verify clean termination, all queued messages flushed

### 4. Producer Resume After Drain
Recovery from full ring:
- Fill ring (producer returns false)
- Flusher drains some
- Producer resumes successfully

## File Structure

```
ssmd-rust/crates/connector/src/
├── ring_buffer.rs      # Add 5 tests to existing mod tests
├── flusher.rs          # Add 5 tests to existing mod tests
└── lib.rs              # Add integration_tests module (cfg test)
```

## Test Commands

```bash
# Run all connector tests
make rust-test

# Run specific test module
cd ssmd-rust && cargo test -p ssmd-connector-lib ring_buffer
cd ssmd-rust && cargo test -p ssmd-connector-lib flusher
cd ssmd-rust && cargo test -p ssmd-connector-lib integration
```
