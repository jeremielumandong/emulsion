//! Lens profiles from the lensfun database: fetch the XML files on demand,
//! find the camera and lens a picture was taken with, and interpolate the
//! distortion, vignetting and chromatic-aberration coefficients for its
//! focal length and aperture. The maths lives in `emulsion-filters`
//! (`Filter::LensProfile`); this module only produces the numbers.
//!
//! Coordinates follow lensfun/PanoTools: a radius of 1 is half the shorter
//! side of the sensor the lens was calibrated on. Calibrations made on
//! another sensor size are rescaled through the crop-factor ratio.

use crate::{IoError, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

const DB_BASE: &str = "https://raw.githubusercontent.com/lensfun/lensfun/master/data/db/";

/// The database files (about 5 MB together), CC-BY-SA 3.0.
pub const DB_FILES: &[&str] = &[
    "6x6.xml",
    "actioncams.xml",
    "compact-canon.xml",
    "compact-casio.xml",
    "compact-fujifilm.xml",
    "compact-kodak.xml",
    "compact-konica-minolta.xml",
    "compact-leica.xml",
    "compact-nikon.xml",
    "compact-olympus.xml",
    "compact-panasonic.xml",
    "compact-pentax.xml",
    "compact-ricoh.xml",
    "compact-samsung.xml",
    "compact-sigma.xml",
    "compact-sony.xml",
    "contax.xml",
    "generic.xml",
    "mil-canon.xml",
    "mil-fujifilm.xml",
    "mil-hasselblad.xml",
    "mil-leica.xml",
    "mil-nikon.xml",
    "mil-olympus.xml",
    "mil-panasonic.xml",
    "mil-pentax.xml",
    "mil-samsung.xml",
    "mil-samyang.xml",
    "mil-sigma.xml",
    "mil-sony.xml",
    "mil-tamron.xml",
    "mil-tokina.xml",
    "mil-zeiss.xml",
    "misc.xml",
    "om-system.xml",
    "rf-leica.xml",
    "slr-canon.xml",
    "slr-hasselblad.xml",
    "slr-konica-minolta.xml",
    "slr-leica.xml",
    "slr-nikon.xml",
    "slr-olympus.xml",
    "slr-panasonic.xml",
    "slr-pentax.xml",
    "slr-ricoh.xml",
    "slr-samsung.xml",
    "slr-samyang.xml",
    "slr-schneider.xml",
    "slr-sigma.xml",
    "slr-soligor.xml",
    "slr-sony.xml",
    "slr-tamron.xml",
    "slr-tokina.xml",
    "slr-ussr.xml",
    "slr-vivitar.xml",
    "slr-zeiss.xml",
];

pub fn db_dir() -> PathBuf {
    crate::recent::data_dir().join("lensfun")
}

pub fn installed() -> bool {
    let d = db_dir();
    DB_FILES.iter().filter(|f| d.join(f).is_file()).count() >= DB_FILES.len() / 2
}

/// Fetch the database. `progress(done, total)` counts files.
pub fn install(progress: &(dyn Fn(usize, usize) + Sync), cancel: &AtomicBool) -> Result<()> {
    let d = db_dir();
    std::fs::create_dir_all(&d)?;
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build(),
    );
    for (i, f) in DB_FILES.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Unsupported("cancelled".into()));
        }
        let dest = d.join(f);
        if !dest.is_file() {
            let body = agent
                .get(&format!("{DB_BASE}{f}"))
                .call()
                .map_err(|e| IoError::Unsupported(format!("{f}: {e}")))?
                .into_body()
                .read_to_string()
                .map_err(|e| IoError::Unsupported(format!("{f}: {e}")))?;
            std::fs::write(&dest, body)?;
        }
        progress(i + 1, DB_FILES.len());
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Camera {
    pub maker: String,
    pub model: String,
    pub variant: String,
    pub mount: String,
    pub cropfactor: f32,
}

