//! Text to shot (SG6): an offline parser for shot descriptions such as
//! "low-angle close-up of two people at a table" and a scene builder that
//! places characters and props, picks poses and frames the camera.
//!
//! The vocabulary lives in the data tables below ([`vocabulary`]); matching
//! is greedy longest-phrase-first over normalized words, so "extreme close
//! up" wins over "close up" and "wide angle lens" over "wide".

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::error::SceneError;
use crate::import::AssetLibrary;
use crate::mannequin::{MannequinKind, MannequinParams};
use crate::pose::{FacePreset, PosePreset};
use crate::props::{BuiltinProp, PropKind};
use crate::scene::{Character, ObjectId, Prop, Scene};
use crate::shot::{CameraAngle, ShotSide, ShotSize, ShotSpec, frame_shot};
use crate::skeleton::Bone;

/// A recognized word or phrase.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Term {
    Size(ShotSize),
    Angle(CameraAngle),
    Side(ShotSide),
    Lens(f32),
    Count(u32),
    /// A person noun; `plural` nouns default to two people.
    Person {
        kind: MannequinKind,
        plural: bool,
    },
    /// "a couple": a man and a woman.
    Couple,
    /// "a family": a man, a woman and a child.
    Family,
    Pose(PosePreset),
    Face(FacePreset),
    Prop(PropKind),
    Setting(Setting),
    Part(Bone),
    /// "talking to", "facing": the characters face each other.
    Facing,
    /// "on the floor" (turns sitting into sitting on the floor).
    OnFloor,
    /// Words that carry no meaning here ("of", "the", "shot").
    Filler,
}

/// A location that brings set dressing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    Street,
    Room,
    Park,
}

/// The vocabulary tables: (phrase, meaning). Phrases are lower-case words
/// separated by single spaces, after the same normalization as the input.
pub mod vocabulary {
    use super::{Setting, Term};
    use crate::mannequin::MannequinKind as K;
    use crate::pose::{FacePreset as F, PosePreset as P};
    use crate::props::PropKind as R;
    use crate::shot::{CameraAngle as A, ShotSide as S, ShotSize as Z};
    use crate::skeleton::Bone;

    pub const SIZES: &[(&str, Z)] = &[
        ("extreme close up", Z::ExtremeCloseUp),
        ("extreme closeup", Z::ExtremeCloseUp),
        ("extreme cu", Z::ExtremeCloseUp),
        ("ecu", Z::ExtremeCloseUp),
        ("xcu", Z::ExtremeCloseUp),
        ("macro shot", Z::ExtremeCloseUp),
        ("medium close up", Z::MediumCloseUp),
        ("medium closeup", Z::MediumCloseUp),
        ("mcu", Z::MediumCloseUp),
        ("chest shot", Z::MediumCloseUp),
        ("close up", Z::CloseUp),
        ("closeup", Z::CloseUp),
        ("close shot", Z::CloseUp),
        ("tight shot", Z::CloseUp),
        ("cu", Z::CloseUp),
        ("head shot", Z::CloseUp),
        ("headshot", Z::CloseUp),
        ("medium wide shot", Z::MediumWide),
        ("medium wide", Z::MediumWide),
        ("medium long shot", Z::MediumWide),
        ("cowboy shot", Z::MediumWide),
        ("cowboy", Z::MediumWide),
        ("knee shot", Z::MediumWide),
        ("mws", Z::MediumWide),
        ("mls", Z::MediumWide),
        ("medium shot", Z::Medium),
        ("mid shot", Z::Medium),
        ("waist shot", Z::Medium),
        ("medium", Z::Medium),
        ("ms", Z::Medium),
        ("extreme wide shot", Z::ExtremeWide),
        ("extreme wide", Z::ExtremeWide),
        ("extreme long shot", Z::ExtremeWide),
        ("establishing shot", Z::ExtremeWide),
        ("ews", Z::ExtremeWide),
        ("els", Z::ExtremeWide),
        ("wide shot", Z::Wide),
        ("long shot", Z::Wide),
        ("full shot", Z::Wide),
        ("full body", Z::Wide),
        ("full length", Z::Wide),
        ("wide", Z::Wide),
        ("ws", Z::Wide),
        ("ls", Z::Wide),
    ];

    pub const ANGLES: &[(&str, A)] = &[
        ("eye level", A::EyeLevel),
        ("straight on", A::EyeLevel),
        ("low angle", A::Low),
        ("from below", A::Low),
        ("looking up at", A::Low),
        ("high angle", A::High),
        ("from above", A::High),
        ("looking down at", A::High),
        ("bird's eye view", A::BirdsEye),
        ("bird's eye", A::BirdsEye),
        ("birds eye view", A::BirdsEye),
        ("birds eye", A::BirdsEye),
        ("bird eye", A::BirdsEye),
        ("overhead", A::BirdsEye),
        ("top down", A::BirdsEye),
        ("aerial", A::BirdsEye),
        ("worm's eye view", A::WormsEye),
        ("worm's eye", A::WormsEye),
        ("worms eye view", A::WormsEye),
        ("worms eye", A::WormsEye),
        ("from the ground", A::WormsEye),
        ("dutch angle", A::Dutch),
        ("dutch tilt", A::Dutch),
        ("dutch", A::Dutch),
        ("canted", A::Dutch),
        ("tilted", A::Dutch),
        ("over the shoulder", A::OverTheShoulder),
        ("over shoulder", A::OverTheShoulder),
        ("ots", A::OverTheShoulder),
        ("two shot", A::TwoShot),
        ("2 shot", A::TwoShot),
    ];

