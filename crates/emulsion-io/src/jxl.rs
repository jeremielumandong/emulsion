//! JPEG XL through `jxl-oxide`, a pure-Rust decoder: lossless and lossy
//! images, 8 and 16 bit, with the embedded colour profile honoured.

use crate::import::{Decoded, decode_with, document_from};
use crate::{IoError, Result};
use emulsion_core::Document;
use std::io::BufReader;
use std::path::Path;

pub fn is_jxl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jxl"))
}

pub fn decode(path: &Path) -> Result<Decoded> {
    let file = BufReader::new(std::fs::File::open(path)?);
    let decoder = jxl_oxide::integration::JxlDecoder::new(file)
        .map_err(|e| IoError::Unsupported(format!("JPEG XL: {e}")))?;
    decode_with(decoder)
}

pub fn open(path: &Path) -> Result<Document> {
    document_from(path, decode(path)?)
}
