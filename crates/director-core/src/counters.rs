//! Process-wide work counters: how often the engine spawned a process,
//! decoded media, imported a document, shaped text or read pixels back.
//! Reports print them after a render, export or QA pass so repeated,
//! discarded and oversized work shows without a profiler.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! counters {
    ($($(#[doc = $doc:literal])+ $name:ident => $label:literal,)+) => {
        /// One kind of counted work.
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Counter { $($(#[doc = $doc])+ $name,)+ }

        const COUNT: usize = [$($label),+].len();
        const LABELS: [&str; COUNT] = [$($label),+];
        static VALUES: [AtomicU64; COUNT] = [const { AtomicU64::new(0) }; COUNT];

        /// Counter values at one moment. Subtract two snapshots for the work between them.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
        pub struct WorkCounts {
            $($(#[doc = $doc])+ pub $name: u64,)+
            /// Largest resident memory of the process so far in bytes, 0 when unknown.
            pub peak_rss_bytes: u64,
        }

        impl WorkCounts {
            fn values(&self) -> [u64; COUNT] { [$(self.$name),+] }
        }

        /// The counters now.
        pub fn snapshot() -> WorkCounts {
            WorkCounts {
                $($name: VALUES[Counter::$name as usize].load(Ordering::Relaxed),)+
                peak_rss_bytes: peak_rss_bytes(),
            }
        }

        impl std::ops::Sub for WorkCounts {
            type Output = WorkCounts;
            /// Work since `earlier`. The memory peak stays that of the process.
            fn sub(self, earlier: WorkCounts) -> WorkCounts {
                WorkCounts {
                    $($name: self.$name.saturating_sub(earlier.$name),)+
                    peak_rss_bytes: self.peak_rss_bytes,
                }
            }
        }
    };
}

counters! {
    /// ffprobe processes spawned.
    ffprobe_spawns => "ffprobe spawns",
    /// FFmpeg processes spawned, decoders and encoders together.
    ffmpeg_spawns => "ffmpeg spawns",
    /// FFmpeg video decode sessions started.
    decode_sessions => "video decode sessions",
    /// Video frames read from a decoder.
    frames_decoded => "video frames decoded",
    /// Video frames read from a decoder that nobody asked for or kept.
    frames_discarded => "video frames discarded",
    /// Bytes of raw video read from decoders.
    decoded_bytes => "video bytes decoded",
    /// Video frames copied into an image for painting.
    frames_presented => "video frames presented",
    /// Scene documents imported into a `Director`.
    document_imports => "document imports",
    /// Audio sources decoded to samples.
    audio_decodes => "audio decodes",
    /// Encoded images decoded.
    image_decodes => "image decodes",
    /// Text paragraphs shaped.
    paragraphs_shaped => "paragraphs shaped",
    /// Frames read back from a render surface.
    readbacks => "surface readbacks",
}

/// Add `amount` to `counter`.
pub fn add(counter: Counter, amount: u64) {
    VALUES[counter as usize].fetch_add(amount, Ordering::Relaxed);
}

/// Add one to `counter`.
pub fn count(counter: Counter) {
    add(counter, 1);
}

impl std::fmt::Display for WorkCounts {
    /// One line of the counters above zero, such as
    /// `work: ffprobe spawns 1, video frames decoded 90, peak memory 412 MB`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "work:")?;
        let mut first = true;
        let mut item = |f: &mut std::fmt::Formatter<'_>, text: String| {
            let separator = if std::mem::take(&mut first) { " " } else { ", " };
            write!(f, "{separator}{text}")
        };
        for (label, value) in LABELS.iter().zip(self.values()) {
            if value > 0 {
                let value = if label.contains("bytes") { format!("{} MB", value / (1 << 20)) } else { value.to_string() };
                item(f, format!("{label} {value}"))?;
            }
        }
        if self.peak_rss_bytes > 0 {
            item(f, format!("peak memory {} MB", self.peak_rss_bytes / (1 << 20)))?;
        }
        Ok(())
    }
}

/// Largest resident set of this process so far, 0 where the platform has no reading.
#[cfg(windows)]
pub fn peak_rss_bytes() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, size: u32) -> i32;
    }
    let mut counters = ProcessMemoryCounters::default();
    counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
    // SAFETY: the struct matches PROCESS_MEMORY_COUNTERS and `cb` holds its size.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok != 0 {
        counters.peak_working_set_size as u64
    } else {
        0
    }
}

/// Largest resident set of this process so far, 0 where the platform has no reading.
#[cfg(not(windows))]
pub fn peak_rss_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
            line.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .map_or(0, |kilobytes| kilobytes * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_difference_holds_the_work_between_and_prints_what_happened() {
        let before = snapshot();
        add(Counter::decoded_bytes, 3 << 20);
        count(Counter::ffprobe_spawns);
        let work = snapshot() - before;
        // Other tests of this process count too, so the difference is a floor.
        assert!(work.ffprobe_spawns >= 1 && work.decoded_bytes >= 3 << 20);
        let line = WorkCounts { ffprobe_spawns: 2, decoded_bytes: 5 << 20, peak_rss_bytes: 7 << 20, ..Default::default() }
            .to_string();
        assert_eq!(line, "work: ffprobe spawns 2, video bytes decoded 5 MB, peak memory 7 MB");
        assert!(peak_rss_bytes() > 0, "the platform reports its memory peak");
    }
}
