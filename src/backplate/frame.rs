//! The backplate's frames: `d5 aa 96`, a type and a length (u16, little
//! endian), the payload, then a CRC-16/CCITT (poly 0x1021, init 0) of type,
//! length and payload, little endian.

const MAGIC: [u8; 3] = [0xd5, 0xaa, 0x96];
/// The largest payload either side sends: a firmware update record.
pub const MAX_PAYLOAD: usize = 0x400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: u16, payload: &[u8]) -> Frame {
        Frame { kind, payload: payload.to_vec() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(9 + self.payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.kind.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.payload);
        let crc = crc16(&out[3..]);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// Frames from a byte stream, as stock's reader parses them: bytes before a
/// magic are skipped; a frame whose CRC does not match is consumed whole,
/// and a header claiming more than `MAX_PAYLOAD` is consumed alone, both
/// counted as corrupt with the type they claimed.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    /// Frames dropped for a bad CRC or length.
    pub dropped: u32,
    /// The types of the frames dropped for a bad CRC since the last take.
    corrupt: Vec<u16>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let Some(start) = self.buf.windows(3).position(|w| w == MAGIC) else {
                // Keep a partial magic at the end.
                let keep = self.buf.len().min(2);
                self.buf.drain(..self.buf.len() - keep);
                return frames;
            };
            self.buf.drain(..start);
            if self.buf.len() < 7 {
                return frames;
            }
            let kind = u16::from_le_bytes([self.buf[3], self.buf[4]]);
            let len = usize::from(u16::from_le_bytes([self.buf[5], self.buf[6]]));
            if len > MAX_PAYLOAD {
                self.dropped += 1;
                self.corrupt.push(kind);
                self.buf.drain(..7);
                continue;
            }
            if self.buf.len() < 9 + len {
                return frames;
            }
            let crc = u16::from_le_bytes([self.buf[7 + len], self.buf[8 + len]]);
            if crc == crc16(&self.buf[3..7 + len]) {
                frames.push(Frame { kind, payload: self.buf[7..7 + len].to_vec() });
                self.buf.drain(..9 + len);
            } else {
                self.dropped += 1;
                self.corrupt.push(kind);
                self.buf.drain(..9 + len);
            }
        }
    }

    /// Forgets a partial frame (stock does so with each reset).
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn take_corrupt(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.corrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn frames_encode_as_stock_sends_them() {
        // Taken from stock nlclient's traffic with nest-qemu's backplate.
        assert_eq!(Frame::new(0x83, &[]).encode(), hex("d5aa9683000000e446"));
        assert_eq!(Frame::new(0xff, &[]).encode(), hex("d5aa96ff000000a34b"));
        assert_eq!(Frame::new(0x85, &[]).encode(), hex("d5aa96850000007d61"));
        assert_eq!(Frame::new(0xc0, &[0, 0, 0, 0]).encode(), hex("d5aa96c000040000000000f00d"));
    }

    #[test]
    fn a_stream_split_anywhere_gives_its_frames_and_drops_corrupt_ones() {
        let mut stream = vec![0x00, 0xd5];
        stream.extend(Frame::new(0x04, &[1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]).encode());
        let mut bad = Frame::new(0x02, &[0x66, 0x08, 0xc2, 0x01]).encode();
        bad[8] ^= 1;
        stream.extend(bad);
        stream.extend(Frame::new(0x09, &[1; 15]).encode());
        for split in 0..stream.len() {
            let mut d = Decoder::default();
            let mut got = d.push(&stream[..split]);
            got.extend(d.push(&stream[split..]));
            assert_eq!(got.iter().map(|f| f.kind).collect::<Vec<_>>(), [0x04, 0x09], "split at {split}");
            assert_eq!(d.dropped, 1);
        }
    }

    #[test]
    fn a_corrupt_frame_is_consumed_whole_and_an_oversized_header_alone() {
        // The bad frame's payload holds a magic: stock does not resync in it.
        let mut bad = Frame::new(0x02, &[0xd5, 0xaa, 0x96, 0x02]).encode();
        bad[9] ^= 1;
        let mut stream = bad;
        stream.extend(hex("d5aa96330001ff"));
        stream.extend(Frame::new(0x05, &[0; 4]).encode());
        let mut d = Decoder::default();
        assert_eq!(d.push(&stream).iter().map(|f| f.kind).collect::<Vec<_>>(), [0x05]);
        assert_eq!(d.take_corrupt(), [0x02, 0x33]);
    }
}
