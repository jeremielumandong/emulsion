//! Scratch voices and dialogue tools (AI8, AI9): list the local
//! text-to-speech voices and the board's characters, cast them, generate a
//! scratch dialogue track from the Dialogue captions, and enhance a
//! dialogue clip through FFmpeg. Speech comes from Piper or eSpeak NG on
//! this computer; nothing is sent over the network. Clips are addressed by
//! track and clip number, as in `timing`.
use super::timing::{clip_number, scene_selection, selection, track, track_number};
use super::{def, layout};
use crate::ToolDef;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::Storyboard;
use emulsion_core::storyboard_voices::{
    MAX_PITCH, MAX_VOICES, RATE_RANGE, Voice, VoiceEngine, character_key,
};
use emulsion_io::voices::{self, Config};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

fn config() -> Config {
    Config::from_preferences(&emulsion_io::settings::Settings::load().storyboard)
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let mut generate = scene_selection();
    generate["extend_panels"] = json!({"type":"boolean","description":"Make unlocked panels longer when their lines need more time (default true). Locked panels keep their length."});
    vec![
        def(
            "list_storyboard_voices",
            "List the text-to-speech voices on this computer and the board's voice cast. `engines` says whether Piper and eSpeak NG are installed and which one uncast characters use (Settings › Storyboard: engine choice and Piper voices folder); `piper_models` lists the Piper voices in that folder (file name, speakers, language); `espeak_voices` the eSpeak NG languages and `espeak_variants` its variants (a voice is \"en-us\" or \"en-us+f2\"). `characters` lists every character who speaks in the Dialogue captions (\"MIA: line\", \"MIA (quietly): line\") with their cast voice, or the default voice they would get. `scratch_lines` counts generated takes. Read-only; nothing is sent over the network.",
            json!({}),
            &[],
        ),
        def(
            "set_storyboard_voice_cast",
            "Cast characters: give each a Piper voice (model file name from list_storyboard_voices, optional speaker number) or an eSpeak NG voice (\"en-gb\", \"en-us+f2\"), a rate (0.5–2 times normal) and a pitch (0–99, 50 is the voice's own; Piper pitch is shifted afterwards). `remove` drops a character from the cast. Character names match the Dialogue captions case-insensitively, without parentheticals. One Undo step.",
            json!({"voices":{"type":"array","minItems":1,"maxItems":MAX_VOICES,"items":{"type":"object","additionalProperties":false,"required":["character"],"properties":{
                "character":{"type":"string","minLength":1,"maxLength":200},
                "engine":{"enum":["piper","espeak"],"description":"Required unless remove is true."},
                "voice":{"type":"string","minLength":1,"maxLength":100,"description":"eSpeak NG voice, such as en-us or en-us+f2."},
                "model":{"type":"string","minLength":1,"maxLength":4096,"description":"Piper model file name in the Piper voices folder, or a full path to a .onnx file."},
                "speaker":{"type":"integer","minimum":0,"maximum":100000,"description":"Speaker number of a multi-speaker Piper model."},
                "rate":{"type":"number","minimum":*RATE_RANGE.start(),"maximum":*RATE_RANGE.end()},
                "pitch":{"type":"integer","minimum":0,"maximum":MAX_PITCH},
                "remove":{"type":"boolean"}
            }}}}),
            &["voices"],
        ),
        def(
            "generate_storyboard_scratch_dialogue",
            "Speak the Dialogue captions of the chosen panels or scenes (default: the whole board) with the voice cast, on this computer, and place the takes on the \"Scratch dialogue\" track (\"Scratch dialogue 2\"… when a take would overlap): each panel's lines back to back from the panel's start. Sounds go in the library's \"Scratch dialogue\" folder. Generating again replaces the earlier scratch takes of those panels, never other sounds or recordings. With extend_panels (default) unlocked panels grow to fit their lines. Needs Piper or eSpeak NG, and FFmpeg. Blocks while speaking. One Undo step.",
            generate,
            &[],
        ),
        def(
            "enhance_storyboard_dialogue_clip",
            "Clean up an audio clip's dialogue through FFmpeg: high-pass at 80 Hz, FFT noise reduction, de-essing, compression and loudness to -16 LUFS, into a new sound beside the original in its library folder (the original sound is kept). The clip then plays the enhanced sound, or with new_track a copy of the clip playing it goes on the \"Enhanced dialogue\" track and the original clip stays. Runs locally and blocks while working. One Undo step.",
            json!({"track":track_number(),"clip":clip_number(),"new_track":{"type":"boolean","description":"Keep the original clip and add the enhanced one on the Enhanced dialogue track (default false: the clip plays the enhanced sound)."}}),
            &["track", "clip"],
        ),
    ]
}

