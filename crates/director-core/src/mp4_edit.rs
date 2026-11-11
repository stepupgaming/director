//! The edit list of the first sound track of an MP4 or QuickTime file, which
//! places the track's samples on the file's timeline. Encoders put priming
//! samples before the sound (1024 for FFmpeg's AAC, 2112 for Apple's) and an
//! edit that skips them, and a track that starts after the picture opens with
//! an empty edit. A player honors both, so a decode that keeps the priming or
//! drops the empty edit plays out of step with the picture.

/// Where the decoded samples of a sound track sit on the file's timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundEdit {
    /// Seconds of silence before the first sample plays.
    pub delay: f64,
    /// Seconds of decoded samples skipped at the start.
    pub skip: f64,
    /// Seconds the track plays after the skip. `None` plays every sample.
    pub duration: Option<f64>,
}

impl SoundEdit {
    /// Interleaved stereo `samples` at `rate` laid on the file's timeline.
    pub fn apply(&self, mut samples: Vec<f32>, rate: u32) -> Vec<f32> {
        let values = |seconds: f64| (seconds * rate as f64).round() as usize * 2;
        samples.drain(..values(self.skip).min(samples.len()));
        if let Some(duration) = self.duration {
            samples.truncate(values(duration));
        }
        let delay = values(self.delay);
        if delay > 0 {
            samples.splice(0..0, std::iter::repeat_n(0.0, delay));
        }
        samples
    }
}

/// The edit of the first sound track when `data` is an MP4 or QuickTime
/// file whose sound track has an edit list, `None` for any other bytes or a
/// track without one, and `Err` for an edit list this reader cannot place
/// (several media edits, a rate other than 1, fragments), which FFmpeg reads.
pub fn sound_edit(data: &[u8]) -> Result<Option<SoundEdit>, String> {
    const TOP: [&[u8; 4]; 8] = [b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot", b"uuid"];
    let top = children(data);
    if !top.first().is_some_and(|(kind, _)| TOP.contains(&kind)) {
        return Ok(None);
    }
    if top.iter().any(|(kind, _)| kind == b"moof") {
        return Err("a fragmented file".into());
    }
    let Some(moov) = child(&top, b"moov") else { return Ok(None) };
    let moov = children(moov);
    let movie_scale = child(&moov, b"mvhd").and_then(timescale).ok_or("no movie timescale")?;
    for (_, trak) in moov.iter().filter(|(kind, _)| kind == b"trak") {
        let trak = children(trak);
        let Some(mdia) = child(&trak, b"mdia").map(children) else { continue };
        if child(&mdia, b"hdlr").and_then(|hdlr| hdlr.get(8..12)) != Some(b"soun") {
            continue;
        }
        let media_scale = child(&mdia, b"mdhd").and_then(timescale).ok_or("no media timescale")?;
        let Some(elst) = child(&trak, b"edts").map(children).and_then(|edts| child(&edts, b"elst")) else {
            return Ok(None);
        };
        return edit(elst, movie_scale, media_scale).map(Some);
    }
    Ok(None)
}

/// Reads the entries of an `elst` box: empty edits, then one media edit.
fn edit(elst: &[u8], movie_scale: f64, media_scale: f64) -> Result<SoundEdit, String> {
    let malformed = || "a malformed edit list".to_string();
    let version = *elst.first().ok_or_else(malformed)?;
    let count = be(elst.get(4..8).ok_or_else(malformed)?) as usize;
    let size = if version == 1 { 20 } else { 12 };
    let entries = elst.get(8..8 + count * size).ok_or_else(malformed)?;
    let mut sound = SoundEdit { delay: 0.0, skip: 0.0, duration: None };
    let mut placed = false;
    for entry in entries.chunks_exact(size) {
        let (duration, media_time, rate) = if version == 1 {
            (be(&entry[0..8]), be(&entry[8..16]) as i64, &entry[16..20])
        } else {
            (be(&entry[0..4]), be(&entry[4..8]) as u32 as i32 as i64, &entry[8..12])
        };
        let seconds = duration as f64 / movie_scale;
        match (placed, media_time) {
            (false, -1) => sound.delay += seconds,
            (false, _) if media_time >= 0 && rate == [0, 1, 0, 0] => {
                sound.skip = media_time as f64 / media_scale;
                sound.duration = (duration > 0).then_some(seconds);
                placed = true;
            }
            _ => return Err("an edit this reader cannot place".into()),
        }
    }
    if placed {
        Ok(sound)
    } else {
        Err("no media edit".into())
    }
}

/// Timescale of an `mvhd` or `mdhd` box.
fn timescale(header: &[u8]) -> Option<f64> {
    let at = if *header.first()? == 1 { 20 } else { 12 };
    let scale = be(header.get(at..at + 4)?);
    (scale > 0).then_some(scale as f64)
}

/// Big-endian unsigned integer of up to eight bytes.
fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |value, byte| value << 8 | *byte as u64)
}

