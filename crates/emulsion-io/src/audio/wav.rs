//! Writing WAV files a block at a time: 32-bit float or 16-bit PCM, any
//! channel count. The header is written first with empty sizes and filled
//! in by [`WavWriter::finish`], so the length need not be known up front
//! (as when recording). Used by the mixdown and by microphone recording.
use anyhow::{Result, bail};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// How samples are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// IEEE float, written as given.
    Float32,
    /// Signed 16-bit, clipped to −1…1.
    Int16,
}

impl SampleFormat {
    fn bytes(self) -> u16 {
        match self {
            Self::Float32 => 4,
            Self::Int16 => 2,
        }
    }
}

/// The most sample bytes a WAV file holds (its sizes are 32-bit).
pub const MAX_DATA_BYTES: u64 = u32::MAX as u64 - 64;

/// A WAV file being written.
pub struct WavWriter<W: Write + Seek> {
    out: W,
    channels: u16,
    format: SampleFormat,
    data_bytes: u64,
}

impl WavWriter<BufWriter<File>> {
    /// Create (or replace) the file at `path`.
    pub fn create(path: &Path, rate: u32, channels: u16, format: SampleFormat) -> Result<Self> {
        Self::new(BufWriter::new(File::create(path)?), rate, channels, format)
    }
}

impl<W: Write + Seek> WavWriter<W> {
    /// Start a WAV stream of `channels` interleaved channels at `rate`.
    pub fn new(mut out: W, rate: u32, channels: u16, format: SampleFormat) -> Result<Self> {
        if channels == 0 || rate == 0 {
            bail!("A WAV file needs a sample rate and at least one channel")
        }
        let block_align = channels * format.bytes();
        out.write_all(b"RIFF")?;
        out.write_all(&36u32.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        let tag: u16 = match format {
            SampleFormat::Float32 => 3,
            SampleFormat::Int16 => 1,
        };
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&rate.to_le_bytes())?;
        out.write_all(&(rate * u32::from(block_align)).to_le_bytes())?;
        out.write_all(&block_align.to_le_bytes())?;
        out.write_all(&(format.bytes() * 8).to_le_bytes())?;
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            out,
            channels,
            format,
            data_bytes: 0,
        })
    }

    /// Append interleaved samples (a whole number of frames).
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let bytes = samples.len() as u64 * u64::from(self.format.bytes());
        if self.data_bytes + bytes > MAX_DATA_BYTES {
            bail!("The sound is too long for one WAV file")
        }
        match self.format {
            SampleFormat::Float32 => {
                for s in samples {
                    self.out.write_all(&s.to_le_bytes())?;
                }
            }
            SampleFormat::Int16 => {
                for s in samples {
                    let v = (s.clamp(-1., 1.) * 32767.).round() as i16;
                    self.out.write_all(&v.to_le_bytes())?;
                }
            }
        }
        self.data_bytes += bytes;
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.data_bytes / u64::from(self.channels * self.format.bytes())
    }

    /// Fill in the sizes and flush. Returns the output.
    pub fn finish(mut self) -> Result<W> {
        let data = self.data_bytes as u32;
        self.out.seek(SeekFrom::Start(4))?;
        self.out.write_all(&(36 + data).to_le_bytes())?;
        self.out.seek(SeekFrom::Start(40))?;
        self.out.write_all(&data.to_le_bytes())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn headers_hold_the_format_and_sizes() {
        let mut w =
            WavWriter::new(Cursor::new(Vec::new()), 48_000, 1, SampleFormat::Int16).unwrap();
        w.write(&[0., 0.5, -2.]).unwrap();
        assert_eq!(w.frames(), 3);
        let bytes = w.finish().unwrap().into_inner();
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 42);
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1, "PCM");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            48_000
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        let samples: Vec<i16> = bytes[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(samples, [0, 16384, -32767]);

        let mut w =
            WavWriter::new(Cursor::new(Vec::new()), 44_100, 2, SampleFormat::Float32).unwrap();
        w.write(&[0.25, -0.25]).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 3, "float");
        assert_eq!(u16::from_le_bytes([bytes[32], bytes[33]]), 8, "block align");
        assert_eq!(f32::from_le_bytes(bytes[44..48].try_into().unwrap()), 0.25);
        assert!(WavWriter::new(Cursor::new(Vec::new()), 0, 1, SampleFormat::Int16).is_err());
    }
}
