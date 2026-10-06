//! Strict, bounded original PNG source codec shared by PSD and native IO.
//! Validation and source-digest work runs on existing IO workers, never in core.
use emulsion_raster::{Raster, TILE};
use std::{collections::BTreeSet, fmt, sync::Arc};
pub(crate) const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_TOTAL_SOURCE_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const MAX_SOURCE_PIXELS: u64 = 16_000_000;
pub(crate) const MAX_TOTAL_SOURCE_PIXELS: u64 = 32_000_000;
pub(crate) const MAX_SOURCE_NATIVE_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_TOTAL_SOURCE_NATIVE_BYTES: u64 = 256 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceError {
    Malformed(&'static str),
    Unsupported(&'static str),
    /// A transient export prerequisite failed; never flatten to hide it.
    Unavailable(&'static str),
}
impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(s) => write!(f, "malformed original PNG source: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported original PNG source: {s}"),
            Self::Unavailable(s) => write!(f, "Smart source export unavailable: {s}"),
        }
    }
}
type Result<T> = std::result::Result<T, SourceError>;

#[derive(Clone, Copy)]
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.at..]
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(SourceError::Malformed("length overflow"))?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or(SourceError::Malformed("truncated PNG"))?;
        self.at = end;
        Ok(out)
    }
    fn number(&mut self, n: usize) -> Result<u64> {
        Ok(self.take(n)?.iter().fold(0, |a, b| a << 8 | u64::from(*b)))
    }
    fn length(&mut self, n: usize) -> Result<usize> {
        usize::try_from(self.number(n)?)
            .map_err(|_| SourceError::Malformed("length exceeds address space"))
    }
}
pub(crate) fn dimensions(width: u32, height: u32) -> Result<u64> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 || pixels > MAX_SOURCE_PIXELS
    {
        return Err(SourceError::Unsupported("source pixel limit"));
    }
    Ok(pixels)
}
pub(crate) fn native_storage(width: u32, height: u32) -> Result<u64> {
    dimensions(width, height)?;
    let bytes = u64::from(width.div_ceil(TILE))
        * u64::from(height.div_ceil(TILE))
        * u64::from(TILE)
        * u64::from(TILE)
        * 8;
    if bytes > MAX_SOURCE_NATIVE_BYTES {
        return Err(SourceError::Unsupported(
            "source padded native tile byte limit",
        ));
    }
    Ok(bytes)
}
#[derive(Default)]
pub(crate) struct SourceBudget {
    pub(crate) encoded: usize,
    pub(crate) pixels: u64,
    pub(crate) native: u64,
}
impl SourceBudget {
    pub(crate) fn charge(&mut self, width: u32, height: u32, encoded: usize) -> Result<()> {
        let pixels = dimensions(width, height)?;
        let native = native_storage(width, height)?;
        let encoded_total = self
            .encoded
            .checked_add(encoded)
            .ok_or(SourceError::Unsupported("source byte total overflow"))?;
        if encoded > MAX_SOURCE_BYTES || encoded_total > MAX_TOTAL_SOURCE_BYTES {
            return Err(SourceError::Unsupported("total embedded source byte limit"));
        }
        let pixel_total = self
            .pixels
            .checked_add(pixels)
            .ok_or(SourceError::Unsupported("source pixel total overflow"))?;
        if pixel_total > MAX_TOTAL_SOURCE_PIXELS {
            return Err(SourceError::Unsupported("total source pixel limit"));
        }
        let native_total = self
            .native
            .checked_add(native)
            .ok_or(SourceError::Unsupported(
                "native source byte total overflow",
            ))?;
        if native_total > MAX_TOTAL_SOURCE_NATIVE_BYTES {
            return Err(SourceError::Unsupported(
                "total padded native tile byte limit",
            ));
        }
        self.encoded = encoded_total;
        self.pixels = pixel_total;
        self.native = native_total;
        Ok(())
    }
}
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xedb8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}
pub(crate) fn png_crc(kind: &[u8], data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc = TABLE[((crc ^ u32::from(*byte)) & 255) as usize] ^ (crc >> 8);
    }
    !crc
}
pub(crate) fn strict_idat(chunks: &[&[u8]], expected: usize) -> Result<()> {
    use flate2::{Decompress, FlushDecompress, Status};
    let mut zlib = Decompress::new(true);
    let mut scratch = [0u8; 16 * 1024];
    let mut ended = false;
    let mut input_bytes = 0u64;
    for chunk in chunks {
        input_bytes += chunk.len() as u64;
        if ended {
            if !chunk.is_empty() {
                return Err(SourceError::Malformed("trailing PNG IDAT stream data"));
            }
            continue;
        }
        let mut input = *chunk;
        loop {
            let (before_in, before_out) = (zlib.total_in(), zlib.total_out());
            let status = zlib
                .decompress(input, &mut scratch, FlushDecompress::None)
                .map_err(|_| SourceError::Malformed("PNG zlib stream or checksum"))?;
            let consumed = (zlib.total_in() - before_in) as usize;
            let produced = (zlib.total_out() - before_out) as usize;
            input = &input[consumed..];
            if zlib.total_out() > expected as u64 {
                return Err(SourceError::Malformed("PNG scanline byte count"));
            }
            if status == Status::StreamEnd {
                ended = true;
                if !input.is_empty() {
                    return Err(SourceError::Malformed("trailing PNG IDAT stream data"));
                }
                break;
            }
            if consumed == 0 && produced == 0 {
                if !input.is_empty() {
                    return Err(SourceError::Malformed("stalled PNG zlib stream"));
                }
                break;
            }
            if input.is_empty() && produced < scratch.len() {
                break;
            }
        }
    }
    while !ended {
        let before = zlib.total_out();
        let status = zlib
            .decompress(&[], &mut scratch, FlushDecompress::Finish)
            .map_err(|_| SourceError::Malformed("incomplete PNG zlib stream"))?;
        if zlib.total_out() > expected as u64 {
            return Err(SourceError::Malformed("PNG scanline byte count"));
        }
        ended = status == Status::StreamEnd;
        if !ended && zlib.total_out() == before {
            return Err(SourceError::Malformed("incomplete PNG zlib stream"));
        }
    }
    if zlib.total_in() != input_bytes || zlib.total_out() != expected as u64 {
        return Err(SourceError::Malformed("PNG stream byte count"));
    }
    Ok(())
}
#[cfg(test)]
pub(crate) fn png_source(data: &[u8]) -> Result<Arc<Raster>> {
    png_source_with_budget(data, &mut SourceBudget::default())
}
pub(crate) fn png_source_with_budget(
    data: &[u8],
    budget: &mut SourceBudget,
) -> Result<Arc<Raster>> {
    if data.len() > MAX_SOURCE_BYTES {
        return Err(SourceError::Unsupported("embedded PNG byte limit"));
    }
    let mut c = Cursor::new(data);
    if c.take(8)? != b"\x89PNG\r\n\x1a\n" {
        return Err(SourceError::Unsupported("embedded source is not PNG"));
    }
    let mut size = None;
    let mut ended = false;
    let mut image_data = false;
    let mut metadata = BTreeSet::new();
    let mut chunks = Vec::new();
    let mut channels = 0u64;
    while !c.remaining().is_empty() {
        let n = c.length(4)?;
        let key = c.take(4)?;
        let body = c.take(n)?;
        let expected = c.number(4)? as u32;
        // image's PNG frame decoder need not read through IEND. Verify every
        // chunk here, including the final checksum, before accepting a source.
        if png_crc(key, body) != expected {
            return Err(SourceError::Malformed("PNG chunk checksum"));
        }
        if size.is_none() && key != b"IHDR" {
            return Err(SourceError::Malformed("PNG has no initial IHDR"));
        }
        if matches!(key, b"sRGB" | b"gAMA" | b"cHRM" | b"pHYs")
            && (image_data || !metadata.insert(key.to_vec()))
        {
            return Err(SourceError::Malformed("PNG metadata order or duplicate"));
        }
        match key {
            b"IHDR" => {
                if size.is_some() || body.len() != 13 {
                    return Err(SourceError::Malformed("PNG header"));
                }
                let mut h = Cursor::new(body);
                let (w, hgt) = (h.number(4)? as u32, h.number(4)? as u32);
                dimensions(w, hgt)?;
                let (depth, color) = (h.number(1)?, h.number(1)?);
                channels = if color == 2 { 3 } else { 4 };
                if depth != 8
                    || !matches!(color, 2 | 6)
                    || h.number(1)? != 0
                    || h.number(1)? != 0
                    || h.number(1)? != 0
                {
                    return Err(SourceError::Unsupported(
                        "PNG must be non-interlaced RGB8 or RGBA8",
                    ));
                }
                size = Some((w, hgt));
            }
            b"sRGB" if body.len() == 1 && body[0] <= 3 => {}
            b"gAMA" if body == 45455u32.to_be_bytes() => {}
            b"cHRM"
                if body
                    == [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000]
                        .into_iter()
                        .flat_map(u32::to_be_bytes)
                        .collect::<Vec<_>>() => {}
            b"iCCP" | b"cICP" | b"sRGB" | b"gAMA" | b"cHRM" => {
                return Err(SourceError::Unsupported(
                    "unknown embedded PNG color profile",
                ));
            }
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(SourceError::Unsupported("animated PNG source"));
            }
            b"IEND" => {
                if !image_data || !body.is_empty() || !c.remaining().is_empty() {
                    return Err(SourceError::Malformed("PNG end/trailing data"));
                }
                ended = true;
            }
            b"IDAT" => {
                image_data = true;
                if chunks.len() >= 1024 {
                    return Err(SourceError::Unsupported("PNG IDAT chunk count limit"));
                }
                chunks.push(body);
            }
            b"pHYs" if body.len() == 9 && body[8] <= 1 => {}
            _ => {
                return Err(SourceError::Unsupported(
                    "PNG chunk outside source-only subset",
                ));
            }
        }
    }
    if !ended {
        return Err(SourceError::Malformed("PNG has no IEND"));
    }
    let (width, height) = size.ok_or(SourceError::Malformed("PNG header"))?;
    // Charge padded native tiles before decoding, including conservative
    // storage for transparent/sparse images. Logical pixels alone do not bound
    // a skinny image's fixed-size native tiles.
    budget.charge(width, height, data.len())?;
    let scanline_bytes = u64::from(height) * (1 + u64::from(width) * channels);
    strict_idat(
        &chunks,
        usize::try_from(scanline_bytes)
            .map_err(|_| SourceError::Unsupported("PNG scanline byte limit"))?,
    )?;
    let image = image::load_from_memory_with_format(data, image::ImageFormat::Png)
        .map_err(|_| SourceError::Malformed("PNG pixel data"))?;
    let pixels = image.into_rgba8();
    if pixels.dimensions() != (width, height) {
        return Err(SourceError::Malformed(
            "PNG dimensions changed during decoding",
        ));
    }
    let source = Raster::from_srgba8(width, height, pixels.as_raw());
    // Original bytes retain hidden RGB and low-alpha samples that cannot be
    // reconstructed from the native premultiplied RGBA16 rendering alone.
    Ok(Arc::new(source))
}
