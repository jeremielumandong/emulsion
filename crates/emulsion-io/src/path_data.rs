//! Lossless path blobs shared by the live document and history commits.
use crate::{IoError, Result};
use emulsion_raster::vector::{Anchor, MAX_ANCHORS, Path, Pt, SubPath};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Seek},
    sync::Arc,
};
use zip::ZipArchive;

const MAGIC: &[u8; 4] = b"EMP1";
const MAX_BYTES: u64 = 16 << 20;
const MAX_TOTAL_BYTES: u64 = crate::ora::MAX_NATIVE_MANIFEST_BYTES;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum PathData {
    Blob(String),
    Legacy(Path),
}

#[derive(Default)]
pub(crate) struct PathPool {
    // Byte equality, including a hash collision check supplied by HashMap.
    blobs: HashMap<Vec<u8>, String>,
    bytes: u64,
}

impl PathPool {
    pub(crate) fn add(&mut self, path: &Path) -> Result<PathData> {
        let bytes = encode(path)?;
        if let Some(name) = self.blobs.get(&bytes) {
            return Ok(PathData::Blob(name.clone()));
        }
        if self.bytes + bytes.len() as u64 > MAX_TOTAL_BYTES {
            return Err(bad("combined path data is too large"));
        }
        self.bytes += bytes.len() as u64;
        let name = format!("emulsion/paths/{}.bin", self.blobs.len());
        self.blobs.insert(bytes, name.clone());
        Ok(PathData::Blob(name))
    }

    pub(crate) fn entries(self) -> impl Iterator<Item = (String, Vec<u8>)> {
        let mut entries: Vec<_> = self
            .blobs
            .into_iter()
            .map(|(bytes, name)| (name, bytes))
            .collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        entries.into_iter()
    }
}

#[derive(Default)]
pub(crate) struct PathReader {
    paths: HashMap<String, Arc<Path>>,
    bytes: u64,
}

impl PathReader {
    pub(crate) fn read<R: Read + Seek>(
        &mut self,
        data: PathData,
        zip: &mut ZipArchive<R>,
    ) -> Result<Arc<Path>> {
        match data {
            PathData::Legacy(path) => {
                check_anchors(path.anchor_count())?;
                Ok(Arc::new(path))
            }
            PathData::Blob(name) => {
                if let Some(path) = self.paths.get(&name) {
                    return Ok(path.clone());
                }
                if !name.starts_with("emulsion/paths/") || !name.ends_with(".bin") {
                    return Err(bad("invalid path blob name"));
                }
                let budget = MAX_TOTAL_BYTES.saturating_sub(self.bytes).min(MAX_BYTES);
                let bytes = crate::ora::read_entry(zip, &name, budget)?;
                let path = Arc::new(decode(&bytes)?);
                self.bytes += bytes.len() as u64;
                self.paths.insert(name, path.clone());
                Ok(path)
            }
        }
    }
}

fn bad(message: &str) -> IoError {
    IoError::Manifest(message.into())
}
fn check_anchors(count: usize) -> Result<()> {
    if count > MAX_ANCHORS {
        Err(bad("a path has too many anchors"))
    } else {
        Ok(())
    }
}
fn same(a: Pt, b: Pt) -> bool {
    a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits()
}
fn point(out: &mut Vec<u8>, p: Pt) -> Result<()> {
    if !p.0.is_finite() || !p.1.is_finite() {
        return Err(bad("non-finite path coordinate"));
    }
    out.extend_from_slice(&p.0.to_le_bytes());
    out.extend_from_slice(&p.1.to_le_bytes());
    Ok(())
}
fn count(out: &mut Vec<u8>, n: usize) -> Result<()> {
    out.extend_from_slice(
        &u32::try_from(n)
            .map_err(|_| bad("path is too large"))?
            .to_le_bytes(),
    );
    Ok(())
}

fn encode(path: &Path) -> Result<Vec<u8>> {
    check_anchors(path.anchor_count())?;
    let mut out = MAGIC.to_vec();
    count(&mut out, path.subpaths.len())?;
    for sub in &path.subpaths {
        out.push(u8::from(sub.closed));
        count(&mut out, sub.anchors.len())?;
        for a in &sub.anchors {
            let hin = !same(a.h_in, a.p);
            let hout = !same(a.h_out, a.p);
            out.push(u8::from(a.smooth) | (u8::from(hin) << 1) | (u8::from(hout) << 2));
            point(&mut out, a.p)?;
            if hin {
                point(&mut out, a.h_in)?;
            }
            if hout {
                point(&mut out, a.h_out)?;
            }
        }
    }
    if out.len() as u64 > MAX_BYTES {
        return Err(bad("path blob is too large"));
    }
    Ok(out)
}

fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N]> {
    let (head, tail) = bytes
        .split_at_checked(N)
        .ok_or_else(|| bad("truncated path blob"))?;
    *bytes = tail;
    Ok(head.try_into().expect("checked length"))
}
fn read_point(bytes: &mut &[u8]) -> Result<Pt> {
    let p = (
        f64::from_le_bytes(take(bytes)?),
        f64::from_le_bytes(take(bytes)?),
    );
    if !p.0.is_finite() || !p.1.is_finite() {
        return Err(bad("non-finite path coordinate"));
    }
    Ok(p)
}
fn decode(mut bytes: &[u8]) -> Result<Path> {
    if &take::<4>(&mut bytes)? != MAGIC {
        return Err(bad("unknown path blob format"));
    }
    let n = u32::from_le_bytes(take(&mut bytes)?) as usize;
    if n > bytes.len() / 5 {
        return Err(bad("invalid path subpath count"));
    }
    let mut path = Path {
        subpaths: Vec::with_capacity(n),
    };
    let mut total = 0usize;
    for _ in 0..n {
        let closed = take::<1>(&mut bytes)?[0];
        if closed > 1 {
            return Err(bad("invalid path closed flag"));
        }
        let n = u32::from_le_bytes(take(&mut bytes)?) as usize;
        total = total.saturating_add(n);
        check_anchors(total)?;
        if n > bytes.len() / 17 {
            return Err(bad("truncated path anchors"));
        }
        let mut anchors = Vec::with_capacity(n);
        for _ in 0..n {
            let flags = take::<1>(&mut bytes)?[0];
            if flags & !7 != 0 {
                return Err(bad("invalid path anchor flags"));
            }
            let p = read_point(&mut bytes)?;
            let h_in = if flags & 2 != 0 {
                read_point(&mut bytes)?
            } else {
                p
            };
            let h_out = if flags & 4 != 0 {
                read_point(&mut bytes)?
            } else {
                p
            };
            anchors.push(Anchor {
                p,
                h_in,
                h_out,
                smooth: flags & 1 != 0,
            });
        }
        path.subpaths.push(SubPath {
            closed: closed != 0,
            anchors,
        });
    }
    if !bytes.is_empty() {
        return Err(bad("unexpected bytes after path"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_is_bit_exact_and_identical_paths_share_one_blob() {
        let path = Path {
            subpaths: vec![SubPath {
                closed: true,
                anchors: vec![
                    Anchor::corner((0.1, -0.0)),
                    Anchor {
                        p: (2.3, 4.5),
                        h_in: (1.1, 3.2),
                        h_out: (6.7, 8.9),
                        smooth: true,
                    },
                    Anchor {
                        p: (0.0, 1.0),
                        h_in: (-0.0, 1.0),
                        h_out: (0.0, 1.0),
                        smooth: false,
                    },
                ],
            }],
        };
        let bytes = encode(&path).unwrap();
        let restored = decode(&bytes).unwrap();
        assert_eq!(encode(&restored).unwrap(), bytes);
        assert_eq!(path, restored);
        let mut pool = PathPool::default();
        pool.add(&path).unwrap();
        pool.add(&restored).unwrap();
        assert_eq!(pool.entries().count(), 1);
    }
    #[test]
    fn corrupt_and_oversized_paths_are_rejected() {
        let path = Path::from_svg("M 1 2 L 3 4").unwrap();
        let mut full = PathPool {
            bytes: MAX_TOTAL_BYTES,
            ..Default::default()
        };
        assert!(full.add(&path).is_err());
        let bytes = encode(&path).unwrap();
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err());
        }
        let mut invalid = bytes.clone();
        invalid.push(0);
        assert!(decode(&invalid).is_err());
        let mut invalid = bytes.clone();
        invalid[8] = 2;
        assert!(decode(&invalid).is_err());
        let mut invalid = bytes.clone();
        invalid[9..13].copy_from_slice(&((MAX_ANCHORS + 1) as u32).to_le_bytes());
        assert!(decode(&invalid).is_err());
        let mut invalid = bytes;
        invalid[14..22].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&invalid).is_err());
    }
}