    pub const SIDES: &[(&str, S)] = &[
        ("from behind", S::Back),
        ("from the back", S::Back),
        ("back view", S::Back),
        ("rear view", S::Back),
        ("in profile", S::Left),
        ("profile", S::Left),
        ("side view", S::Left),
        ("from the side", S::Left),
        ("three quarter", S::FrontLeft),
        ("3/4", S::FrontLeft),
        ("front view", S::Front),
        ("from the front", S::Front),
        ("frontal", S::Front),
    ];

    pub const LENSES: &[(&str, f32)] = &[
        ("wide angle lens", 18.0),
        ("wide angle", 18.0),
        ("wide lens", 18.0),
        ("fisheye", 10.0),
        ("fish eye", 10.0),
        ("telephoto lens", 135.0),
        ("telephoto", 135.0),
        ("long lens", 135.0),
        ("normal lens", 50.0),
        ("portrait lens", 85.0),
    ];

    pub const COUNTS: &[(&str, u32)] = &[
        ("a", 1),
        ("an", 1),
        ("one", 1),
        ("single", 1),
        ("lone", 1),
        ("two", 2),
        ("2", 2),
        ("pair of", 2),
        ("couple of", 2),
        ("three", 3),
        ("3", 3),
        ("four", 4),
        ("4", 4),
        ("five", 5),
        ("5", 5),
        ("six", 6),
        ("group of", 4),
        ("several", 3),
        ("crowd of", 6),
    ];

    /// (word, kind, plural)
    pub const PEOPLE: &[(&str, K, bool)] = &[
        ("man", K::AdultMale, false),
        ("men", K::AdultMale, true),
        ("guy", K::AdultMale, false),
        ("guys", K::AdultMale, true),
        ("gentleman", K::AdultMale, false),
        ("father", K::AdultMale, false),
        ("dad", K::AdultMale, false),
        ("husband", K::AdultMale, false),
        ("policeman", K::AdultMale, false),
        ("woman", K::AdultFemale, false),
        ("women", K::AdultFemale, true),
        ("lady", K::AdultFemale, false),
        ("ladies", K::AdultFemale, true),
        ("mother", K::AdultFemale, false),
        ("mom", K::AdultFemale, false),
        ("wife", K::AdultFemale, false),
        ("person", K::AdultNeutral, false),
        ("people", K::AdultNeutral, true),
        ("persons", K::AdultNeutral, true),
        ("figure", K::AdultNeutral, false),
        ("figures", K::AdultNeutral, true),
        ("someone", K::AdultNeutral, false),
        ("character", K::AdultNeutral, false),
        ("characters", K::AdultNeutral, true),
        ("friends", K::AdultNeutral, true),
        ("soldier", K::AdultNeutral, false),
        ("soldiers", K::AdultNeutral, true),
        ("detective", K::AdultNeutral, false),
        ("hero", K::AdultNeutral, false),
        ("villain", K::AdultNeutral, false),
        ("crowd", K::AdultNeutral, true),
        ("child", K::Child, false),
        ("children", K::Child, true),
        ("kid", K::Child, false),
        ("kids", K::Child, true),
        ("boy", K::Child, false),
        ("boys", K::Child, true),
        ("girl", K::Child, false),
        ("girls", K::Child, true),
    ];

    pub const POSES: &[(&str, P)] = &[
        ("running", P::Run),
        ("runs", P::Run),
        ("sprinting", P::Run),
        ("chasing", P::Run),
        ("fleeing", P::Run),
        ("walking", P::Walk),
        ("walks", P::Walk),
        ("strolling", P::Walk),
        ("sitting", P::Sit),
        ("sits", P::Sit),
        ("seated", P::Sit),
        ("kneeling", P::Kneel),
        ("kneels", P::Kneel),
        ("crouching", P::Crouch),
        ("crouched", P::Crouch),
        ("squatting", P::Crouch),
        ("hiding", P::Crouch),
        ("pointing", P::Point),
        ("points", P::Point),
        ("waving", P::Wave),
        ("waves", P::Wave),
        ("reaching", P::Reach),
        ("reaches", P::Reach),
        ("fighting", P::FightStance),
        ("punching", P::FightStance),
        ("boxing", P::FightStance),
        ("falling", P::Fall),
        ("falls", P::Fall),
        ("tripping", P::Fall),
        ("lying down", P::LieDown),
        ("lying", P::LieDown),
        ("sleeping", P::LieDown),
        ("asleep", P::LieDown),
        ("on the phone", P::PhoneCall),
        ("on a phone", P::PhoneCall),
        ("phone call", P::PhoneCall),
        ("calling", P::PhoneCall),
        ("carrying", P::Carry),
        ("arms crossed", P::ArmsCrossed),
        ("crossed arms", P::ArmsCrossed),
        ("arms folded", P::ArmsCrossed),
        ("hands on hips", P::HandsOnHips),
        ("standing", P::Stand),
        ("stands", P::Stand),
        ("waiting", P::RelaxedStand),
        ("talking", P::RelaxedStand),
        ("chatting", P::RelaxedStand),
        ("speaking", P::RelaxedStand),
        ("arguing", P::RelaxedStand),
        ("listening", P::RelaxedStand),
        ("looking", P::RelaxedStand),
    ];

