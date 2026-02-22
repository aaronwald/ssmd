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
        self.try_read_with(|payload| payload.to_vec())
    }

    /// Consumer: read next message and process payload without allocation
    #[inline]
    pub fn try_read_with<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
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
        let payload = &self.mmap[payload_start..payload_end];
        let result = f(payload);

        // Release read position
        self.read_pos.store(read + 1, Ordering::Release);
        Some(result)
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

    #[test]
    fn test_spsc_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ring.buf");
        let ring = Arc::new(RingBuffer::new(&path).unwrap());

        let ring_producer = Arc::clone(&ring);
        let ring_consumer = Arc::clone(&ring);

        const NUM_MESSAGES: usize = 10_000;

        // Producer thread
        let producer = thread::spawn(move || {
            for i in 0..NUM_MESSAGES {
                let msg = format!("msg{:05}", i);
                while !ring_producer.try_write(msg.as_bytes()) {
                    thread::yield_now(); // Backpressure - wait for consumer
                }
            }
        });

        // Consumer thread
        let consumer = thread::spawn(move || {
            let mut received = Vec::with_capacity(NUM_MESSAGES);
            while received.len() < NUM_MESSAGES {
                if let Some(data) = ring_consumer.try_read() {
                    received.push(String::from_utf8(data).unwrap());
                } else {
                    thread::yield_now();
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();

        // Verify all messages received in order
        assert_eq!(received.len(), NUM_MESSAGES);
        for (i, msg) in received.iter().enumerate() {
            assert_eq!(msg, &format!("msg{:05}", i), "Message {} out of order", i);
        }
    }

    #[test]
    fn test_position_wraparound() {
        let (ring, _tmp) = create_test_ring();

        // Simulate near-wraparound by writing and reading many messages
        // This tests that position arithmetic works correctly
        for round in 0..3 {
            for i in 0..RING_SLOTS {
                let msg = format!("round{}msg{}", round, i);
                assert!(ring.try_write(msg.as_bytes()), "Write failed at round {} msg {}", round, i);
            }
            for i in 0..RING_SLOTS {
                let data = ring.try_read().expect("Read failed");
                let expected = format!("round{}msg{}", round, i);
                assert_eq!(data, expected.as_bytes());
            }
        }

        // Buffer should be empty after all reads
        assert!(ring.is_empty());
    }

    #[test]
    fn test_empty_buffer_operations() {
        let (ring, _tmp) = create_test_ring();

        // Verify empty state
        assert!(ring.is_empty());
        assert!(!ring.is_full());

        // Multiple reads on empty should return None and not corrupt state
        for _ in 0..100 {
            assert!(ring.try_read().is_none());
            assert!(ring.peek().is_none());
        }

        // Should still work after empty reads
        assert!(ring.try_write(b"test"));
        assert_eq!(ring.try_read().unwrap(), b"test");
        assert!(ring.is_empty());
    }

    #[test]
    fn test_exactly_full_buffer() {
        let (ring, _tmp) = create_test_ring();

        // Fill exactly to capacity
        for i in 0..RING_SLOTS {
            assert!(ring.try_write(format!("msg{}", i).as_bytes()));
        }

        // Verify full state
        assert!(ring.is_full());
        assert!(!ring.is_empty());

        // One more write should fail
        assert!(!ring.try_write(b"overflow"));

        // Read one
        let first = ring.try_read().unwrap();
        assert_eq!(first, b"msg0");

        // Now not full, but not empty
        assert!(!ring.is_full());
        assert!(!ring.is_empty());

        // Can write one more now
        assert!(ring.try_write(b"new"));

        // Full again
        assert!(ring.is_full());
    }

    #[test]
    fn test_payload_size_boundaries() {
        let (ring, _tmp) = create_test_ring();
        let header_size = std::mem::size_of::<SlotHeader>();
        let max_payload = SLOT_SIZE - header_size;

        // Empty payload (0 bytes)
        assert!(ring.try_write(b""));
        let empty = ring.try_read().unwrap();
        assert!(empty.is_empty());

        // Exactly max payload
        let max_msg = vec![b'x'; max_payload];
        assert!(ring.try_write(&max_msg));
        let read_max = ring.try_read().unwrap();
        assert_eq!(read_max.len(), max_payload);

        // One byte over max should fail
        let too_big = vec![b'y'; max_payload + 1];
        assert!(!ring.try_write(&too_big));

        // Buffer should still work after rejected write
        assert!(ring.try_write(b"still works"));
        assert_eq!(ring.try_read().unwrap(), b"still works");
    }
}