/// One calibration row at a focal length (and aperture/distance for
/// vignetting).
#[derive(Clone, Debug, Default)]
pub struct Distortion {
    pub focal: f32,
    /// "ptlens": a b c; "poly3": k1; "poly5": k1 k2.
    pub model: String,
    pub params: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Tca {
    pub focal: f32,
    /// poly3: br cr vr bb cb vb.
    pub params: [f32; 6],
}

#[derive(Clone, Debug, Default)]
pub struct Vignetting {
    pub focal: f32,
    pub aperture: f32,
    pub distance: f32,
    pub k: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Lens {
    pub maker: String,
    pub model: String,
    pub mounts: Vec<String>,
    pub cropfactor: f32,
    pub aspect_ratio: f32,
    pub distortion: Vec<Distortion>,
    pub tca: Vec<Tca>,
    pub vignetting: Vec<Vignetting>,
}

#[derive(Default)]
pub struct Database {
    pub cameras: Vec<Camera>,
    pub lenses: Vec<Lens>,
}

fn text(e: &quick_xml::events::BytesText) -> String {
    e.xml_content(quick_xml::XmlVersion::Implicit1_0)
        .trim()
        .to_string()
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.as_ref() == key).then(|| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.trim().to_string())
                .unwrap_or_default()
        })
    })
}

fn num(s: Option<String>) -> f32 {
    s.and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0)
}

fn parse_aspect(s: &str) -> f32 {
    if let Some((a, b)) = s.split_once(':') {
        let (a, b) = (
            a.trim().parse::<f32>().unwrap_or(3.0),
            b.trim().parse::<f32>().unwrap_or(2.0),
        );
        if b > 0.0 {
            return a / b;
        }
    }
    s.parse::<f32>().unwrap_or(1.5)
}