    pub const FACES: &[(&str, F)] = &[
        ("happy", F::Happy),
        ("smiling", F::Happy),
        ("laughing", F::Happy),
        ("sad", F::Sad),
        ("crying", F::Sad),
        ("upset", F::Sad),
        ("angry", F::Angry),
        ("furious", F::Angry),
        ("shouting", F::Angry),
        ("yelling", F::Angry),
        ("surprised", F::Surprised),
        ("shocked", F::Surprised),
        ("amazed", F::Surprised),
        ("scared", F::Scared),
        ("afraid", F::Scared),
        ("terrified", F::Scared),
        ("frightened", F::Scared),
    ];

    pub const PROPS: &[(&str, R)] = &[
        ("table", R::Table),
        ("desk", R::Table),
        ("chair", R::Chair),
        ("stool", R::Chair),
        ("car", R::Car),
        ("cars", R::Car),
        ("truck", R::Car),
        ("taxi", R::Car),
        ("van", R::Car),
        ("tree", R::Tree),
        ("trees", R::Tree),
        ("door", R::Door),
        ("doorway", R::Door),
        ("window", R::WindowFrame),
        ("bed", R::Bed),
        ("sofa", R::Sofa),
        ("couch", R::Sofa),
        ("stairs", R::Stairs),
        ("staircase", R::Stairs),
        ("steps", R::Stairs),
        ("lamp post", R::LampPost),
        ("lamppost", R::LampPost),
        ("street lamp", R::LampPost),
        ("street light", R::LampPost),
        ("streetlight", R::LampPost),
        ("box", R::Box),
        ("crate", R::Box),
        ("boxes", R::Box),
        ("wall", R::Wall),
        ("ball", R::Sphere),
        ("barrel", R::Cylinder),
        ("pillar", R::Cylinder),
        ("column", R::Cylinder),
    ];

    pub const SETTINGS: &[(&str, Setting)] = &[
        ("street", Setting::Street),
        ("road", Setting::Street),
        ("sidewalk", Setting::Street),
        ("alley", Setting::Street),
        ("city", Setting::Street),
        ("room", Setting::Room),
        ("kitchen", Setting::Room),
        ("office", Setting::Room),
        ("bedroom", Setting::Room),
        ("living room", Setting::Room),
        ("bar", Setting::Room),
        ("restaurant", Setting::Room),
        ("cafe", Setting::Room),
        ("classroom", Setting::Room),
        ("hallway", Setting::Room),
        ("corridor", Setting::Room),
        ("park", Setting::Park),
        ("forest", Setting::Park),
        ("woods", Setting::Park),
        ("garden", Setting::Park),
        ("field", Setting::Park),
    ];

    pub const PARTS: &[(&str, Bone)] = &[
        ("hand", Bone::HandR),
        ("hands", Bone::HandR),
        ("fist", Bone::HandR),
        ("face", Bone::Head),
        ("eyes", Bone::Head),
        ("eye", Bone::Head),
        ("foot", Bone::FootR),
        ("feet", Bone::FootR),
        ("shoes", Bone::FootR),
    ];

