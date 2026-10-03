//! Scratch voices (AI8, AI9) through a text-to-speech engine installed on
//! this computer — nothing is sent over the network. Piper (`piper`, neural
//! voices from `.onnx` models the user downloads) is used when it is found
//! and has a voice, otherwise eSpeak NG (`espeak-ng`), the way PDF import
//! leans on Poppler: no engine gives [`MISSING`]. Each line is written to a
//! WAV and imported into the sound library like any sound (which needs
//! FFmpeg). Programs run through the shared external-process helper with
//! cancel and timeout.
use crate::ffmpeg::{Program, Waited, command};
use anyhow::{Context, Result, bail};
use emulsion_core::storyboard::Preferences;
use emulsion_core::storyboard_voices::{
    Delivery, Emphasis, EngineChoice, NORMAL_PITCH, PlannedLine, Voice, VoiceEngine,
};
use emulsion_core::timeline::AudioAsset;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Shown when no engine is installed.
pub const MISSING: &str = "Scratch voices need a text-to-speech engine on this computer: install Piper (piper, with a downloaded voice) or eSpeak NG (espeak-ng) and make sure it is on PATH, then try again. Nothing is sent over the network.";
const PIPER_MISSING: &str = "This voice needs Piper. Install Piper (piper) and make sure it is on PATH, or choose an eSpeak NG voice.";
const ESPEAK_MISSING: &str = "This voice needs eSpeak NG. Install eSpeak NG (espeak-ng) and make sure it is on PATH, or choose a Piper voice.";
const PIPER: Program<'static> = Program {
    name: "Piper",
    missing: PIPER_MISSING,
};
const ESPEAK: Program<'static> = Program {
    name: "eSpeak NG",
    missing: ESPEAK_MISSING,
};

/// Longest one line may take to speak.
const LINE_TIMEOUT: Duration = Duration::from_secs(120);
/// Words a minute eSpeak NG speaks at normal rate.
const ESPEAK_WPM: f32 = 175.;
/// eSpeak NG variants uncast characters cycle through, so they differ.
const DEFAULT_VARIANTS: [&str; 8] = ["m3", "f2", "m1", "f4", "m7", "f1", "m2", "f3"];

/// The engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Piper,
    Espeak,
}

fn runs(program: &str, arg: &str) -> bool {
    command(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Whether Piper is on PATH.
pub fn piper_available() -> bool {
    runs("piper", "--help")
}

/// Whether eSpeak NG is on PATH.
pub fn espeak_available() -> bool {
    runs("espeak-ng", "--version")
}

/// Where voices come from: the Settings › Storyboard choices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub choice: EngineChoice,
    /// The Piper voices folder.
    pub piper_voices: Option<PathBuf>,
}

impl Config {
    pub fn from_preferences(prefs: &Preferences) -> Self {
        Self {
            choice: prefs.voice_engine,
            piper_voices: prefs.piper_voices.as_ref().map(PathBuf::from),
        }
    }

    /// The engine uncast characters speak with: Piper when chosen, or on
    /// Auto when it runs and the voices folder has a model; otherwise
    /// eSpeak NG.
    pub fn engine(&self) -> Result<Engine> {
        let piper = || piper_available() && !self.models().is_empty();
        match self.choice {
            EngineChoice::Piper if !piper_available() => bail!(PIPER_MISSING),
            EngineChoice::Piper if self.models().is_empty() => bail!(
                "Piper has no voice yet. Download a voice (.onnx with its .onnx.json) into a folder and choose it in Settings › Storyboard › Piper voices folder."
            ),
            EngineChoice::Piper => Ok(Engine::Piper),
            EngineChoice::Espeak if !espeak_available() => bail!(ESPEAK_MISSING),
            EngineChoice::Espeak => Ok(Engine::Espeak),
            EngineChoice::Auto if piper() => Ok(Engine::Piper),
            EngineChoice::Auto if espeak_available() => Ok(Engine::Espeak),
            EngineChoice::Auto => bail!(MISSING),
        }
    }

    /// The Piper models in the voices folder.
    pub fn models(&self) -> Vec<PiperModel> {
        self.piper_voices
            .as_deref()
            .map_or_else(Vec::new, piper_models)
    }

    /// A voice for the `index`th uncast character: Piper models (and their
    /// speakers) or eSpeak NG variants in turn.
    pub fn default_voice(&self, index: usize) -> Result<Voice> {
        Ok(Voice::new(match self.engine()? {
            Engine::Piper => {
                let models = self.models();
                let model = &models[index % models.len()];
                let speakers = model.speakers.len().max(1);
                VoiceEngine::Piper {
                    model: model.file_name(),
                    speaker: (speakers > 1).then_some(((index / models.len()) % speakers) as u32),
                }
            }
            Engine::Espeak => VoiceEngine::Espeak {
                voice: format!("en-us+{}", DEFAULT_VARIANTS[index % DEFAULT_VARIANTS.len()]),
            },
        }))
    }

    /// The model file a Piper voice names: a full path, or a file in the
    /// voices folder.
    pub fn model_path(&self, model: &str) -> Result<PathBuf> {
        let path = Path::new(model);
        let found = if path.is_absolute() {
            path.is_file().then(|| path.to_path_buf())
        } else {
            self.piper_voices
                .as_deref()
                .map(|dir| dir.join(path))
                .filter(|p| p.is_file())
        };
        found.with_context(|| {
            format!(
                "The Piper voice “{model}” is not in the Piper voices folder (Settings › Storyboard). Download it there or choose another voice."
            )
        })
    }
}

/// A Piper voice model found in a folder.
#[derive(Clone, Debug, PartialEq)]
pub struct PiperModel {
    /// The model's name ("en_US-amy-medium").
    pub name: String,
    pub path: PathBuf,
    /// Speaker names of a multi-speaker model, in speaker-number order;
    /// empty for one speaker.
    pub speakers: Vec<String>,
    /// The language code its config gives, if any.
    pub language: Option<String>,
}

impl PiperModel {
    /// What a cast stores: the file name, found again in the folder.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.name.clone(), |n| n.to_string_lossy().into_owned())
    }
}

