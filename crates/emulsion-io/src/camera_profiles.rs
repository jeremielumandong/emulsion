//! Bounded standalone DCP reader based on the public DNG tag specification.
//! No executable code or proprietary profile assets are bundled.
use crate::{IoError, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, serde::Serialize)]
pub struct Profile {
    pub name: String,
    pub camera: String,
    pub digest: [u8; 32],
    pub illuminants: [u16; 2],
    pub matrices: [Vec<f32>; 2],
    pub forward: [Vec<f32>; 2],
    pub curve: Vec<[f32; 2]>,
    pub maps: [Option<Table>; 2],
    pub look: Option<Table>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Table {
    pub dims: [usize; 3],
    pub data: Vec<[f32; 3]>,
    pub srgb_encoding: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ProfileSummary {
    pub name: String,
    pub camera: String,
    pub digest: [u8; 32],
}
impl ProfileSummary {
    pub fn compatible(&self, make: &str, model: &str) -> bool {
        compatible(&self.camera, make, model)
    }
}
fn compatible(camera: &str, make: &str, model: &str) -> bool {
    let norm = |v: &str| {
        v.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    camera.is_empty()
        || norm(camera) == norm(model)
        || norm(camera) == norm(&format!("{make} {model}"))
}
fn bad(v: impl ToString) -> IoError {
    IoError::Unsupported(format!("Camera profile: {}", v.to_string()))
}
pub fn directory() -> PathBuf {
    crate::recent::data_dir().join("camera-profiles")
}
pub fn path(d: &[u8; 32]) -> PathBuf {
    directory().join(format!(
        "{}.dcp",
        d.iter().map(|v| format!("{v:02x}")).collect::<String>()
    ))
}
pub fn read(file: &Path) -> Result<Profile> {
    let mut bytes = vec![];
    std::fs::File::open(file)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    parse(&bytes)
}
pub fn load(d: &[u8; 32]) -> Result<std::sync::Arc<Profile>> {
    type Cached = (
        PathBuf,
        u64,
        Option<std::time::SystemTime>,
        std::sync::Arc<Profile>,
    );
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<Cached>>> =
        std::sync::OnceLock::new();
    let file = path(d);
    let metadata = std::fs::metadata(&file)?;
    let stamp = metadata.modified().ok();
    let mut cache = CACHE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((old, len, modified, profile)) = &*cache
        && old == &file
        && *len == metadata.len()
        && modified == &stamp
    {
        return Ok(profile.clone());
    }
    let profile = read(&file)?;
    if profile.digest != *d {
        return Err(bad("profile asset changed"));
    }
    let profile = std::sync::Arc::new(profile);
    *cache = Some((file, metadata.len(), stamp, profile.clone()));
    Ok(profile)
}

pub fn installed() -> Vec<ProfileSummary> {
    type Bank = (PathBuf, Option<std::time::SystemTime>, Vec<ProfileSummary>);
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<Bank>>> = std::sync::OnceLock::new();
    let dir = directory();
    let stamp = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
    let mut cache = CACHE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((path, modified, profiles)) = &*cache
        && path == &dir
        && modified == &stamp
    {
        return profiles.clone();
    }
    let profiles: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|v| v == "dcp"))
        .take(256)
        .filter_map(|e| read(&e.path()).ok())
        .map(|p| ProfileSummary {
            name: p.name,
            camera: p.camera,
            digest: p.digest,
        })
        .collect();
    *cache = Some((dir, stamp, profiles.clone()));
    profiles
}

pub fn install(file: &Path) -> Result<Profile> {
    let profile = read(file)?;
    std::fs::create_dir_all(directory())?;
    let target = path(&profile.digest);
    if !target.exists() {
        let mut bytes = vec![];
        std::fs::File::open(file)?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != profile.digest {
            return Err(bad("profile changed during import"));
        }
        crate::write_atomic(&target, |f| {
            use std::io::Write;
            f.write_all(&bytes)?;
            Ok(())
        })?;
    }
    Ok(profile)
}
pub fn resolve(name: &str) -> Option<[u8; 32]> {
    let found: Vec<_> = installed()
        .into_iter()
        .filter(|p| p.name.eq_ignore_ascii_case(name))
        .collect();
    if found.len() == 1 {
        Some(found[0].digest)
    } else {
        None
    }
}
impl Profile {
    pub fn compatible(&self, make: &str, model: &str) -> bool {
        compatible(&self.camera, make, model)
    }
    pub fn blend(&self, kelvin: Option<f32>) -> f32 {
        if self.matrices[1].is_empty() && self.forward[1].is_empty() {
            return 0.;
        }
        let t = |v| -> f32 {
            match v {
                17 | 3 => 2856.,
                20 => 5500.,
                21 | 1 | 4 => 6504.,
                22 => 7500.,
                23 => 5000.,
                _ => 6504.,
            }
        };
        let (a, b) = (t(self.illuminants[0]), t(self.illuminants[1]));
        if (a - b).abs() < 1. {
            return 0.;
        }
        ((1. / kelvin.unwrap_or(6504.) - 1. / a) / (1. / b - 1. / a)).clamp(0., 1.)
    }
}
fn parse(bytes: &[u8]) -> Result<Profile> {
    if bytes.len() < 8 || bytes.len() > 16 * 1024 * 1024 {
        return Err(bad("invalid profile size"));
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(bad("expected a TIFF camera profile")),
    };
    let u16at = |o: usize| -> Result<u16> {
        let b: [u8; 2] = bytes
            .get(o..o + 2)
            .ok_or_else(|| bad("truncated tag"))?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32at = |o: usize| -> Result<u32> {
        let b: [u8; 4] = bytes
            .get(o..o + 4)
            .ok_or_else(|| bad("truncated tag"))?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if ![42, 0x4352].contains(&u16at(2)?) {
        return Err(bad("unsupported profile signature"));
    }
    let ifd = u32at(4)? as usize;
    let count = u16at(ifd)? as usize;
    if count > 256 {
        return Err(bad("too many profile tags"));
    }
    let mut tags = BTreeMap::new();
    for i in 0..count {
        let at = ifd + 2 + i * 12;
        let tag = u16at(at)?;
        let kind = u16at(at + 2)?;
        let n = u32at(at + 4)? as usize;
        let size = match kind {
            1 | 2 | 7 => 1,
            3 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return Err(bad("unsupported TIFF field type")),
        };
        let len = n
            .checked_mul(size)
            .filter(|n| *n <= 16 * 1024 * 1024)
            .ok_or_else(|| bad("tag allocation limit"))?;
        let offset = if len <= 4 {
            at + 8
        } else {
            u32at(at + 8)? as usize
        };
        let data = bytes
            .get(offset..offset + len)
            .ok_or_else(|| bad("tag outside file"))?;
        if tags.insert(tag, (kind, n, data)).is_some() {
            return Err(bad("duplicate profile tag"));
        }
    }
    let text = |tag| -> Result<String> {
        let Some((kind, _, data)) = tags.get(&tag) else {
            return Ok(String::new());
        };
        if ![1, 2, 7].contains(kind) || data.len() > 4096 {
            return Err(bad("invalid profile text"));
        }
        Ok(String::from_utf8_lossy(data)
            .trim_end_matches('\0')
            .to_owned())
    };
    let values = |tag| -> Result<Vec<f32>> {
        let Some((kind, n, data)) = tags.get(&tag) else {
            return Ok(vec![]);
        };
        let integer = |b: &[u8]| {
            let b: [u8; 4] = b.try_into().unwrap();
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let mut out = Vec::with_capacity(*n);
        for i in 0..*n {
            let v = match kind {
                3 => {
                    let b: [u8; 2] = data[i * 2..i * 2 + 2].try_into().unwrap();
                    if little {
                        u16::from_le_bytes(b) as f32
                    } else {
                        u16::from_be_bytes(b) as f32
                    }
                }
                4 => integer(&data[i * 4..i * 4 + 4]) as f32,
                11 => f32::from_bits(integer(&data[i * 4..i * 4 + 4])),
                10 => {
                    let a = integer(&data[i * 8..i * 8 + 4]) as i32;
                    let b = integer(&data[i * 8 + 4..i * 8 + 8]) as i32;
                    a as f32 / b as f32
                }
                5 => {
                    integer(&data[i * 8..i * 8 + 4]) as f32
                        / integer(&data[i * 8 + 4..i * 8 + 8]) as f32
                }
                _ => return Err(bad("unsupported numeric tag type")),
            };
            if !v.is_finite() {
                return Err(bad("nonfinite profile value"));
            }
            out.push(v);
        }
        Ok(out)
    };
    // Unsupported adaptive/HDR/third-illuminant operations are never silently ignored.
    for tag in tags.keys() {
        if *tag >= 50000
            && ![
                50708, 50721, 50722, 50778, 50779, 50932, 50936, 50937, 50938, 50939, 50940, 50941,
                50942, 50964, 50965, 50981, 50982, 51107, 51108,
            ]
            .contains(tag)
        {
            return Err(bad(format!("unsupported profile operation tag {tag}")));
        }
    }
    if !text(50932)?.is_empty() {
        return Err(bad("profile requires a camera calibration signature"));
    }
    let matrices = [values(50721)?, values(50722)?];
    let forward = [values(50964)?, values(50965)?];
    if matrices[0].len() != 9 {
        return Err(bad("a three-channel ColorMatrix1 is required"));
    }
    for m in matrices.iter().chain(&forward) {
        if !m.is_empty() && (m.len() != 9 || m.iter().any(|v| v.abs() > 100.)) {
            return Err(bad("invalid RGB matrix"));
        }
    }
    let mut illuminants = [21, 21];
    for (i, tag) in [50778, 50779].into_iter().enumerate() {
        if let Some(v) = values(tag)?.first() {
            illuminants[i] = *v as u16;
        }
        if ![1, 3, 4, 17, 20, 21, 22, 23].contains(&illuminants[i]) {
            return Err(bad("unsupported calibration illuminant"));
        }
    }
    let curve = values(50940)?;
    if curve.len() > 8192 || !curve.len().is_multiple_of(2) {
        return Err(bad("invalid tone curve"));
    }
    let curve: Vec<[f32; 2]> = curve
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| [p[0], p[1]])
        .collect();
    if (!curve.is_empty()
        && (curve.len() < 2 || curve[0][0] != 0. || curve.last().unwrap()[0] != 1.))
        || curve.iter().flatten().any(|v| !(0.0..=1.0).contains(v))
        || curve.windows(2).any(|p| p[0][0] >= p[1][0])
    {
        return Err(bad("invalid tone curve coordinates"));
    }
    let table = |dims_tag, data_tag, encoding_tag| -> Result<Option<Table>> {
        let data = values(data_tag)?;
        if data.is_empty() {
            return Ok(None);
        }
        let dims = values(dims_tag)?;
        if dims.len() != 3 || dims.iter().any(|v| *v < 1. || *v > 256. || v.fract() != 0.) {
            return Err(bad("invalid table dimensions"));
        }
        let dims = [dims[0] as usize, dims[1] as usize, dims[2] as usize];
        if dims[1] < 2
            || dims.iter().product::<usize>() * 3 != data.len()
            || data.len() > 3 * 1024 * 1024
        {
            return Err(bad("invalid table length"));
        }
        if data.as_chunks::<3>().0.iter().any(|p| {
            p[0].abs() > 360. || !(0.0..=16.0).contains(&p[1]) || !(0.0..=16.0).contains(&p[2])
        }) {
            return Err(bad("invalid hue/saturation/value correction"));
        }
        let encoding = values(encoding_tag)?.first().copied().unwrap_or(0.);
        if encoding != 0. && encoding != 1. {
            return Err(bad("unknown table encoding"));
        }
        Ok(Some(Table {
            dims,
            data: data
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
            srgb_encoding: encoding == 1.,
        }))
    };
    let name = text(50936)?;
    if name.is_empty() {
        return Err(bad("missing profile name"));
    }
    Ok(Profile {
        name,
        camera: text(50708)?,
        digest: Sha256::digest(bytes).into(),
        illuminants,
        matrices,
        forward,
        curve,
        maps: [table(50937, 50938, 51107)?, table(50937, 50939, 51107)?],
        look: table(50981, 50982, 51108)?,
    })
}
fn hsv(p: [f32; 3]) -> [f32; 3] {
    let max = p.into_iter().fold(0., f32::max);
    let min = p.into_iter().fold(f32::INFINITY, f32::min);
    let d = max - min;
    let h = if d <= 1e-8 {
        0.
    } else if max == p[0] {
        ((p[1] - p[2]) / d).rem_euclid(6.)
    } else if max == p[1] {
        (p[2] - p[0]) / d + 2.
    } else {
        (p[0] - p[1]) / d + 4.
    };
    [h * 60., if max > 0. { d / max } else { 0. }, max]
}
fn rgb(p: [f32; 3]) -> [f32; 3] {
    let h = p[0].rem_euclid(360.) / 60.;
    let c = p[1] * p[2];
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = p[2] - c;
    let v = match h as usize {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    v.map(|v| v + m)
}
fn encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
impl Table {
    fn correction(&self, p: [f32; 3]) -> [f32; 3] {
        let [h, s, v] = hsv(p);
        let position = [
            h / 360. * self.dims[0] as f32,
            s.clamp(0., 1.) * (self.dims[1] - 1) as f32,
            (if self.srgb_encoding {
                encode(v.max(0.))
            } else {
                v
            })
            .clamp(0., 1.)
                * (self.dims[2] - 1) as f32,
        ];
        let base = position.map(|v| v.floor() as usize);
        let fraction = std::array::from_fn::<_, 3, _>(|i| position[i] - base[i] as f32);
        let mut result = [0.; 3];
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let k = [
                        (base[0] + x) % self.dims[0],
                        (base[1] + y).min(self.dims[1] - 1),
                        (base[2] + z).min(self.dims[2] - 1),
                    ];
                    let weight = [x, y, z]
                        .into_iter()
                        .enumerate()
                        .map(|(i, n)| {
                            if n == 0 {
                                1. - fraction[i]
                            } else {
                                fraction[i]
                            }
                        })
                        .product::<f32>();
                    let value = self.data[(k[2] * self.dims[0] + k[0]) * self.dims[1] + k[1]];
                    for c in 0..3 {
                        result[c] += value[c] * weight;
                    }
                }
            }
        }
        result
    }
    fn apply(&self, p: [f32; 3], correction: [f32; 3]) -> [f32; 3] {
        let mut h = hsv(p);
        h[0] += correction[0];
        h[1] = (h[1] * correction[1]).clamp(0., 1.);
        h[2] = if self.srgb_encoding {
            decode((encode(h[2].max(0.)) * correction[2]).clamp(0., 1.))
        } else {
            (h[2] * correction[2]).clamp(0., 1.)
        };
        rgb(h)
    }
}
/// Apply table operations in linear ProPhoto RGB, using floating-point ICC
/// transforms to avoid clipping to sRGB before the profile operation.
pub fn apply(profile: &Profile, pixels: &mut [[f32; 3]], kelvin: Option<f32>) -> Result<()> {
    if profile.maps.iter().all(Option::is_none)
        && profile.look.is_none()
        && profile.curve.is_empty()
    {
        return Ok(());
    }
    let mut converted = pixels.to_vec();
    crate::photo_color::convert_float(
        &mut converted,
        crate::photo_color::Space::Srgb,
        crate::photo_color::Space::ProPhoto,
    )?;
    let blend = profile.blend(kelvin);
    for pixel in &mut converted {
        let mut p = *pixel;
        if let Some(first) = &profile.maps[0] {
            let a = first.correction(p);
            let b = profile.maps[1]
                .as_ref()
                .map(|t| t.correction(p))
                .unwrap_or(a);
            p = first.apply(
                p,
                std::array::from_fn(|i| a[i] * (1. - blend) + b[i] * blend),
            );
        }
        if let Some(table) = &profile.look {
            p = table.apply(p, table.correction(p));
        }
        if profile.curve.len() >= 2 {
            p = p.map(|v| {
                let x = v.clamp(0., 1.);
                let i = profile
                    .curve
                    .partition_point(|p| p[0] < x)
                    .clamp(1, profile.curve.len() - 1);
                let a = profile.curve[i - 1];
                let b = profile.curve[i];
                a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0])
            });
        }
        *pixel = p;
    }
    crate::photo_color::convert_float(
        &mut converted,
        crate::photo_color::Space::ProPhoto,
        crate::photo_color::Space::Srgb,
    )?;
    pixels.copy_from_slice(&converted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut tags = vec![
            (50936u16, 2u16, b"Test camera profile\0".to_vec()),
            (50708, 2, b"Test Camera\0".to_vec()),
            (
                50721,
                11,
                [1f32, 0., 0., 0., 1., 0., 0., 0., 1.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            ),
            (50778, 3, 21u16.to_le_bytes().to_vec()),
        ];
        tags.sort_by_key(|t| t.0);
        let mut bytes = vec![b'I', b'I', 0x52, 0x43, 8, 0, 0, 0];
        bytes.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        let mut offset = 8 + 2 + tags.len() * 12 + 4;
        let mut payload = vec![];
        for (tag, kind, data) in tags {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            let size = if kind == 11 {
                4
            } else if kind == 3 {
                2
            } else {
                1
            };
            bytes.extend_from_slice(&((data.len() / size) as u32).to_le_bytes());
            if data.len() <= 4 {
                let mut v = [0; 4];
                v[..data.len()].copy_from_slice(&data);
                bytes.extend(v);
            } else {
                bytes.extend_from_slice(&(offset as u32).to_le_bytes());
                offset += data.len();
                payload.extend(data);
            }
        }
        bytes.extend([0; 4]);
        bytes.extend(payload);
        bytes
    }
    #[test]
    fn bounded_reader_checks_identity_and_offsets() {
        let bytes = fixture();
        let p = parse(&bytes).unwrap();
        assert_eq!(p.name, "Test camera profile");
        assert!(p.compatible("Test", "Camera"));
        assert!(!p.compatible("Other", "Camera"));
        assert_eq!(p.matrices[0].len(), 9);
        for size in 0..bytes.len() {
            assert!(parse(&bytes[..size]).is_err(), "truncation {size}");
        }
        let mut invalid = bytes.clone();
        invalid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&invalid).is_err());
    }
    #[test]
    fn neutral_lookup_is_identity_and_hue_wrap_is_continuous() {
        let table = Table {
            dims: [2, 2, 1],
            data: vec![[0., 1., 1.]; 4],
            srgb_encoding: false,
        };
        for p in [[0.2, 0.4, 0.1], [0.1, 0.1, 0.1], [0.8, 0.1, 0.5]] {
            let out = table.apply(p, table.correction(p));
            for c in 0..3 {
                assert!((out[c] - p[c]).abs() < 1e-6);
            }
        }
    }
}