    pub const OTHER: &[(&str, Term)] = &[
        ("talking to", Term::Facing),
        ("speaking to", Term::Facing),
        ("talks to", Term::Facing),
        ("arguing with", Term::Facing),
        ("facing", Term::Facing),
        ("looking at", Term::Facing),
        ("face to face", Term::Facing),
        ("each other", Term::Facing),
        ("on the floor", Term::OnFloor),
        ("on the ground", Term::OnFloor),
        ("couple", Term::Couple),
        ("family", Term::Family),
        ("of", Term::Filler),
        ("the", Term::Filler),
        ("at", Term::Filler),
        ("in", Term::Filler),
        ("on", Term::Filler),
        ("to", Term::Filler),
        ("with", Term::Filler),
        ("and", Term::Filler),
        ("is", Term::Filler),
        ("are", Term::Filler),
        ("down", Term::Filler),
        ("up", Term::Filler),
        ("along", Term::Filler),
        ("across", Term::Filler),
        ("near", Term::Filler),
        ("by", Term::Filler),
        ("next", Term::Filler),
        ("around", Term::Filler),
        ("inside", Term::Filler),
        ("through", Term::Filler),
        ("into", Term::Filler),
        ("shot", Term::Filler),
        ("view", Term::Filler),
        ("angle", Term::Filler),
        ("lens", Term::Filler),
        ("their", Term::Filler),
        ("his", Term::Filler),
        ("her", Term::Filler),
        ("each", Term::Filler),
        ("other", Term::Filler),
        ("while", Term::Filler),
        ("who", Term::Filler),
        ("from", Term::Filler),
        ("very", Term::Filler),
        ("big", Term::Filler),
        ("small", Term::Filler),
        ("old", Term::Filler),
        ("young", Term::Filler),
        ("tall", Term::Filler),
        ("red", Term::Filler),
        ("parked", Term::Filler),
    ];
}

/// Every phrase in the vocabulary with its meaning.
pub fn all_terms() -> Vec<(&'static str, Term)> {
    use vocabulary::*;
    let mut v: Vec<(&'static str, Term)> = Vec::new();
    v.extend(SIZES.iter().map(|(p, t)| (*p, Term::Size(*t))));
    v.extend(ANGLES.iter().map(|(p, t)| (*p, Term::Angle(*t))));
    v.extend(SIDES.iter().map(|(p, t)| (*p, Term::Side(*t))));
    v.extend(LENSES.iter().map(|(p, t)| (*p, Term::Lens(*t))));
    v.extend(COUNTS.iter().map(|(p, t)| (*p, Term::Count(*t))));
    v.extend(PEOPLE.iter().map(|(p, k, pl)| {
        (
            *p,
            Term::Person {
                kind: *k,
                plural: *pl,
            },
        )
    }));
    v.extend(POSES.iter().map(|(p, t)| (*p, Term::Pose(*t))));
    v.extend(FACES.iter().map(|(p, t)| (*p, Term::Face(*t))));
    v.extend(PROPS.iter().map(|(p, t)| (*p, Term::Prop(*t))));
    v.extend(SETTINGS.iter().map(|(p, t)| (*p, Term::Setting(*t))));
    v.extend(PARTS.iter().map(|(p, t)| (*p, Term::Part(*t))));
    v.extend(OTHER.iter().copied());
    v
}

/// A character the description asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectSpec {
    pub name: String,
    pub kind: MannequinKind,
    pub pose: Option<PosePreset>,
    pub face: FacePreset,
}

/// What the parser understood.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShotDescription {
    pub size: Option<ShotSize>,
    pub angle: Option<CameraAngle>,
    pub side: Option<ShotSide>,
    pub focal_length_mm: Option<f32>,
    pub subjects: Vec<SubjectSpec>,
    pub props: Vec<PropKind>,
    pub setting: Option<Setting>,
    pub focus: Option<Bone>,
    pub facing: bool,
    /// Words that matched nothing.
    pub unrecognized: Vec<String>,
}

fn normalize(text: &str) -> Vec<(String, bool)> {
    // (lower-case word, original was capitalized)
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '’' | '‘' => '\'',
            '-' | '–' | '—' | '_' | ',' | '.' | ';' | ':' | '!' | '?' | '"' | '(' | ')' => ' ',
            c => c,
        })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| {
            let cap = w.chars().next().is_some_and(char::is_uppercase);
            (w.to_lowercase(), cap)
        })
        .collect()
}

type AddPeople<'a> = dyn FnMut(&mut ShotDescription, MannequinKind, u32, &mut Option<usize>) + 'a;