impl Database {
    /// Parse one lensfun XML file into the database.
    pub fn parse_into(&mut self, xml: &str) -> Result<()> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut cam: Option<Camera> = None;
        let mut lens: Option<Lens> = None;
        let mut field = String::new();
        let mut lang_ok = true;
        loop {
            let ev = reader
                .read_event()
                .map_err(|e| IoError::Xml(e.to_string()))?;
            match ev {
                Event::Eof => break,
                Event::Start(e) => {
                    let name = e.local_name().as_ref().to_string();
                    // Only the untranslated name of a maker/model counts.
                    lang_ok = attr(&e, "lang").is_none();
                    match name.as_str() {
                        "camera" => cam = Some(Camera::default()),
                        "lens" => {
                            lens = Some(Lens {
                                cropfactor: 1.0,
                                aspect_ratio: 1.5,
                                ..Default::default()
                            })
                        }
                        _ => field = name,
                    }
                }
                Event::Empty(e) => {
                    let name = e.local_name().as_ref().to_string();
                    if let Some(l) = &mut lens {
                        match name.as_str() {
                            "distortion" => {
                                let model = attr(&e, "model").unwrap_or_default();
                                let params = match model.as_str() {
                                    "ptlens" => {
                                        [num(attr(&e, "a")), num(attr(&e, "b")), num(attr(&e, "c"))]
                                    }
                                    "poly5" => [num(attr(&e, "k1")), num(attr(&e, "k2")), 0.0],
                                    _ => [num(attr(&e, "k1")), 0.0, 0.0],
                                };
                                l.distortion.push(Distortion {
                                    focal: num(attr(&e, "focal")),
                                    model,
                                    params,
                                });
                            }
                            "tca" => {
                                let poly3 = attr(&e, "model").as_deref() == Some("poly3");
                                let params = if poly3 {
                                    [
                                        num(attr(&e, "br")),
                                        num(attr(&e, "cr")),
                                        attr(&e, "vr").and_then(|v| v.parse().ok()).unwrap_or(1.0),
                                        num(attr(&e, "bb")),
                                        num(attr(&e, "cb")),
                                        attr(&e, "vb").and_then(|v| v.parse().ok()).unwrap_or(1.0),
                                    ]
                                } else {
                                    [
                                        0.0,
                                        0.0,
                                        attr(&e, "kr").and_then(|v| v.parse().ok()).unwrap_or(1.0),
                                        0.0,
                                        0.0,
                                        attr(&e, "kb").and_then(|v| v.parse().ok()).unwrap_or(1.0),
                                    ]
                                };
                                l.tca.push(Tca {
                                    focal: num(attr(&e, "focal")),
                                    params,
                                });
                            }
                            "vignetting" => {
                                l.vignetting.push(Vignetting {
                                    focal: num(attr(&e, "focal")),
                                    aperture: num(attr(&e, "aperture")),
                                    distance: attr(&e, "distance")
                                        .and_then(|v| v.parse().ok())
                                        .unwrap_or(1000.0),
                                    k: [
                                        num(attr(&e, "k1")),
                                        num(attr(&e, "k2")),
                                        num(attr(&e, "k3")),
                                    ],
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Event::Text(t) => {
                    if !lang_ok {
                        continue;
                    }
                    let v = text(&t);
                    if let Some(c) = &mut cam {
                        match field.as_str() {
                            "maker" => c.maker = v,
                            "model" => c.model = v,
                            "variant" => c.variant = v,
                            "mount" => c.mount = v,
                            "cropfactor" => c.cropfactor = v.parse().unwrap_or(1.0),
                            _ => {}
                        }
                    } else if let Some(l) = &mut lens {
                        match field.as_str() {
                            "maker" => l.maker = v,
                            "model" => l.model = v,
                            "mount" => l.mounts.push(v),
                            "cropfactor" => l.cropfactor = v.parse().unwrap_or(1.0),
                            "aspect-ratio" => l.aspect_ratio = parse_aspect(&v),
                            _ => {}
                        }
                    }
                }
                Event::End(e) => {
                    let name = e.local_name().as_ref().to_string();
                    match name.as_str() {
                        "camera" => {
                            if let Some(c) = cam.take() {
                                self.cameras.push(c);
                            }
                        }
                        "lens" => {
                            if let Some(l) = lens.take() {
                                self.lenses.push(l);
                            }
                        }
                        _ => {}
                    }
                    field.clear();
                    lang_ok = true;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Load every installed file.
    pub fn load() -> Result<Database> {
        let mut db = Database::default();
        let d = db_dir();
        for f in DB_FILES {
            if let Ok(xml) = std::fs::read_to_string(d.join(f)) {
                db.parse_into(&xml)?;
            }
        }
        if db.lenses.is_empty() {
            return Err(IoError::Unsupported("lens database not installed".into()));
        }
        Ok(db)
    }

    /// The camera whose maker and model best match EXIF strings.
    pub fn find_camera(&self, make: &str, model: &str) -> Option<&Camera> {
        let (mk, md) = (norm(make), norm(model));
        let mut best: Option<(usize, &Camera)> = None;
        for c in &self.cameras {
            let cm = norm(&c.model);
            if !norm(&c.maker)
                .split(' ')
                .any(|w| !w.is_empty() && mk.contains(w))
                && !mk.is_empty()
            {
                continue;
            }
            let score = if cm == md {
                100
            } else if !cm.is_empty() && (md.contains(&cm) || cm.contains(&md)) {
                60 + cm.len().min(30)
            } else {
                0
            };
            if score > 0 && best.is_none_or(|(s, _)| score > s) {
                best = Some((score, c));
            }
        }
        best.map(|(_, c)| c)
    }

    /// The lens best matching an EXIF lens description, preferring ones
    /// that fit the camera's mount and whose focal range covers `focal`.
    pub fn find_lens(&self, lens_desc: &str, camera: Option<&Camera>, focal: f32) -> Option<&Lens> {
        let want = norm(lens_desc);
        let want_words: Vec<&str> = want.split(' ').filter(|w| w.len() > 1).collect();
        if want_words.is_empty() {
            return None;
        }
        // Makers write "XF35mmF1.4 R" where the database has "XF 35mm F1.4 R":
        // compare with spaces and dashes removed as well.
        let compact = |s: &str| s.replace([' ', '-'], "");
        let want_compact = compact(&want);
        let mut best: Option<(i32, &Lens)> = None;
        for l in &self.lenses {
            let have = norm(&format!("{} {}", l.maker, l.model));
            let have_words: Vec<&str> = have.split(' ').collect();
            let word_hits = want_words.iter().filter(|w| have_words.contains(w)).count() as i32;
            // Focal-length tokens like "24-70mm" or "50mm" must match whole.
            let digit_hits = want_words
                .iter()
                .filter(|w| w.chars().any(|c| c.is_ascii_digit()) && have_words.contains(w))
                .count() as i32;
            let have_compact = compact(&have);
            let model_compact = compact(&norm(&l.model));
            let compact_hit = !want_compact.is_empty()
                && model_compact.len() >= 6
                && (have_compact.contains(&want_compact) || want_compact.contains(&model_compact));
            // A lens is a candidate when the whole name lines up, or the
            // focal range plus another word does, or three words do.
            if !(compact_hit || (word_hits >= 2 && digit_hits >= 1) || word_hits >= 3) {
                continue;
            }
            let mut score = word_hits * 10 + digit_hits * 15 + if compact_hit { 60 } else { 0 };
            if let Some(c) = camera {
                if l.mounts.iter().any(|m| m == &c.mount) {
                    score += 20;
                } else if !l.mounts.is_empty() {
                    // Wrong mount is not a match, however alike the names.
                    continue;
                }
            }
            if focal > 0.0 && !l.distortion.is_empty() {
                let lo = l
                    .distortion
                    .iter()
                    .map(|d| d.focal)
                    .fold(f32::INFINITY, f32::min);
                let hi = l.distortion.iter().map(|d| d.focal).fold(0.0f32, f32::max);
                if focal < lo * 0.9 || focal > hi * 1.1 {
                    score -= 15;
                }
            }
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, l));
            }
        }
        best.filter(|(s, _)| *s >= 20).map(|(_, l)| l)
    }
}

/// Lower-case, punctuation removed, runs of spaces collapsed.
fn norm(s: &str) -> String {
    let mut out = String::new();
    let mut space = true;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '-' || c == '.' {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim().to_string()
}

/// Coefficients ready for `Filter::LensProfile`, in the picture's own
/// normalised radius (1 = half the shorter side).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub lens: String,
    pub camera: String,
    /// Distortion as ptlens a, b, c (poly3/poly5 are converted).
    pub distortion: Option<[f32; 3]>,
    /// Vignetting k1, k2, k3.
    pub vignetting: Option<[f32; 3]>,
    /// TCA poly3: br cr vr bb cb vb.
    pub tca: Option<[f32; 6]>,
    /// Calibration crop factor over the camera's: rescales the radius.
    pub scale: f32,
}

fn lerp_rows<T: Clone>(
    rows: &[T],
    focal: f32,
    key: impl Fn(&T) -> f32,
    mix: impl Fn(&T, &T, f32) -> T,
) -> Option<T> {
    if rows.is_empty() {
        return None;
    }
    let mut sorted: Vec<&T> = rows.iter().collect();
    sorted.sort_by(|a, b| key(a).total_cmp(&key(b)));
    if focal <= key(sorted[0]) {
        return Some(sorted[0].clone());
    }
    for w in sorted.windows(2) {
        let (a, b) = (w[0], w[1]);
        if focal <= key(b) {
            let t = ((focal - key(a)) / (key(b) - key(a)).max(1e-6)).clamp(0.0, 1.0);
            return Some(mix(a, b, t));
        }
    }
    sorted.last().map(|l| (*l).clone())
}

fn mixf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Lens {
    /// Coefficients for a picture at `focal` mm and `aperture`, taken on a
    /// camera with `cam_crop`.
    pub fn profile(&self, focal: f32, aperture: f32, cam_crop: f32) -> Profile {
        let distortion = lerp_rows(
            &self.distortion,
            focal,
            |d| d.focal,
            |a, b, t| Distortion {
                focal,
                model: a.model.clone(),
                params: [
                    mixf(a.params[0], b.params[0], t),
                    mixf(a.params[1], b.params[1], t),
                    mixf(a.params[2], b.params[2], t),
                ],
            },
        )
        .map(|d| match d.model.as_str() {
            "ptlens" => d.params,
            // poly3: r_d = r_u (1 - k1 + k1 r²) ≡ ptlens with b = k1.
            "poly3" => [0.0, d.params[0], 0.0],
            // poly5 has no exact ptlens form; its r² term maps to b, the r⁴ term is folded into a.
            _ => [d.params[1], d.params[0], 0.0],
        });
        let tca = lerp_rows(
            &self.tca,
            focal,
            |t| t.focal,
            |a, b, t| Tca {
                focal,
                params: std::array::from_fn(|i| mixf(a.params[i], b.params[i], t)),
            },
        )
        .map(|t| t.params);
        // Vignetting: nearest aperture rows, interpolated over focal.
        let vignetting = if self.vignetting.is_empty() {
            None
        } else {
            let ap = if aperture > 0.0 { aperture } else { 8.0 };
            let mut rows: Vec<Vignetting> = self.vignetting.clone();
            rows.sort_by(|a, b| (a.aperture - ap).abs().total_cmp(&(b.aperture - ap).abs()));
            let best_ap = rows[0].aperture;
            let at_ap: Vec<Vignetting> = rows
                .into_iter()
                .filter(|r| (r.aperture - best_ap).abs() < 0.01)
                .collect();
            lerp_rows(
                &at_ap,
                focal,
                |v| v.focal,
                |a, b, t| Vignetting {
                    focal,
                    aperture: a.aperture,
                    distance: a.distance,
                    k: std::array::from_fn(|i| mixf(a.k[i], b.k[i], t)),
                },
            )
            .map(|v| v.k)
        };
        let scale = if cam_crop > 0.0 && self.cropfactor > 0.0 {
            self.cropfactor / cam_crop
        } else {
            1.0
        };
        Profile {
            lens: format!("{} {}", self.maker, self.model).trim().to_string(),
            camera: String::new(),
            distortion,
            vignetting,
            tca,
            scale,
        }
    }
}

/// Everything from EXIF to coefficients in one step.
pub fn profile_for(
    db: &Database,
    make: &str,
    model: &str,
    lens: &str,
    focal: f32,
    aperture: f32,
) -> Option<Profile> {
    let cam = db.find_camera(make, model);
    let l = db.find_lens(lens, cam, focal)?;
    let crop = cam.map(|c| c.cropfactor).unwrap_or(l.cropfactor);
    let mut p = l.profile(focal, aperture, crop);
    p.camera = cam
        .map(|c| format!("{} {}", c.maker, c.model))
        .unwrap_or_default();
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<lensdatabase version="2">
  <camera><maker>Sony</maker><model>ILCE-7M3</model><model lang="en">A7 III</model><mount>Sony E</mount><cropfactor>1.0</cropfactor></camera>
  <camera><maker>Fujifilm</maker><model>X-T4</model><mount>Fujifilm X</mount><cropfactor>1.5</cropfactor></camera>
  <lens>
    <maker>Sony</maker><model>FE 24-70mm F2.8 GM</model><mount>Sony E</mount><cropfactor>1.0</cropfactor><aspect-ratio>3:2</aspect-ratio>
    <calibration>
      <distortion model="ptlens" focal="24" a="0.01" b="-0.03" c="0.005"/>
      <distortion model="ptlens" focal="70" a="0.0" b="0.01" c="0.0"/>
      <tca model="poly3" focal="24" br="0" cr="0" vr="1.0002" bb="0" cb="0" vb="0.9998"/>
      <vignetting model="pa" focal="24" aperture="2.8" distance="1000" k1="-0.6" k2="0.2" k3="-0.05"/>
      <vignetting model="pa" focal="24" aperture="8" distance="1000" k1="-0.2" k2="0.05" k3="0"/>
    </calibration>
  </lens>
  <lens>
    <maker>Fujifilm</maker><model>XF 35mm F1.4 R</model><mount>Fujifilm X</mount><cropfactor>1.5</cropfactor>
    <calibration><distortion model="poly3" focal="35" k1="-0.004"/></calibration>
  </lens>
</lensdatabase>"#;

    #[test]
    fn parses_matches_and_interpolates() {
        let mut db = Database::default();
        db.parse_into(XML).unwrap();
        assert_eq!(db.cameras.len(), 2);
        assert_eq!(db.lenses.len(), 2);
        let cam = db.find_camera("SONY", "ILCE-7M3").unwrap();
        assert_eq!(cam.mount, "Sony E");
        let names: Vec<String> = db
            .lenses
            .iter()
            .map(|l| {
                format!(
                    "{}|{}|{:?}|{}",
                    l.maker,
                    l.model,
                    l.mounts,
                    l.distortion.len()
                )
            })
            .collect();
        assert!(
            db.find_lens("FE 24-70mm F2.8 GM", Some(cam), 47.0)
                .is_some(),
            "lenses: {names:?}"
        );
        let p = profile_for(&db, "SONY", "ILCE-7M3", "FE 24-70mm F2.8 GM", 47.0, 2.8).unwrap();
        assert!(p.lens.contains("24-70"));
        let d = p.distortion.unwrap();
        assert!(
            (d[0] - 0.005).abs() < 1e-4 && (d[1] - (-0.01)).abs() < 1e-4,
            "{d:?}"
        );
        assert!(
            (p.vignetting.unwrap()[0] - (-0.6)).abs() < 1e-6,
            "aperture 2.8 row"
        );
        assert_eq!(p.scale, 1.0);
        let f = profile_for(&db, "FUJIFILM", "X-T4", "XF35mmF1.4 R", 35.0, 4.0).unwrap();
        assert_eq!(f.distortion.unwrap(), [0.0, -0.004, 0.0]);
        assert!(profile_for(&db, "Canon", "EOS R5", "RF 50mm", 50.0, 2.0).is_none());
        // A phone's own description shares words with real lenses ("pro",
        // digits) without being one.
        assert!(
            profile_for(
                &db,
                "Google",
                "Pixel 10 Pro XL",
                "Pixel 10 Pro XL back camera 6.9mm f/1.68",
                6.9,
                1.7
            )
            .is_none()
        );
        // The wrong mount is not a match even with the right name.
        assert!(profile_for(&db, "FUJIFILM", "X-T4", "FE 24-70mm F2.8 GM", 47.0, 2.8).is_none());
    }
}
