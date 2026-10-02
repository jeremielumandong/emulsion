//! Audio clip effects tools (T12): read a clip's gain envelope and
//! three-band EQ, set and delete keys on either, and set fixed EQ gains.
//! Keys sit at frames from the clip's start and ease like layer keys.
//! Clips are addressed by track and clip number, as in `timing`.
use super::def;
use super::timing::{clip_number, track, track_number};
use crate::ToolDef;
use emulsion_core::motion::Easing;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::Storyboard;
use emulsion_core::timeline::effects::{
    EQ_HIGH_HZ, EQ_LOW_HZ, EQ_MID_HZ, EQ_MID_Q, EQ_RANGE_DB, MAX_EFFECT_KEYS,
};
use emulsion_core::timeline::{AudioClip, ClipParam, EffectKey};
use serde_json::{Value, json};

const PARAMS: [&str; 4] = ["envelope", "low", "mid", "high"];
const EASINGS: [&str; 5] = ["linear", "ease_in", "ease_out", "ease_in_out", "step"];

fn easing_name(easing: Easing) -> &'static str {
    match easing {
        Easing::Linear => "linear",
        Easing::EaseIn => "ease_in",
        Easing::EaseOut => "ease_out",
        Easing::EaseInOut => "ease_in_out",
        Easing::Step => "step",
    }
}

fn param() -> Value {
    json!({"enum":PARAMS,"description":"envelope: the gain envelope in dB (-60 to +24), added to the clip's gain. low, mid, high: the EQ bands in dB (-24 to +24): a low shelf at 200 Hz, a peak at 1 kHz and a high shelf at 5 kHz."})
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let at = || json!({"track":track_number(),"clip":clip_number()});
    let mut describe = at();
    describe["frames"] = json!({"type":"array","items":{"type":"integer","minimum":0},"maxItems":64,"description":"Also give every parameter's value at these frames into the clip."});
    let mut set_keys = at();
    set_keys["param"] = param();
    set_keys["keys"] = json!({"type":"array","minItems":1,"maxItems":MAX_EFFECT_KEYS,"items":{"type":"object","additionalProperties":false,"required":["db"],"properties":{
        "frame":{"type":"integer","minimum":0,"description":"Frames from the clip's start. Use frame or seconds."},
        "seconds":{"type":"number","minimum":0,"maximum":86400,"description":"Seconds from the clip's start, rounded to a frame."},
        "db":{"type":"number","minimum":-60,"maximum":24},
        "easing":{"enum":EASINGS,"description":"How the value moves from this key to the next (default linear; step holds)."}
    }}});
    let mut delete_keys = at();
    delete_keys["param"] = param();
    delete_keys["frames"] = json!({"type":"array","items":{"type":"integer","minimum":0},"minItems":1,"maxItems":MAX_EFFECT_KEYS,"description":"Frames (from the clip's start) of the keys to delete. Omit to delete all of the parameter's keys."});
    let mut eq = at();
    for band in ["low_db", "mid_db", "high_db"] {
        eq[band] = json!({"type":"number","minimum":-EQ_RANGE_DB,"maximum":EQ_RANGE_DB});
    }
    vec![
        def(
            "describe_storyboard_clip_effects",
            "Read an audio clip's effects: its gain envelope keys and its three EQ bands (fixed gain or keys), with each key's frame from the clip's start, dB and easing, and optionally every parameter's value at chosen frames.",
            describe,
            &["track", "clip"],
        ),
        def(
            "set_storyboard_clip_effect_keys",
            "Add keys to a clip's gain envelope or one EQ band (low, mid, high), each at a frame or second from the clip's start, with a dB value and easing; a key on a frame that already has one replaces it. The envelope adds to the clip's gain (fades still apply); a keyed band ignores its fixed gain. One Undo step.",
            set_keys,
            &["track", "clip", "param", "keys"],
        ),
        def(
            "delete_storyboard_clip_effect_keys",
            "Delete keys of a clip's gain envelope or one EQ band, by frame from the clip's start, or all of them. A band without keys plays its fixed gain again. One Undo step.",
            delete_keys,
            &["track", "clip", "param"],
        ),
        def(
            "set_storyboard_clip_eq",
            "Set fixed EQ gains in dB (-24 to +24) on a clip: low shelf (200 Hz), mid peak (1 kHz) and high shelf (5 kHz). A band given here loses its keys; 0 on every band turns the EQ off. One Undo step.",
            eq,
            &["track", "clip"],
        ),
    ]
}

/// The clip's track index and index from `track` and `clip` numbers.
fn locate(board: &Storyboard, args: &Value) -> Result<(usize, usize), String> {
    let t = track(board, &args["track"])?;
    let n = args["clip"].as_u64().unwrap() as usize;
    if n > board.timeline.tracks[t].clips.len() {
        return Err(format!("There is no clip {n} on track {}.", t + 1));
    }
    Ok((t, n - 1))
}