/// Parses a shot description. Never fails; unknown words are listed in
/// [`ShotDescription::unrecognized`] and capitalized unknown words become
/// character names.
pub fn parse_shot(text: &str) -> ShotDescription {
    let words = normalize(text);
    let terms = all_terms();
    let phrases: Vec<(Vec<&str>, Term)> = terms
        .iter()
        .map(|(p, t)| (p.split(' ').collect(), *t))
        .collect();
    let mut d = ShotDescription::default();
    let mut pending_count: Option<u32> = None;
    // Index of the first subject in the most recent group.
    let mut group_start: Option<usize> = None;
    let mut pending_pose: Option<PosePreset> = None;
    let mut pending_face: Option<FacePreset> = None;
    let mut counter = std::collections::HashMap::<MannequinKind, u32>::new();
    let mut add_people =
        |d: &mut ShotDescription, kind: MannequinKind, n: u32, group_start: &mut Option<usize>| {
            *group_start = Some(d.subjects.len());
            for _ in 0..n.clamp(1, 12) {
                let c = counter.entry(kind).or_insert(0);
                *c += 1;
                let base = match kind {
                    MannequinKind::AdultMale => "Man",
                    MannequinKind::AdultFemale => "Woman",
                    MannequinKind::AdultNeutral => "Person",
                    MannequinKind::Child => "Child",
                };
                d.subjects.push(SubjectSpec {
                    name: format!("{base} {c}"),
                    kind,
                    pose: None,
                    face: FacePreset::Neutral,
                });
            }
        };
    // A count right after a group ("a family of four") resizes that group.
    let mut last_was_group = false;
    let mut trailing: Option<u32> = None;
    let resize = |d: &mut ShotDescription,
                  add_people: &mut AddPeople<'_>,
                  group_start: Option<usize>,
                  n: u32| {
        let Some(g) = group_start else { return };
        let n = n.clamp(1, 12) as usize;
        let len = d.subjects.len() - g;
        if n < len {
            d.subjects.truncate(g + n);
        } else if n > len {
            let kind = d.subjects.last().map(|s| s.kind).unwrap_or_default();
            let mut tmp = None;
            add_people(d, kind, (n - len) as u32, &mut tmp);
        }
    };
    let mut i = 0;
    while i < words.len() {
        // Lens like "35mm" or "35 mm".
        let w = words[i].0.as_str();
        if let Some(num) = w.strip_suffix("mm").and_then(|n| n.parse::<f32>().ok()) {
            d.focal_length_mm = Some(num);
            i += 1;
            continue;
        }
        if words.get(i + 1).is_some_and(|n| n.0 == "mm")
            && let Ok(num) = w.parse::<f32>()
        {
            d.focal_length_mm = Some(num);
            i += 2;
            continue;
        }
        let mut best: Option<(usize, Term)> = None;
        for (p, t) in &phrases {
            if p.len() > best.map_or(0, |b| b.0)
                && i + p.len() <= words.len()
                && p.iter().enumerate().all(|(k, pw)| words[i + k].0 == *pw)
            {
                best = Some((p.len(), *t));
            }
        }
        let Some((len, term)) = best else {
            let (word, cap) = &words[i];
            if *cap && word.chars().all(|c| c.is_alphabetic() || c == '\'') {
                let name = {
                    let mut c = word.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                };
                if let Some(n) = trailing.take() {
                    resize(&mut d, &mut add_people, group_start, n);
                }
                last_was_group = true;
                group_start = Some(d.subjects.len());
                d.subjects.push(SubjectSpec {
                    name,
                    kind: MannequinKind::AdultNeutral,
                    pose: None,
                    face: FacePreset::Neutral,
                });
                pending_count = None;
            } else {
                d.unrecognized.push(word.clone());
            }
            i += 1;
            continue;
        };
        let after_of = i > 0 && words[i - 1].0 == "of";
        i += len;
        let is_group = matches!(term, Term::Person { .. } | Term::Couple | Term::Family);
        if !matches!(term, Term::Filler | Term::Count(_)) {
            if let Some(n) = trailing.take()
                && !is_group
            {
                resize(&mut d, &mut add_people, group_start, n);
                pending_count = None;
            }
            last_was_group = is_group;
        }
        match term {
            Term::Size(s) => d.size = Some(s),
            Term::Angle(a) => d.angle = Some(a),
            Term::Side(s) => d.side = Some(s),
            Term::Lens(f) => d.focal_length_mm = Some(f),
            Term::Count(n) => {
                pending_count = Some(n);
                if last_was_group && after_of && trailing.is_none() {
                    trailing = Some(n);
                }
            }
            Term::Person { kind, plural } => {
                let n = pending_count.take().unwrap_or(if plural { 2 } else { 1 });
                add_people(&mut d, kind, n, &mut group_start);
            }
            Term::Couple => {
                pending_count = None;
                let start = d.subjects.len();
                add_people(&mut d, MannequinKind::AdultMale, 1, &mut group_start);
                add_people(&mut d, MannequinKind::AdultFemale, 1, &mut group_start);
                group_start = Some(start);
            }
            Term::Family => {
                pending_count = None;
                let start = d.subjects.len();
                add_people(&mut d, MannequinKind::AdultMale, 1, &mut group_start);
                add_people(&mut d, MannequinKind::AdultFemale, 1, &mut group_start);
                add_people(&mut d, MannequinKind::Child, 1, &mut group_start);
                group_start = Some(start);
            }
            Term::Pose(p) => {
                let refine = |old: Option<PosePreset>| match (old, p) {
                    // "talking" should not undo a stronger pose word.
                    (Some(o), PosePreset::RelaxedStand) => Some(o),
                    _ => Some(p),
                };
                match group_start {
                    Some(g) => {
                        for s in &mut d.subjects[g..] {
                            s.pose = refine(s.pose);
                        }
                    }
                    None => pending_pose = refine(pending_pose),
                }
            }
            Term::Face(f) => match group_start {
                Some(g) => d.subjects[g..].iter_mut().for_each(|s| s.face = f),
                None => pending_face = Some(f),
            },
            Term::Prop(p) => {
                pending_count = None;
                if d.props.len() < 16 {
                    d.props.push(p);
                }
            }
            Term::Setting(s) => d.setting = Some(s),
            Term::Part(b) => d.focus = Some(b),
            Term::Facing => {
                d.facing = true;
                // The next subject starts a new group.
                group_start = None;
            }
            Term::OnFloor => {
                for s in &mut d.subjects {
                    if s.pose == Some(PosePreset::Sit) {
                        s.pose = Some(PosePreset::SitOnFloor);
                    }
                }
                if pending_pose == Some(PosePreset::Sit) {
                    pending_pose = Some(PosePreset::SitOnFloor);
                }
            }
            Term::Filler => {}
        }
    }
    if let Some(n) = trailing {
        resize(&mut d, &mut add_people, group_start, n);
    }
    for s in &mut d.subjects {
        if s.pose.is_none() {
            s.pose = pending_pose;
        }
        if s.face == FacePreset::Neutral
            && let Some(f) = pending_face
        {
            s.face = f;
        }
    }
    if d.subjects.is_empty()
        && (pending_pose.is_some()
            || pending_face.is_some()
            || (d.props.is_empty() && d.setting.is_none())
            || d.focus.is_some())
    {
        d.subjects.push(SubjectSpec {
            name: "Person 1".into(),
            kind: MannequinKind::AdultNeutral,
            pose: pending_pose,
            face: pending_face.unwrap_or_default(),
        });
    }
    d
}