/// The Piper models (`.onnx` with a `.onnx.json` config beside) in `dir`,
/// by name.
pub fn piper_models(dir: &Path) -> Vec<PiperModel> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PiperModel> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        })
        .filter_map(|path| {
            let mut config = path.clone().into_os_string();
            config.push(".json");
            let config: serde_json::Value =
                serde_json::from_slice(&std::fs::read(PathBuf::from(config)).ok()?).ok()?;
            let mut speakers: Vec<(u64, String)> = config["speaker_id_map"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(name, id)| Some((id.as_u64()?, name.clone())))
                        .collect()
                })
                .unwrap_or_default();
            let count = config["num_speakers"].as_u64().unwrap_or(1);
            if speakers.is_empty() && count > 1 {
                speakers = (0..count.min(10_000))
                    .map(|i| (i, format!("Speaker {i}")))
                    .collect();
            }
            speakers.sort();
            Some(PiperModel {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                speakers: speakers.into_iter().map(|(_, n)| n).collect(),
                language: config["language"]["code"]
                    .as_str()
                    .or_else(|| config["espeak"]["voice"].as_str())
                    .map(str::to_string),
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// An eSpeak NG language voice or variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EspeakVoice {
    /// What `-v` takes: a language code ("en-us") or, for a variant, its
    /// file name ("f2"), added after `+`.
    pub id: String,
    pub name: String,
    /// "M" or "F" ("-" when not given).
    pub gender: String,
}

fn espeak_list(arg: &str) -> Result<String> {
    let mut cmd = command("espeak-ng");
    cmd.arg(arg);
    let done = crate::ffmpeg::run(
        &mut cmd,
        ESPEAK,
        None,
        1 << 20,
        &AtomicBool::new(false),
        Some(Duration::from_secs(20)),
    )?;
    if !done.success() {
        bail!(
            "eSpeak NG could not list its voices. {}",
            crate::ffmpeg::last_line(&done.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// Parse `espeak-ng --voices` output: columns Pty, Language, Age/Gender,
/// VoiceName, File. MBROLA voices (which need extra data) are left out;
/// for variants the id is the file's name.
fn parse_espeak(text: &str, variants: bool) -> Vec<EspeakVoice> {
    let mut out: Vec<EspeakVoice> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let (language, gender, name, file) =
                (cols.get(1)?, cols.get(2)?, cols.get(3)?, cols.get(4)?);
            if file.starts_with("mb/") {
                return None;
            }
            let id = if variants {
                file.rsplit('/').next()?.to_string()
            } else {
                language.to_string()
            };
            Some(EspeakVoice {
                id,
                name: name.replace('_', " "),
                gender: gender.rsplit('/').next().unwrap_or("-").to_string(),
            })
        })
        .collect();
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// eSpeak NG's languages.
pub fn espeak_voices() -> Result<Vec<EspeakVoice>> {
    Ok(parse_espeak(&espeak_list("--voices")?, false))
}

/// eSpeak NG's voice variants ("f2", "m3", "Annie"…).
pub fn espeak_variants() -> Result<Vec<EspeakVoice>> {
    Ok(parse_espeak(&espeak_list("--voices=variant")?, true))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Run one engine command that writes `out`, feeding `text` on stdin.
fn speak(
    mut cmd: std::process::Command,
    program: Program,
    text: String,
    cancel: &AtomicBool,
) -> Result<()> {
    let done = crate::ffmpeg::run(
        &mut cmd,
        program,
        Some(text.into_bytes()),
        1 << 20,
        cancel,
        Some(LINE_TIMEOUT),
    )?;
    match done.waited {
        Waited::Exited(status) if status.success() => Ok(()),
        Waited::Exited(_) => bail!(
            "{} could not speak this line. {}",
            program.name,
            crate::ffmpeg::last_line(&done.stderr)
        ),
        Waited::Canceled => bail!("Canceled."),
        Waited::TimedOut => bail!("{} took too long on a line.", program.name),
    }
}

/// Shift `wav`'s pitch by `semitones`, keeping its length (for Piper,
/// which has no pitch control of its own).
fn shift_pitch(wav: &Path, out: &Path, semitones: f32, cancel: &AtomicBool) -> Result<()> {
    let ratio = 2f32.powf(semitones / 12.);
    let mut cmd = command("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(wav)
        .arg("-af")
        .arg(format!(
            "aresample=48000,asetrate={:.0},aresample=48000,atempo={:.5}",
            48_000. * ratio,
            1. / ratio
        ))
        .arg(out);
    let done = crate::ffmpeg::run(
        &mut cmd,
        crate::ffmpeg::FFMPEG,
        None,
        0,
        cancel,
        Some(LINE_TIMEOUT),
    )?;
    if !done.success() {
        bail!(
            "Changing the voice's pitch failed. {}",
            crate::ffmpeg::last_line(&done.stderr)
        );
    }
    Ok(())
}

/// Speak `text` in `voice`, as delivered, into the WAV file `out`.
pub fn synthesize(
    config: &Config,
    voice: &Voice,
    delivery: &Delivery,
    text: &str,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    let text = text.trim();
    if text.is_empty() {
        bail!("There is nothing to say.");
    }
    let (rate, pitch) = delivery.applied(voice);
    match &voice.engine {
        VoiceEngine::Espeak { voice } => {
            let mut cmd = command("espeak-ng");
            cmd.args(["-m", "--stdin", "-v", voice])
                .arg("-s")
                .arg(((ESPEAK_WPM * rate).round() as u32).to_string())
                .arg("-p")
                .arg(pitch.to_string())
                .arg("-w")
                .arg(out);
            let words = xml_escape(text);
            let words = match delivery.emphasis {
                Emphasis::None => words,
                Emphasis::Moderate => format!("<emphasis level=\"moderate\">{words}</emphasis>"),
                Emphasis::Strong => format!("<emphasis level=\"strong\">{words}</emphasis>"),
            };
            speak(cmd, ESPEAK, format!("<speak>{words}</speak>\n"), cancel)
        }
        VoiceEngine::Piper { model, speaker } => {
            let model = config.model_path(model)?;
            let (slower, livelier) = match delivery.emphasis {
                Emphasis::None => (1., 0.),
                Emphasis::Moderate => (1.05, 0.08),
                Emphasis::Strong => (1.1, 0.15),
            };
            let shifted = pitch != NORMAL_PITCH;
            let raw = if shifted {
                out.with_extension("raw.wav")
            } else {
                out.to_path_buf()
            };
            let mut cmd = command("piper");
            cmd.arg("--model")
                .arg(&model)
                .arg("--output_file")
                .arg(&raw)
                .arg("--length_scale")
                .arg(format!("{:.3}", slower / rate))
                .arg("--noise_scale")
                .arg(format!("{:.3}", (delivery.variation + livelier).min(1.)));
            if let Some(speaker) = speaker {
                cmd.arg("--speaker").arg(speaker.to_string());
            }
            speak(cmd, PIPER, format!("{text}\n"), cancel)?;
            if shifted {
                // ±6 semitones across the 0–99 scale.
                let semitones = (f32::from(pitch) - f32::from(NORMAL_PITCH)) / 50. * 6.;
                shift_pitch(&raw, out, semitones, cancel)?;
                let _ = std::fs::remove_file(&raw);
            }
            Ok(())
        }
    }
    .and_then(|()| {
        let written = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        if written <= 44 {
            bail!("The voice wrote no sound for this line.");
        }
        Ok(())
    })
}

/// Speak `text` and import the result as a sound (named "Scratch line";
/// the board names and files it when placing it). Needs FFmpeg.
pub fn take(
    config: &Config,
    voice: &Voice,
    delivery: &Delivery,
    text: &str,
    cancel: &AtomicBool,
) -> Result<AudioAsset> {
    let dir = tempfile::tempdir()?;
    let wav = dir.path().join("Scratch line.wav");
    synthesize(config, voice, delivery, text, &wav, cancel)?;
    crate::audio::store::import(&wav, "")
}

/// The voice a planned line speaks with: its cast voice, or a default.
pub fn voice_for(config: &Config, line: &PlannedLine) -> Result<Voice> {
    match &line.voice {
        Some(voice) => Ok(voice.clone()),
        None => config.default_voice(line.cast_index),
    }
}

/// Speak every planned line, in order, into sounds ready for
/// `Storyboard::apply_scratch`. `progress` hears (done, total) before each
/// line; a failure names the line.
pub fn takes(
    config: &Config,
    plan: Vec<PlannedLine>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<(PlannedLine, AudioAsset)>> {
    if plan.is_empty() {
        bail!(
            "There is no dialogue to speak: write lines like “MIA: Hello.” in the Dialogue captions."
        );
    }
    // Fail early, with the install message, when no engine runs.
    if plan.iter().any(|l| l.voice.is_none()) {
        config.engine()?;
    }
    let total = plan.len();
    let mut out = Vec::with_capacity(total);
    for (i, line) in plan.into_iter().enumerate() {
        progress(i, total);
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("Canceled.");
        }
        let voice = voice_for(config, &line)?;
        let asset =
            take(config, &voice, &line.delivery, &line.text, cancel).with_context(|| {
                let who = if line.character.is_empty() {
                    String::new()
                } else {
                    format!("{}'s ", line.character)
                };
                format!("Speaking {who}line “{}”", line.text)
            })?;
        out.push((line, asset));
    }
    progress(total, total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn espeak() -> Option<Config> {
        if !espeak_available() {
            eprintln!("skipped: eSpeak NG is not installed");
            return None;
        }
        Some(Config {
            choice: EngineChoice::Espeak,
            piper_voices: None,
        })
    }

    #[test]
    fn espeak_lists_and_speaks_lines_with_delivery() {
        let Some(config) = espeak() else { return };
        let voices = espeak_voices().unwrap();
        assert!(voices.iter().any(|v| v.id == "en-us"), "{voices:?}");
        assert!(espeak_variants().unwrap().iter().any(|v| v.id == "f2"));
        let voice = config.default_voice(1).unwrap();
        assert_eq!(
            voice.engine,
            VoiceEngine::Espeak {
                voice: "en-us+f2".into()
            }
        );
        let dir = tempfile::tempdir().unwrap();
        let length = |delivery: Delivery| {
            let out = dir.path().join("line.wav");
            synthesize(
                &config,
                &voice,
                &delivery,
                "Is anyone <there> & waiting?",
                &out,
                &AtomicBool::new(false),
            )
            .unwrap();
            std::fs::metadata(&out).unwrap().len()
        };
        let normal = length(Delivery::default());
        let slow = length(Delivery {
            rate: 0.5,
            emphasis: Emphasis::Strong,
            ..Delivery::default()
        });
        assert!(slow > normal * 3 / 2, "{slow} vs {normal}");
        // A voice eSpeak NG lacks names the problem.
        let bad = Voice::new(VoiceEngine::Espeak {
            voice: "zz-nope".into(),
        });
        let err = synthesize(
            &config,
            &bad,
            &Delivery::default(),
            "Hi",
            &dir.path().join("x.wav"),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(err.to_string().contains("eSpeak NG"), "{err}");
    }

    #[test]
    fn lines_become_sounds() {
        let Some(config) = espeak() else { return };
        if !crate::ffmpeg::available() {
            return;
        }
        let mut board = emulsion_core::storyboard::Storyboard::new(
            emulsion_core::storyboard::Settings::new(64, 36),
            &[1],
        );
        let field = board.dialogue_field().unwrap();
        board
            .panels
            .get_mut(&1)
            .unwrap()
            .captions
            .insert(field, "MIA: Hello there.\nTOM: Hi.".into());
        let plan = board.scratch_plan(&[1], &[1]);
        let mut seen = Vec::new();
        let takes = takes(&config, plan, &AtomicBool::new(false), |d, t| {
            seen.push((d, t))
        })
        .unwrap();
        assert_eq!(seen, [(0, 2), (1, 2), (2, 2)]);
        assert_eq!(takes.len(), 2);
        assert!(
            takes
                .iter()
                .all(|(_, a)| a.duration_ms > 200 && a.source.is_some())
        );
        board.apply_scratch(&[1], &[1], takes, true).unwrap();
        board.validate(&[1]).unwrap();
        assert!(takes_cancel_cleanly(&config));
    }

    fn takes_cancel_cleanly(config: &Config) -> bool {
        let line = PlannedLine {
            panel: 1,
            line: 0,
            character: String::new(),
            text: "Hello".into(),
            voice: None,
            cast_index: 0,
            delivery: Delivery::default(),
        };
        takes(config, vec![line], &AtomicBool::new(true), |_, _| {})
            .unwrap_err()
            .to_string()
            == "Canceled."
    }

    #[test]
    fn a_missing_engine_says_what_to_install() {
        let none = Config {
            choice: EngineChoice::Piper,
            piper_voices: None,
        };
        if !piper_available() {
            assert_eq!(none.engine().unwrap_err().to_string(), PIPER_MISSING);
        }
        if !piper_available() && !espeak_available() {
            let auto = Config::default();
            assert_eq!(auto.engine().unwrap_err().to_string(), MISSING);
        }
        assert!(MISSING.contains("Piper") && MISSING.contains("espeak-ng"));
        assert!(MISSING.contains("network"));
        let err = none.model_path("nope.onnx").unwrap_err().to_string();
        assert!(err.contains("Piper voices folder"), "{err}");
    }

    #[test]
    fn piper_models_are_read_from_the_voices_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("en_US-amy-medium.onnx"), b"model").unwrap();
        std::fs::write(
            dir.path().join("en_US-amy-medium.onnx.json"),
            br#"{"language":{"code":"en_US"},"num_speakers":1}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("en_GB-vctk.onnx"), b"model").unwrap();
        std::fs::write(
            dir.path().join("en_GB-vctk.onnx.json"),
            br#"{"num_speakers":3,"speaker_id_map":{"p3":2,"p1":0,"p2":1}}"#,
        )
        .unwrap();
        // No config: not a usable voice.
        std::fs::write(dir.path().join("broken.onnx"), b"model").unwrap();
        let models = piper_models(dir.path());
        let names: Vec<_> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["en_GB-vctk", "en_US-amy-medium"]);
        assert_eq!(models[0].speakers, ["p1", "p2", "p3"]);
        assert_eq!(models[1].language.as_deref(), Some("en_US"));
        let config = Config {
            choice: EngineChoice::Auto,
            piper_voices: Some(dir.path().into()),
        };
        assert_eq!(
            config.model_path("en_GB-vctk.onnx").unwrap(),
            dir.path().join("en_GB-vctk.onnx")
        );
        assert!(config.model_path("broken.onnx").is_ok());
        let listed = "Pty Language       Age/Gender VoiceName          File                 Other Languages\n 2  en-us           --/M      English_(America)  gmw/en-US            (en 3)\n 5  en-us           --/F      us-mbrola-1        mb/mb-us1            (en 8)\n";
        let parsed = parse_espeak(listed, false);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "English (America)");
        assert_eq!(parsed[0].gender, "M");
    }
}