fn engine_json(engine: &VoiceEngine) -> Value {
    match engine {
        VoiceEngine::Espeak { voice } => json!({"engine":"espeak","voice":voice}),
        VoiceEngine::Piper { model, speaker } => {
            json!({"engine":"piper","model":model,"speaker":speaker})
        }
    }
}

fn voice_json(voice: &Voice) -> Value {
    let mut v = engine_json(&voice.engine);
    v["rate"] = json!(voice.rate);
    v["pitch"] = json!(voice.pitch);
    v["label"] = json!(voice.engine.label());
    v
}

fn list(editor: &ProjectEditor, board: &Storyboard) -> Value {
    let config = config();
    let engine = config.engine();
    let characters: Vec<Value> = board
        .characters(&layout(editor))
        .iter()
        .enumerate()
        .map(|(i, name)| match board.voices.voice(name) {
            Some(voice) => json!({"character":name,"cast":true,"voice":voice_json(voice)}),
            None => {
                let default = config.default_voice(i).ok();
                json!({"character":name,"cast":false,"voice":default.as_ref().map(voice_json)})
            }
        })
        .collect();
    let models: Vec<Value> = config
        .models()
        .iter()
        .map(|m| json!({"model":m.file_name(),"name":m.name,"speakers":m.speakers,"language":m.language}))
        .collect();
    let espeak = |list: anyhow::Result<Vec<voices::EspeakVoice>>| -> Value {
        list.map_or_else(
            |_| json!([]),
            |v| {
                v.iter()
                    .map(|v| json!({"id":v.id,"name":v.name,"gender":v.gender}))
                    .collect()
            },
        )
    };
    let has_espeak = voices::espeak_available();
    json!({
        "engines":{
            "piper":voices::piper_available(),
            "espeak":has_espeak,
            "choice":config.choice,
            "piper_voices_folder":config.piper_voices,
            "default_engine":engine.as_ref().ok().map(|e| format!("{e:?}").to_lowercase()),
            "message":engine.err().map(|e| e.to_string()),
        },
        "piper_models":models,
        "espeak_voices":if has_espeak { espeak(voices::espeak_voices()) } else { json!([]) },
        "espeak_variants":if has_espeak { espeak(voices::espeak_variants()) } else { json!([]) },
        "characters":characters,
        "scratch_lines":board.voices.lines.len(),
    })
}