/// A generated shot.
#[derive(Debug, Clone)]
pub struct GeneratedShot {
    pub scene: Scene,
    pub description: ShotDescription,
    pub spec: ShotSpec,
    /// A one-line summary of what was understood.
    pub interpretation: String,
}

/// Parses `text` and builds a framed scene (`aspect` = board width / height).
pub fn text_to_shot(text: &str, aspect: f32) -> Result<GeneratedShot, SceneError> {
    let d = parse_shot(text);
    build_shot(&d, aspect)
}

/// Builds a scene for a parsed description.
pub fn build_shot(d: &ShotDescription, aspect: f32) -> Result<GeneratedShot, SceneError> {
    let mut scene = Scene::new();
    let mut d = d.clone();
    let angle = d.angle.unwrap_or(CameraAngle::EyeLevel);
    let at_table = d.props.contains(&PropKind::Table);
    // Over-the-shoulder and two-shots need a second character.
    if angle.needs_secondary() && d.subjects.len() == 1 {
        d.subjects.push(SubjectSpec {
            name: "Person 2".into(),
            kind: MannequinKind::AdultNeutral,
            pose: d.subjects[0].pose,
            face: FacePreset::Neutral,
        });
        d.facing = true;
    }
    let n = d.subjects.len();
    let facing = d.facing || (angle.needs_secondary() && n >= 2);
    let mut char_ids: Vec<ObjectId> = Vec::new();
    let mut prop_ids: Vec<ObjectId> = Vec::new();
    let mut remaining_props = d.props.clone();
    let take_prop = |list: &mut Vec<PropKind>, k: PropKind| -> bool {
        if let Some(i) = list.iter().position(|p| *p == k) {
            list.remove(i);
            true
        } else {
            false
        }
    };

    if at_table && n > 0 {
        take_prop(&mut remaining_props, PropKind::Table);
        let table = scene.add_prop("Table", Prop::builtin(PropKind::Table), Vec3::ZERO, 0.0);
        prop_ids.push(table);
        // Seats around the table: opposite sides first, then the ends.
        let seats = [
            (Vec3::new(0.0, 0.0, -0.75), 0.0),
            (Vec3::new(0.0, 0.0, 0.75), 180.0),
            (Vec3::new(-1.05, 0.0, 0.0), 90.0),
            (Vec3::new(1.05, 0.0, 0.0), -90.0),
            (Vec3::new(-0.45, 0.0, -0.75), 0.0),
            (Vec3::new(0.45, 0.0, 0.75), 180.0),
        ];
        for (k, s) in d.subjects.iter().enumerate() {
            let (pos, yaw) = seats
                .get(k)
                .copied()
                .unwrap_or((Vec3::new(k as f32 * 0.8 - 2.0, 0.0, -1.6), 0.0));
            let pose = s.pose.unwrap_or(PosePreset::Sit);
            if pose == PosePreset::Sit {
                take_prop(&mut remaining_props, PropKind::Chair);
                let back =
                    crate::math::yaw_pitch_roll_quat(yaw, 0.0, 0.0) * Vec3::new(0.0, 0.0, -0.12);
                let chair = scene.add_prop(
                    &format!("Chair {}", k + 1),
                    Prop::builtin(PropKind::Chair),
                    pos + back,
                    yaw,
                );
                prop_ids.push(chair);
            }
            char_ids.push(add_subject(&mut scene, s, pose, pos, yaw));
        }
    } else if facing && n >= 2 {
        let gap = 1.1;
        for (k, s) in d.subjects.iter().enumerate() {
            let pose = s.pose.unwrap_or(PosePreset::RelaxedStand);
            let (pos, yaw) = match k {
                0 => (Vec3::new(gap * 0.5, 0.0, 0.0), -90.0),
                1 => (Vec3::new(-gap * 0.5, 0.0, 0.0), 90.0),
                _ => (Vec3::new((k as f32 - 2.0) * 0.8, 0.0, -1.2), 0.0),
            };
            char_ids.push(add_subject(&mut scene, s, pose, pos, yaw));
        }
    } else {
        let spacing = 0.9;
        for (k, s) in d.subjects.iter().enumerate() {
            let pose = s.pose.unwrap_or(PosePreset::Stand);
            let x = (k as f32 - (n as f32 - 1.0) * 0.5) * spacing;
            let pos = Vec3::new(x, 0.0, 0.0);
            if pose == PosePreset::Sit {
                if take_prop(&mut remaining_props, PropKind::Sofa) {
                    let sofa = scene.add_prop(
                        "Sofa",
                        Prop::builtin(PropKind::Sofa),
                        Vec3::new(0.0, 0.0, -0.2),
                        0.0,
                    );
                    prop_ids.push(sofa);
                } else if !prop_ids
                    .iter()
                    .any(|id| scene.object(*id).is_some_and(|o| o.name == "Sofa"))
                {
                    take_prop(&mut remaining_props, PropKind::Chair);
                    let chair = scene.add_prop(
                        &format!("Chair {}", k + 1),
                        Prop::builtin(PropKind::Chair),
                        pos + Vec3::new(0.0, 0.0, -0.12),
                        0.0,
                    );
                    prop_ids.push(chair);
                }
            }
            if pose == PosePreset::LieDown && take_prop(&mut remaining_props, PropKind::Bed) {
                let bed = scene.add_prop("Bed", Prop::builtin(PropKind::Bed), pos, 0.0);
                prop_ids.push(bed);
                let top = BuiltinProp::new(PropKind::Bed).size().y * 0.53;
                let id = add_subject(&mut scene, s, pose, pos + Vec3::new(0.0, top, -0.25), 0.0);
                char_ids.push(id);
                continue;
            }
            char_ids.push(add_subject(&mut scene, s, pose, pos, 0.0));
        }
    }

    // Remaining props: the subject when there are no characters, otherwise
    // dressing beside and behind the characters.
    for (k, kind) in remaining_props.iter().enumerate() {
        let pos = if char_ids.is_empty() && k == 0 {
            Vec3::ZERO
        } else {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let size = BuiltinProp::new(*kind).size();
            let off = 1.0 + size.x * 0.5 + (k / 2) as f32 * 1.5;
            match kind {
                PropKind::Wall | PropKind::WindowFrame | PropKind::Door => {
                    Vec3::new(0.0, 0.0, -1.8)
                }
                _ => Vec3::new(side * off, 0.0, -0.6),
            }
        };
        let name = title(kind.name());
        let mut prop = BuiltinProp::new(*kind);
        if *kind == PropKind::WindowFrame {
            prop.size = Some(Vec3::new(1.2, 1.2, 0.1));
        }
        let id = scene.add_prop(
            &name,
            Prop::Builtin(prop),
            pos + if *kind == PropKind::WindowFrame {
                Vec3::new(0.0, 0.9, 0.0)
            } else {
                Vec3::ZERO
            },
            0.0,
        );
        prop_ids.push(id);
    }
    let dressing = dress_setting(&mut scene, d.setting);

    // Subject and framing.
    let subject = char_ids
        .first()
        .or(prop_ids.first())
        .or(dressing.first())
        .copied();
    let Some(subject) = subject else {
        return Err(SceneError::Invalid("nothing to frame".into()));
    };
    let secondary = char_ids.get(1).copied();
    let focus = d.focus.filter(|_| !char_ids.is_empty());
    let size = d.size.unwrap_or(if focus.is_some() {
        ShotSize::ExtremeCloseUp
    } else if char_ids.len() >= 3 || d.setting.is_some() || char_ids.is_empty() {
        ShotSize::Wide
    } else {
        ShotSize::Medium
    });
    let mut angle = angle;
    if d.angle.is_none() && char_ids.len() == 2 && facing && !at_table {
        angle = CameraAngle::TwoShot;
    }
    let side = d.side.unwrap_or(
        if at_table && char_ids.len() >= 2 && angle == CameraAngle::EyeLevel {
            ShotSide::FrontLeft
        } else if focus == Some(Bone::HandR) {
            // Profile from the right shows a raised right hand clearly.
            ShotSide::Right
        } else {
            ShotSide::Front
        },
    );
    if let Some(f) = focus
        && let Some(c) = scene.character_mut(subject)
    {
        // A detail shot of a hand reads better with the hand raised.
        if f == Bone::HandR && matches!(c.pose.name.as_str(), "stand" | "relaxed_stand") {
            c.pose = PosePreset::Point.pose();
            c.pose.right_hand = crate::pose::HandShape::Open;
        }
    }
    let spec = ShotSpec {
        size,
        angle,
        side,
        subject,
        secondary: if angle.needs_secondary() {
            secondary
        } else {
            None
        },
        focus,
        focal_length_mm: d.focal_length_mm,
        group: char_ids.len() >= 2 && size >= ShotSize::Medium && !at_table,
    };
    let camera = frame_shot(&scene, &AssetLibrary::new(), &spec, aspect)?;
    scene.apply_shot(spec, camera);
    let interpretation = interpretation(&d, &spec, char_ids.len());
    Ok(GeneratedShot {
        scene,
        description: d,
        spec,
        interpretation,
    })
}

