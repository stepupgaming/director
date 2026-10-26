//! Shared-memory frame ring: one writer publishes painted frames into slots of
//! a memory mapping that readers in this process or another one copy out
//! without locks.
//!
//! Layout, little-endian. A 64-byte header: magic `DRFM`, version `u32`,
//! slot count `u32`, reserved `u32`, pixel capacity per slot `u64`, the newest
//! complete slot plus one `u64` (0 before the first frame), and the published
//! frame count `u64`. Each slot is a 64-byte header followed by `capacity`
//! RGBA bytes: sequence `u64` (odd while the writer is inside the slot),
//! generation `u64`, revision `u64`, time `f64` bits, width `u32`, height `u32`.
//! Rows are `width * 4` bytes with straight alpha.

use crate::error::{ErrorCode, HostError};
use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

const MAGIC: u32 = u32::from_le_bytes(*b"DRFM");
const VERSION: u32 = 1;
const HEADER: usize = 64;
const SLOT_HEADER: usize = 64;
/// Reader retries before it reports a torn read as no frame.
const READ_ATTEMPTS: usize = 64;

/// A frame copied out of the ring.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedFrame {
    pub slot: usize,
    pub generation: u64,
    pub revision: u64,
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Metadata of a frame to publish.
#[derive(Clone, Copy, Debug)]
pub struct FrameStamp {
    pub generation: u64,
    pub revision: u64,
    pub time: f64,
}

pub struct SharedFrames {
    _map: MmapMut,
    base: *mut u8,
    slots: usize,
    capacity: usize,
}

// The mapping lives as long as `base`; shared fields are atomics and pixel
// copies are guarded by each slot's sequence.
unsafe impl Send for SharedFrames {}
unsafe impl Sync for SharedFrames {}

impl SharedFrames {
    /// A ring of `slots` slots of `capacity` pixel bytes, backed by the file at
    /// `path` (created or truncated) or by anonymous memory.
    pub fn create(path: Option<&Path>, slots: usize, capacity: usize) -> Result<Self, HostError> {
        if slots < 2 || capacity == 0 {
            return Err(HostError::new(ErrorCode::InvalidArgument, "a frame ring needs two slots and a nonzero capacity"));
        }
        let length = HEADER + slots * (SLOT_HEADER + capacity.next_multiple_of(8));
        let io = |error: std::io::Error| HostError::new(ErrorCode::Render, format!("frame ring: {error}"));
        let mut map = match path {
            Some(path) => {
                let file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path).map_err(io)?;
                file.set_len(length as u64).map_err(io)?;
                unsafe { MmapOptions::new().map_mut(&file) }.map_err(io)?
            }
            None => MmapOptions::new().len(length).map_anon().map_err(io)?,
        };
        map[..HEADER].fill(0);
        map[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        map[4..8].copy_from_slice(&VERSION.to_le_bytes());
        map[8..12].copy_from_slice(&(slots as u32).to_le_bytes());
        map[16..24].copy_from_slice(&(capacity as u64).to_le_bytes());
        for slot in 0..slots {
            let at = HEADER + slot * (SLOT_HEADER + capacity.next_multiple_of(8));
            map[at..at + SLOT_HEADER].fill(0);
        }
        let base = map.as_mut_ptr();
        Ok(Self {
            _map: map,
            base,
            slots,
            capacity,
        })
    }

    /// Map an existing ring file for reading.
    pub fn open(path: &Path) -> Result<Self, HostError> {
        let io = |error: std::io::Error| HostError::new(ErrorCode::Load, format!("frame ring: {error}"));
        let file = OpenOptions::new().read(true).write(true).open(path).map_err(io)?;
        let mut map = unsafe { MmapOptions::new().map_mut(&file) }.map_err(io)?;
        let field = |at: usize| u32::from_le_bytes(map[at..at + 4].try_into().expect("u32"));
        if map.len() < HEADER || field(0) != MAGIC || field(4) != VERSION {
            return Err(HostError::new(ErrorCode::Load, format!("{} is not a version {VERSION} frame ring", path.display())));
        }
        let slots = field(8) as usize;
        let capacity = u64::from_le_bytes(map[16..24].try_into().expect("u64")) as usize;
        if map.len() < HEADER + slots * (SLOT_HEADER + capacity.next_multiple_of(8)) {
            return Err(HostError::new(ErrorCode::Load, format!("{} is shorter than its header says", path.display())));
        }
        let base = map.as_mut_ptr();
        Ok(Self {
            _map: map,
            base,
            slots,
            capacity,
        })
    }

    pub fn slots(&self) -> usize {
        self.slots
    }

    /// Pixel bytes per slot.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Frames published so far.
    pub fn published(&self) -> u64 {
        self.u64(24 + 8).load(Ordering::Acquire)
    }