fn child<'a>(boxes: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes.iter().find(|(found, _)| found == kind).map(|(_, body)| *body)
}

/// The boxes in `data` up to the first one whose size does not fit.
fn children(mut data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut boxes = Vec::new();
    while data.len() >= 8 {
        let kind: [u8; 4] = data[4..8].try_into().unwrap();
        let (header, size) = match be(&data[0..4]) {
            0 => (8, data.len() as u64),
            1 if data.len() >= 16 => (16, be(&data[8..16])),
            1 => break,
            size => (8, size),
        };
        if size < header as u64 || size > data.len() as u64 {
            break;
        }
        boxes.push((kind, &data[header..size as usize]));
        data = &data[size as usize..];
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        [&(8 + body.len() as u32).to_be_bytes()[..], kind, body].concat()
    }

    /// A movie at timescale 1000 with one sound track at 48000 and `entries`.
    fn movie(entries: &[(u32, i32, [u8; 4])]) -> Vec<u8> {
        let mut header = vec![0; 20];
        header[12..16].copy_from_slice(&1000u32.to_be_bytes());
        let mut media = vec![0; 20];
        media[12..16].copy_from_slice(&48000u32.to_be_bytes());
        let hdlr = [&[0; 8][..], b"soun", &[0; 12]].concat();
        let mut elst = [0u32.to_be_bytes(), (entries.len() as u32).to_be_bytes()].concat();
        for (duration, media_time, rate) in entries {
            elst.extend([&duration.to_be_bytes()[..], &media_time.to_be_bytes(), rate].concat());
        }
        let mdia = boxed(b"mdia", &[boxed(b"mdhd", &media), boxed(b"hdlr", &hdlr)].concat());
        let trak = boxed(b"trak", &[boxed(b"edts", &boxed(b"elst", &elst)), mdia].concat());
        [boxed(b"ftyp", b"isom"), boxed(b"moov", &[boxed(b"mvhd", &header), trak].concat())].concat()
    }

    #[test]
    fn reads_the_priming_skip_and_the_empty_edit_before_it() {
        const ONE: [u8; 4] = [0, 1, 0, 0];
        let priming = sound_edit(&movie(&[(2000, 1024, ONE)])).unwrap().unwrap();
        assert_eq!(priming, SoundEdit { delay: 0.0, skip: 1024.0 / 48000.0, duration: Some(2.0) });
        let late = sound_edit(&movie(&[(250, -1, ONE), (2000, 2112, ONE)])).unwrap().unwrap();
        assert_eq!(late, SoundEdit { delay: 0.25, skip: 2112.0 / 48000.0, duration: Some(2.0) });
        assert!(sound_edit(&movie(&[(1000, 0, ONE), (1000, 48000, ONE)])).is_err(), "two media edits");
        assert!(sound_edit(&movie(&[(1000, 0, [0, 2, 0, 0])])).is_err(), "double rate");
        assert_eq!(sound_edit(b"ID3\x04\0\0\0\0\0\0").unwrap(), None, "an MP3");
        assert_eq!(sound_edit(b"RIFF\0\0\0\0WAVEfmt ").unwrap(), None, "a WAV");
    }

    #[test]
    fn lays_samples_on_the_timeline() {
        let samples: Vec<f32> = (0..20).map(|i| i as f32).collect();
        let edit = SoundEdit { delay: 0.2, skip: 0.2, duration: Some(0.5) };
        let placed = edit.apply(samples, 10);
        assert_eq!(placed, [0.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0]);
    }
}