fn add_subject(
    scene: &mut Scene,
    s: &SubjectSpec,
    pose: PosePreset,
    pos: Vec3,
    yaw: f32,
) -> ObjectId {
    let c = Character {
        body: MannequinParams::of(s.kind),
        face: s.face,
        ..Character::default().with_pose(pose)
    };
    scene.add_character(&s.name, c, pos, yaw)
}

/// Adds set dressing; returns the ids added (the first is the ground piece).
fn dress_setting(scene: &mut Scene, setting: Option<Setting>) -> Vec<ObjectId> {
    let before: std::collections::BTreeSet<ObjectId> = scene.objects.iter().map(|o| o.id).collect();
    match setting {
        Some(Setting::Street) => {
            scene.add_prop(
                "Road",
                Prop::Builtin(
                    BuiltinProp::new(PropKind::Floor).with_size(Vec3::new(7.0, 0.02, 60.0)),
                ),
                Vec3::ZERO,
                0.0,
            );
            for side in [-1.0f32, 1.0] {
                let wall = BuiltinProp::new(PropKind::Wall).with_size(Vec3::new(60.0, 7.0, 0.3));
                scene.add_prop(
                    if side < 0.0 {
                        "Buildings right"
                    } else {
                        "Buildings left"
                    },
                    Prop::Builtin(wall),
                    Vec3::new(side * 5.5, 0.0, 0.0),
                    90.0,
                );
                for k in 0..4 {
                    let z = -12.0 + k as f32 * 9.0;
                    scene.add_prop(
                        "Lamp post",
                        Prop::builtin(PropKind::LampPost),
                        Vec3::new(side * 3.8, 0.0, z),
                        if side > 0.0 { -90.0 } else { 90.0 },
                    );
                }
            }
        }
        Some(Setting::Room) => {
            scene.add_prop(
                "Floor",
                Prop::Builtin(
                    BuiltinProp::new(PropKind::Floor).with_size(Vec3::new(7.0, 0.02, 7.0)),
                ),
                Vec3::ZERO,
                0.0,
            );
            let back = BuiltinProp::new(PropKind::Wall).with_size(Vec3::new(7.0, 2.7, 0.15));
            scene.add_prop(
                "Back wall",
                Prop::Builtin(back),
                Vec3::new(0.0, 0.0, -3.5),
                0.0,
            );
            let side = BuiltinProp::new(PropKind::Wall).with_size(Vec3::new(7.0, 2.7, 0.15));
            scene.add_prop(
                "Side wall",
                Prop::Builtin(side),
                Vec3::new(-3.5, 0.0, 0.0),
                90.0,
            );
        }
        Some(Setting::Park) => {
            let spots = [
                (-4.0, -6.0),
                (3.5, -8.0),
                (-7.0, -2.0),
                (6.5, -3.5),
                (0.5, -12.0),
                (-2.5, -10.5),
            ];
            for (k, (x, z)) in spots.iter().enumerate() {
                let s = 0.8 + 0.1 * (k % 3) as f32;
                let tree =
                    BuiltinProp::new(PropKind::Tree).with_size(PropKind::Tree.default_size() * s);
                scene.add_prop("Tree", Prop::Builtin(tree), Vec3::new(*x, 0.0, *z), 0.0);
            }
        }
        None => {}
    }
    scene
        .objects
        .iter()
        .map(|o| o.id)
        .filter(|id| !before.contains(id))
        .collect()
}

fn title(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn interpretation(d: &ShotDescription, spec: &ShotSpec, characters: usize) -> String {
    let mut parts = vec![spec.name()];
    if let Some(f) = d.focal_length_mm {
        parts.push(format!("{f:.0} mm"));
    }
    if characters > 0 {
        let names: Vec<String> = d
            .subjects
            .iter()
            .map(|s| match s.pose {
                Some(p) => format!("{} ({})", s.name, p.name().replace('_', " ")),
                None => s.name.clone(),
            })
            .collect();
        parts.push(names.join(", "));
    }
    if !d.props.is_empty() {
        parts.push(
            d.props
                .iter()
                .map(|p| p.name().replace('_', " "))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if let Some(s) = d.setting {
        parts.push(format!("{s:?}").to_lowercase());
    }
    parts.join(" · ")
}