    /// Write a frame into the slot after the newest one and make it the newest.
    /// One writer at a time.
    pub fn publish(&self, stamp: FrameStamp, width: u32, height: u32, rgba: &[u8]) -> Result<usize, HostError> {
        if rgba.len() != width as usize * height as usize * 4 || rgba.len() > self.capacity {
            return Err(HostError::new(
                ErrorCode::Render,
                format!("a {width}x{height} frame does not fit the {} byte frame slots", self.capacity),
            ));
        }
        let latest = self.u64(24).load(Ordering::Acquire) as usize;
        let slot = latest % self.slots;
        let at = self.slot_at(slot);
        let sequence = self.u64(at);
        let start = sequence.load(Ordering::Relaxed);
        sequence.store(start + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.u64(at + 8).store(stamp.generation, Ordering::Relaxed);
        self.u64(at + 16).store(stamp.revision, Ordering::Relaxed);
        self.u64(at + 24).store(stamp.time.to_bits(), Ordering::Relaxed);
        self.u32(at + 32).store(width, Ordering::Relaxed);
        self.u32(at + 36).store(height, Ordering::Relaxed);
        unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), self.base.add(at + SLOT_HEADER), rgba.len()) };
        sequence.store(start + 2, Ordering::Release);
        self.u64(24).store(slot as u64 + 1, Ordering::Release);
        self.u64(32).fetch_add(1, Ordering::AcqRel);
        Ok(slot)
    }

    /// Copy out the newest complete frame, or `None` before the first one.
    pub fn latest(&self) -> Option<SharedFrame> {
        (0..READ_ATTEMPTS).find_map(|_| {
            let latest = self.u64(24).load(Ordering::Acquire) as usize;
            let slot = latest.checked_sub(1)?;
            self.read(slot).ok()
        })
    }

    /// Copy one slot; `Err` on a torn read while the writer is inside it.
    fn read(&self, slot: usize) -> Result<SharedFrame, ()> {
        let at = self.slot_at(slot);
        let sequence = self.u64(at);
        let before = sequence.load(Ordering::Acquire);
        if before % 2 == 1 {
            return Err(());
        }
        let (width, height) = (self.u32(at + 32).load(Ordering::Relaxed), self.u32(at + 36).load(Ordering::Relaxed));
        let length = (width as usize * height as usize * 4).min(self.capacity);
        let mut frame = SharedFrame {
            slot,
            generation: self.u64(at + 8).load(Ordering::Relaxed),
            revision: self.u64(at + 16).load(Ordering::Relaxed),
            time: f64::from_bits(self.u64(at + 24).load(Ordering::Relaxed)),
            width,
            height,
            rgba: vec![0; length],
        };
        unsafe { std::ptr::copy_nonoverlapping(self.base.add(at + SLOT_HEADER), frame.rgba.as_mut_ptr(), length) };
        fence(Ordering::Acquire);
        if sequence.load(Ordering::Relaxed) == before {
            Ok(frame)
        } else {
            Err(())
        }
    }

    fn slot_at(&self, slot: usize) -> usize {
        HEADER + slot * (SLOT_HEADER + self.capacity.next_multiple_of(8))
    }

    fn u64(&self, at: usize) -> &AtomicU64 {
        unsafe { AtomicU64::from_ptr(self.base.add(at).cast()) }
    }

    fn u32(&self, at: usize) -> &AtomicU32 {
        unsafe { AtomicU32::from_ptr(self.base.add(at).cast()) }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameStamp, SharedFrames};

    fn stamp(generation: u64) -> FrameStamp {
        FrameStamp {
            generation,
            revision: 3,
            time: 0.5,
        }
    }

    #[test]
    fn frames_rotate_slots_and_read_back() {
        let frames = SharedFrames::create(None, 3, 16).unwrap();
        assert!(frames.latest().is_none());
        let pixels: Vec<u8> = (0..16).collect();
        assert_eq!(frames.publish(stamp(1), 2, 2, &pixels).unwrap(), 0);
        assert_eq!(frames.publish(stamp(2), 1, 2, &pixels[..8]).unwrap(), 1);
        let latest = frames.latest().unwrap();
        assert_eq!((latest.slot, latest.generation, latest.revision, latest.time), (1, 2, 3, 0.5));
        assert_eq!((latest.width, latest.height, latest.rgba.as_slice()), (1, 2, &pixels[..8]));
        frames.publish(stamp(3), 2, 2, &pixels).unwrap();
        assert_eq!(frames.publish(stamp(4), 2, 2, &pixels).unwrap(), 0);
        assert_eq!(frames.published(), 4);
        assert!(frames.publish(stamp(5), 3, 2, &[0; 24]).is_err());
    }

    #[test]
    fn a_second_mapping_of_the_file_reads_the_writer_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frames.ring");
        let writer = SharedFrames::create(Some(&path), 2, 4).unwrap();
        let reader = SharedFrames::open(&path).unwrap();
        writer.publish(stamp(9), 1, 1, &[1, 2, 3, 4]).unwrap();
        let frame = reader.latest().unwrap();
        assert_eq!((frame.generation, frame.rgba.as_slice()), (9, &[1, 2, 3, 4][..]));
        std::fs::write(dir.path().join("other"), [0u8; 128]).unwrap();
        assert!(SharedFrames::open(&dir.path().join("other")).is_err());
    }
}