fn keys_json(keys: &[EffectKey]) -> Vec<Value> {
    keys.iter()
        .map(|k| json!({"frame":k.frame,"db":k.db,"easing":easing_name(k.easing),"curve":k.curve.is_some()}))
        .collect()
}

fn effects_json(board: &Storyboard, (t, c): (usize, usize), frames: &[u64]) -> Value {
    let clip = &board.timeline.tracks[t].clips[c];
    let band = |b: &emulsion_core::timeline::EqBand, hz: f32| json!({"hz":hz,"db":b.db,"keys":keys_json(&b.keys)});
    let mut mid = band(&clip.eq.mid, EQ_MID_HZ);
    mid["q"] = json!(EQ_MID_Q);
    let samples: Vec<Value> = frames
        .iter()
        .map(|f| {
            let mut v = json!({"frame":f});
            for param in ClipParam::ALL {
                v[param.key()] = json!(clip.param_at(param, *f as f64));
            }
            v
        })
        .collect();
    json!({
        "track":t + 1,
        "clip":c + 1,
        "name":clip.name,
        "frames":clip.frames,
        "gain_db":clip.gain_db,
        "envelope":keys_json(&clip.envelope),
        "eq":{
            "on":!clip.eq.is_flat(),
            "low":band(&clip.eq.low, EQ_LOW_HZ),
            "mid":mid,
            "high":band(&clip.eq.high, EQ_HIGH_HZ),
        },
        "values":samples,
    })
}

fn easing(value: &Value) -> Easing {
    match value.as_str() {
        Some("ease_in") => Easing::EaseIn,
        Some("ease_out") => Easing::EaseOut,
        Some("ease_in_out") => Easing::EaseInOut,
        Some("step") => Easing::Step,
        _ => Easing::Linear,
    }
}

/// Change the clip at `at` as one Undo step and describe it.
fn edit(
    editor: &mut ProjectEditor,
    at: (usize, usize),
    change: impl FnOnce(&mut AudioClip) -> Result<(), String>,
) -> Result<Value, String> {
    editor.edit_storyboard(|b| change(&mut b.timeline.tracks[at.0].clips[at.1]))?;
    Ok(effects_json(editor.storyboard().unwrap(), at, &[]))
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let param = || ClipParam::from_key(args["param"].as_str().unwrap_or("")).unwrap();
    let result = match name {
        "describe_storyboard_clip_effects" => locate(board, args).map(|at| {
            let frames = super::ids(&args["frames"]);
            effects_json(board, at, &frames)
        }),
        "set_storyboard_clip_effect_keys" => locate(board, args).and_then(|at| {
            let rate = board.settings.frame_rate;
            let param = param();
            let (lo, hi) = param.range();
            let mut keys = Vec::new();
            for key in args["keys"].as_array().unwrap() {
                let frame = match (key["frame"].as_u64(), key["seconds"].as_f64()) {
                    (Some(_), Some(_)) => {
                        return Err("Give a key's frame or seconds, not both.".into());
                    }
                    (Some(f), None) => f,
                    (None, Some(s)) => rate.seconds_to_frames(s),
                    (None, None) => return Err("Each key needs a frame or seconds.".into()),
                };
                let db = key["db"].as_f64().unwrap() as f32;
                if !(lo..=hi).contains(&db) {
                    return Err(format!("{} keys are {lo} to {hi} dB.", param.label()));
                }
                keys.push(EffectKey {
                    frame,
                    db,
                    easing: easing(&key["easing"]),
                    curve: None,
                });
            }
            edit(editor, at, |clip| {
                for key in keys {
                    emulsion_core::timeline::effects::set_key(clip.keys_mut(param), key);
                }
                Ok(())
            })
        }),
        "delete_storyboard_clip_effect_keys" => locate(board, args).and_then(|at| {
            let param = param();
            let frames = args.get("frames").map(super::ids);
            edit(editor, at, |clip| {
                let keys = clip.keys_mut(param);
                match frames {
                    None => keys.clear(),
                    Some(frames) => {
                        if let Some(missing) =
                            frames.iter().find(|f| !keys.iter().any(|k| k.frame == **f))
                        {
                            return Err(format!(
                                "{} has no key at frame {missing}.",
                                param.label()
                            ));
                        }
                        keys.retain(|k| !frames.contains(&k.frame));
                    }
                }
                Ok(())
            })
        }),
        "set_storyboard_clip_eq" => locate(board, args).and_then(|at| {
            edit(editor, at, |clip| {
                for (field, band) in [
                    ("low_db", &mut clip.eq.low),
                    ("mid_db", &mut clip.eq.mid),
                    ("high_db", &mut clip.eq.high),
                ] {
                    if let Some(db) = args[field].as_f64() {
                        band.db = db as f32;
                        band.keys.clear();
                    }
                }
                Ok(())
            })
        }),
        _ => return None,
    };
    Some(result)
}