fn cast(editor: &mut ProjectEditor, args: &Value) -> Result<Value, String> {
    let mut changes = Vec::new();
    for entry in args["voices"].as_array().unwrap() {
        let key = character_key(entry["character"].as_str().unwrap());
        if key.is_empty() {
            return Err("Name each character.".into());
        }
        if entry["remove"] == true {
            changes.push((key, None));
            continue;
        }
        let engine = match entry["engine"].as_str() {
            Some("espeak") => VoiceEngine::Espeak {
                voice: entry["voice"]
                    .as_str()
                    .ok_or("An eSpeak NG voice needs `voice`, such as en-us+f2.")?
                    .into(),
            },
            Some(_) => VoiceEngine::Piper {
                model: entry["model"]
                    .as_str()
                    .ok_or("A Piper voice needs `model`, a file name from list_storyboard_voices.")?
                    .into(),
                speaker: entry["speaker"].as_u64().map(|s| s as u32),
            },
            None => return Err(format!("Give {key} an engine, or remove them.")),
        };
        let mut voice = Voice::new(engine);
        if let Some(rate) = entry["rate"].as_f64() {
            voice.rate = rate as f32;
        }
        if let Some(pitch) = entry["pitch"].as_u64() {
            voice.pitch = pitch as u8;
        }
        changes.push((key, Some(voice)));
    }
    editor.edit_storyboard(|b| {
        for (key, voice) in changes {
            match voice {
                Some(voice) => {
                    b.voices.voices.insert(key, voice);
                }
                None => {
                    b.voices.voices.remove(&key);
                }
            }
        }
        Ok(())
    })?;
    let board = editor.storyboard().unwrap();
    let cast: serde_json::Map<String, Value> = board
        .voices
        .voices
        .iter()
        .map(|(k, v)| (k.clone(), voice_json(v)))
        .collect();
    Ok(json!({"cast":cast}))
}

fn generate(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let layout = layout(editor);
    let chosen = ["panels", "scenes", "scene_names"]
        .iter()
        .any(|k| args.get(*k).is_some());
    let scope = if chosen {
        selection(board, &layout, args)?
    } else {
        board
            .playing(&layout)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    let plan = board.scratch_plan(&layout, &scope);
    let takes = voices::takes(&config(), plan, &AtomicBool::new(false), |_, _| {})
        .map_err(|e| format!("{e:#}"))?;
    let extend = args["extend_panels"] != false;
    let mut report = None;
    editor.edit_storyboard(|b| {
        report = Some(b.apply_scratch(&layout, &scope, takes, extend)?);
        Ok(())
    })?;
    let report = report.unwrap();
    Ok(json!({
        "lines":report.placed,
        "replaced":report.removed,
        "extended_panels":report.extended,
        "locked_panels_too_short":report.locked,
        "tracks":editor.storyboard().unwrap().timeline.tracks.iter().enumerate()
            .filter(|(_, t)| t.name.starts_with(emulsion_core::storyboard_voices::SCRATCH_TRACK))
            .map(|(i, t)| json!({"track":i + 1,"name":t.name,"clips":t.clips.len()}))
            .collect::<Vec<_>>(),
    }))
}

fn enhance(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let t = track(board, &args["track"])?;
    let n = args["clip"].as_u64().unwrap() as usize;
    let clip = board.timeline.tracks[t]
        .clips
        .get(n - 1)
        .ok_or_else(|| format!("There is no clip {n} on track {}.", t + 1))?;
    let source = board
        .timeline
        .assets
        .get(&clip.asset)
        .and_then(|a| a.source.clone())
        .ok_or("That clip's sound has no data to enhance.")?;
    let asset = emulsion_io::audio::enhance::enhance(&source, &AtomicBool::new(false))
        .map_err(|e| e.to_string())?;
    let original = clip.asset;
    let new_track = args["new_track"] == true;
    let mut at = None;
    editor.edit_storyboard(|b| {
        at = Some(b.place_enhanced((t, n - 1), asset, new_track)?);
        Ok(())
    })?;
    let (track, clip) = at.unwrap();
    let board = editor.storyboard().unwrap();
    let placed = &board.timeline.tracks[track].clips[clip];
    Ok(json!({
        "track":track + 1,
        "clip":clip + 1,
        "sound":placed.asset,
        "name":board.timeline.assets[&placed.asset].name,
        "original_sound":original,
    }))
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    Some(match name {
        "list_storyboard_voices" => Ok(list(editor, board)),
        "set_storyboard_voice_cast" => cast(editor, args),
        "generate_storyboard_scratch_dialogue" => generate(editor, board, args),
        "enhance_storyboard_dialogue_clip" => enhance(editor, board, args),
        _ => return None,
    })
}
