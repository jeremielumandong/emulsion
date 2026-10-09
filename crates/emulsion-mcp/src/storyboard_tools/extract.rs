//! Production hand-offs: extract a run of scenes to a new `.emu` for another
//! artist and merge their file back (conflicts reported, resolved per panel,
//! applied as one Undo step), and export scenes as layered ORA or PSD files
//! with a JSON description each for animation production.
use super::def;
use super::export::output;
use crate::ToolDef;
use emulsion_core::{
    project::ProjectEditor,
    storyboard::Storyboard,
    storyboard_extract::{MergeOptions, MergeReport, Resolution},
};
use emulsion_io::storyboard_export::layered;
use emulsion_io::storyboard_extract::{read_extract, write_extract};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

pub(super) fn definitions() -> Vec<ToolDef> {
    let groups = json!({"type":"array","minItems":1,"maxItems":500,"items":{"type":"integer","minimum":1},"description":"Scene IDs from describe_storyboard, or sequence/act IDs for all their scenes. Together they must be one run of neighbouring scenes."});
    vec![
        def(
            "extract_storyboard_scenes",
            "Extract a run of whole scenes to a new storyboard .emu for another artist: their panels (layers, captions, timing, keyframes, comps), the scenes' cameras, audio and reference video clips under them (cut to the range, starting at frame 0), the sounds and videos those clips play, and the project library. The file records this project's ID, the scenes and panels taken with a content fingerprint of each, and the time, so merge_storyboard_extract can bring it back and spot conflicts. The board does not change. Returns the extract's panels and range.",
            json!({
                "groups":groups,
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the .emu file to write; an existing file is replaced."},
                "title":{"type":"string","maxLength":200,"description":"This project's name, shown when the extract is merged. Default: Storyboard."},
                "claim_for":{"type":"string","minLength":1,"maxLength":100,"description":"Claim the extracted scenes for this artist, in the extract and on this board (advisory; see claim_storyboard_scenes). One Undo step here."}
            }),
            &["groups", "path"],
        ),
        def(
            "merge_storyboard_extract",
            "Merge an extract made by extract_storyboard_scenes (and edited since) back: the extracted scenes are replaced by the extract's panels, scenes, cameras and the sound and video in the range, and everything after the range moves by the change in running time. Conflicts: `changed_here` (the panel changed here since the extract; `changed_there` says whether the extract changed it too), `deleted_here`, `deleted_there` and `added_here`; each has a default (whichever side did the work). Run with dry_run first and show the person the conflicts, then apply with `resolutions` (theirs or mine per panel). An extract of another project is refused unless merge_anyway. One Undo step.",
            json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the extract .emu."},
                "dry_run":{"type":"boolean","description":"Report the range and conflicts without changing the board."},
                "resolutions":{"type":"array","maxItems":4096,"items":{"type":"object","additionalProperties":false,"required":["panel","take"],"properties":{
                    "panel":{"type":"integer","minimum":1,"description":"The conflict's panel ID."},
                    "take":{"enum":["theirs","mine"]}
                }},"description":"Choices for conflicts; others take their default."},
                "merge_anyway":{"type":"boolean","description":"Merge an extract made from another project. Only when the person confirms this project is a copy of that one."}
            }),
            &["path"],
        ),
        def(
            "export_storyboard_layered_scenes",
            "Export scenes for animation production: each panel as a layered OpenRaster (.ora) or PSD (.psd) file with groups, blend modes, opacity and visibility, plus one JSON per scene (schema emulsion.storyboard.scene/1) with the panels' names, files, durations, start frames and SMPTE timecode, captions, the camera keys falling in each panel (frames from the panel start, easing and bezier curves) with the framing at its first and last frame, layer keyframes by layer ID and name, layer comps, and every layer's ID, name, parent, blend and opacity. Panel names use the panel image tokens ({project} {act} {seq} {scene} {panel} {name} {index} {frames} {duration} {timecode} {shot} {angle} {status}; {index:3} pads); scene files use {project} {act} {seq} {scene}. Patterns that give two files one name write nothing. Returns the files and any file-specific PSD flattening, baked-mask or rounded-density warnings.",
            json!({
                "directory":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute folder path, created if needed."},
                "format":{"enum":["ora","psd"],"description":"Default ora."},
                "pattern":{"type":"string","minLength":1,"maxLength":200,"description":"Panel file names. Default {seq}_{scene}_{panel}."},
                "scene_pattern":{"type":"string","minLength":1,"maxLength":200,"description":"Scene JSON names. Default {seq}_{scene}."},
                "scenes":{"type":"array","maxItems":500,"items":{"type":"integer","minimum":1},"description":"Scene IDs to export; omit for every scene."},
                "title":{"type":"string","maxLength":200,"description":"Value of {project}. Default: Storyboard."}
            }),
            &["directory"],
        ),
    ]
}

fn report_json(report: &MergeReport) -> Value {
    json!({
        "same_project":report.same_project,
        "source_project":report.source_project,
        "this_project":report.this_project,
        "source_name":report.source_name,
        "extracted_at":report.extracted_at,
        "panels_here":report.panels_here,
        "panels_there":report.panels_there,
        "frames_here":report.frames_here,
        "frames_there":report.frames_there,
        "conflicts":report.conflicts,
    })
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    _board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let title = args["title"].as_str().unwrap_or("Storyboard");
    let result = match name {
        "extract_storyboard_scenes" => (|| {
            let path = output(args, "path", Some("emu"))?;
            let groups = super::ids(&args["groups"]);
            let mut project = editor.snapshot().ok_or("Open a storyboard first.")?;
            // Claims go into the extract and, once it is written, this board.
            let claim = match args["claim_for"].as_str() {
                Some(artist) => {
                    let board = project
                        .storyboard
                        .as_mut()
                        .ok_or("Open a storyboard first.")?;
                    let layout: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
                    let scenes =
                        emulsion_core::storyboard_extract::range_scenes(board, &layout, &groups)?;
                    let device = emulsion_io::cloud::shared::device_id();
                    let now = emulsion_core::storyboard_review::now();
                    board.claim_scenes(&scenes, artist, &device, now)?;
                    Some((scenes, artist.to_string(), device, now))
                }
                None => None,
            };
            let extract =
                write_extract(&project, &groups, title, &path).map_err(|e| e.to_string())?;
            if let Some((scenes, artist, device, now)) = &claim {
                editor.edit_storyboard(|b| b.claim_scenes(scenes, artist, device, *now))?;
            }
            // The extract names this project by its ID; keep it on disk.
            editor.mark_storyboard_unsaved();
            let board = extract.storyboard.as_ref().unwrap();
            let record = board.extract.as_ref().unwrap();
            Ok(json!({
                "path":path,
                "panels":extract.pages.iter().map(|p| p.meta.id).collect::<Vec<_>>(),
                "scenes":record.scenes,
                "start_frame":record.start_frame,
                "frames":record.frames,
                "source_project":record.source_project,
                "sounds":board.timeline.assets.len(),
                "videos":board.timeline.videos.len(),
            }))
        })(),
        "merge_storyboard_extract" => (|| {
            let path = output(args, "path", Some("emu"))?;
            let extract = read_extract(&path).map_err(|e| e.to_string())?;
            let report = editor.plan_merge(&extract)?;
            if args["dry_run"] == true {
                return Ok(json!({"dry_run":true,"report":report_json(&report)}));
            }
            let resolutions = args["resolutions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    let take = if r["take"] == "mine" {
                        Resolution::Mine
                    } else {
                        Resolution::Theirs
                    };
                    (r["panel"].as_u64().unwrap_or_default(), take)
                })
                .collect();
            let options = MergeOptions {
                resolutions,
                allow_other_project: args["merge_anyway"] == true,
            };
            let summary = editor.merge_extract(&extract, &options)?;
            Ok(json!({
                "panels":summary.panels,
                "kept_mine":summary.kept_mine,
                "took_theirs":summary.took_theirs,
                "frames_delta":summary.frames_delta,
                "report":report_json(&report),
            }))
        })(),
        "export_storyboard_layered_scenes" => (|| {
            let directory = output(args, "directory", None)?;
            let mut options = layered::Options {
                scenes: super::ids(&args["scenes"]),
                ..Default::default()
            };
            if args["format"] == "psd" {
                options.format = layered::Format::Psd;
            }
            if let Some(pattern) = args["pattern"].as_str() {
                options.pattern = pattern.into();
            }
            if let Some(pattern) = args["scene_pattern"].as_str() {
                options.scene_pattern = pattern.into();
            }
            options.validate().map_err(|e| e.to_string())?;
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let written = layered::write_with_reports(
                &project,
                title,
                &options,
                &directory,
                &AtomicBool::new(false),
                &mut |_, _| {},
            )
            .map_err(|failure| {
                let mut message = failure.error.to_string();
                for (path, report) in &failure.written.psd_reports {
                    let warning = crate::export_tools::psd_export_warnings(Some(*report));
                    if !warning.is_empty() {
                        message.push_str(&format!("\n{}{warning}", path.display()));
                    }
                }
                message
            })?;
            let warnings: Vec<_> = written
                .psd_reports
                .iter()
                .filter_map(|(path, report)| {
                    let warning = crate::export_tools::psd_export_warnings(Some(*report));
                    if warning.is_empty() {
                        None
                    } else {
                        Some(json!({"path":path,"message":warning.trim_start_matches("; ")}))
                    }
                })
                .collect();
            let mut result =
                json!({"directory":directory,"panels":written.panels,"scenes":written.scenes});
            if !warnings.is_empty() {
                result["warnings"] = json!(warnings);
            }
            Ok(result)
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::super::export::tests::Dir;
    use super::super::tests::{board, call};
    use emulsion_core::project::ProjectEditor;
    use serde_json::json;

    fn scenes(e: &mut ProjectEditor) -> Vec<u64> {
        let outline = call(e, "describe_storyboard", json!({}));
        outline["acts"][0]["sequences"][0]["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_u64().unwrap())
            .collect()
    }

    #[test]
    fn scenes_extract_merge_back_and_export_as_layered_files() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"start":"scene","group_name":"Chase","panels":[{"frames":24},{"frames":24}]}),
        );
        let ids = scenes(&mut e);
        let chase = ids[1];
        let dir = Dir::new("extract");
        let file = dir.path().join("chase.emu");
        let stamp = e.stamp();
        let extracted = call(
            &mut e,
            "extract_storyboard_scenes",
            json!({"groups":[chase],"path":file,"title":"Film"}),
        );
        assert_eq!(extracted["panels"].as_array().unwrap().len(), 2);
        assert_eq!(extracted["start_frame"], 48);
        assert_eq!(e.stamp(), stamp, "extracting leaves the board alone");
        assert!(
            e.is_modified(),
            "but asks for a save, to keep the project ID"
        );

        // The other artist lengthens a panel.
        let mut artist = ProjectEditor::open(
            emulsion_io::storyboard_extract::read_extract(&file).unwrap(),
            Some(file.clone()),
        )
        .unwrap();
        let first = extracted["panels"][0].as_u64().unwrap();
        call(
            &mut artist,
            "update_storyboard_panel",
            json!({"panel":first,"frames":48}),
        );
        emulsion_io::project::write(&artist.snapshot().unwrap(), &file).unwrap();

        let dry = call(
            &mut e,
            "merge_storyboard_extract",
            json!({"path":file,"dry_run":true}),
        );
        assert_eq!(dry["report"]["same_project"], true);
        assert_eq!(dry["report"]["frames_there"], 72);
        assert_eq!(e.stamp(), stamp, "a dry run changes nothing");
        let merged = call(&mut e, "merge_storyboard_extract", json!({"path":file}));
        assert_eq!(merged["frames_delta"], 24);
        let outline = call(&mut e, "describe_storyboard", json!({}));
        assert_eq!(outline["total_frames"], 48 + 72);
        assert!(e.undo());
        assert_eq!(e.stamp(), stamp);

        let out = dir.path().join("layers");
        let written = call(
            &mut e,
            "export_storyboard_layered_scenes",
            json!({"directory":out,"format":"psd","scenes":[chase],"title":"Film"}),
        );
        assert_eq!(written["panels"].as_array().unwrap().len(), 2);
        assert_eq!(written["scenes"].as_array().unwrap().len(), 1);
        assert!(out.join("Sequence 1_Chase_1.psd").is_file());
        assert!(out.join("Sequence 1_Chase.json").is_file());
    }

    #[test]
    fn layered_psd_results_disclose_each_written_panels_actual_losses() {
        use emulsion_core::{Document, Node};
        use emulsion_raster::{Mask, Raster};
        use std::sync::Arc;
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{"frames":24},{"frames":24}]}),
        );
        let mut project = e.snapshot().unwrap();
        for (i, page) in project.pages.iter_mut().enumerate() {
            let mut doc = Document::new(64, 36);
            let mut node = Node::raster(
                1,
                "Ink",
                Arc::new(Raster::solid(64, 36, [0., 0., 0., 1.])),
                Default::default(),
            );
            if i == 1 {
                node.mask = Some(Arc::new(Mask::empty(64, 36, 127)));
            } else if i == 2 {
                node.mask = Some(Arc::new(Mask::empty(64, 36, 255)));
                {
                    let mut affine = node.mask_transform.affine().expect("affine fixture");
                    let mut columns = affine.to_cols_array();
                    columns[4] = 0.5;
                    affine = glam::DAffine2::from_cols_array(&columns);
                    node.mask_transform = emulsion_core::Mapping2::Affine(affine);
                }
                node.mask_enabled = false;
                let mut rounded = node.clone();
                rounded.id = 2;
                rounded.name = "Rounded".into();
                {
                    let mut affine = rounded.mask_transform.affine().expect("affine fixture");
                    let mut columns = affine.to_cols_array();
                    columns[4] = 0.;
                    affine = glam::DAffine2::from_cols_array(&columns);
                    rounded.mask_transform = emulsion_core::Mapping2::Affine(affine);
                }
                rounded.mask_properties.density = 0.1;
                doc.nodes.push(rounded);
            }
            doc.nodes.push(node);
            doc.next_id = 3;
            page.doc = doc;
        }
        let before: Vec<_> = project.pages.iter().map(|p| p.doc.clone()).collect();
        e = ProjectEditor::open(project, None).unwrap();
        let stamp = e.stamp();
        let active = e.active_page();
        let dir = Dir::new("layered-write-reports");
        let written = call(
            &mut e,
            "export_storyboard_layered_scenes",
            json!({
                "directory":dir.path().join("psd"), "format":"psd", "pattern":"panel-{index}"
            }),
        );
        let warnings = written["warnings"].as_array().unwrap();
        assert_eq!(written["panels"].as_array().unwrap().len(), 3);
        assert_eq!(warnings.len(), 2, "ordinary PSD adds no warning");
        assert_eq!(warnings[0]["path"], written["panels"][1]);
        assert!(
            warnings[0]["message"]
                .as_str()
                .unwrap()
                .contains("PSD appearance flattened")
        );
        assert!(!warnings[0]["message"].as_str().unwrap().contains("density"));
        assert_eq!(warnings[1]["path"], written["panels"][2]);
        let combined = warnings[1]["message"].as_str().unwrap();
        assert!(combined.contains("transforms were baked"), "{combined}");
        assert!(
            combined.contains("density values rounded to 8-bit: 1"),
            "{combined}"
        );
        assert!(!combined.contains("flattened"));
        for path in written["panels"].as_array().unwrap() {
            assert!(std::path::Path::new(path.as_str().unwrap()).is_file());
        }
        let ora = call(
            &mut e,
            "export_storyboard_layered_scenes",
            json!({
                "directory":dir.path().join("ora"), "format":"ora"
            }),
        );
        assert!(ora.get("warnings").is_none(), "ORA results stay unchanged");
        let blocked = dir.path().join("blocked");
        std::fs::create_dir_all(blocked.join("panel-1.psd")).unwrap();
        let failed = execute(
            &mut e,
            "export_storyboard_layered_scenes",
            &json!({
                "directory":blocked, "format":"psd", "pattern":"panel-{index}"
            }),
        );
        assert!(failed.is_error);
        let message = failed.content[0]["text"].as_str().unwrap();
        assert!(!message.contains("warnings"));
        assert!(!message.contains("flattened"));
        // A later panel or JSON failure must retain the first lossy file's warning.
        let mut partial_project = e.snapshot().unwrap();
        partial_project.pages[0].doc = partial_project.pages[1].doc.clone();
        let mut partial = ProjectEditor::open(partial_project, None).unwrap();
        let partial_stamp = partial.stamp();
        for (tag, blocked_file, completed) in [
            ("later-panel", "panel-2.psd", 1),
            ("scene-json", "Sequence 1_1.json", 3),
        ] {
            let out = dir.path().join(tag);
            std::fs::create_dir_all(out.join(blocked_file)).unwrap();
            let result = execute(
                &mut partial,
                "export_storyboard_layered_scenes",
                &json!({
                    "directory":out, "format":"psd", "pattern":"panel-{index}"
                }),
            );
            assert!(result.is_error);
            let text = result.content[0]["text"].as_str().unwrap();
            assert!(text.starts_with("Cannot write"), "{text}");
            assert!(
                text.contains("panel-1.psd; PSD appearance flattened"),
                "{text}"
            );
            assert!(!text.contains("Wrote") && !text.contains("Exported"));
            assert!(out.join("panel-1.psd").is_file());
            assert_eq!(text.lines().count(), completed + 1, "{text}");
            assert_eq!(partial.stamp(), partial_stamp);
        }
        assert_eq!(e.stamp(), stamp);
        assert_eq!(e.active_page(), active);
        assert_eq!(
            e.snapshot()
                .unwrap()
                .pages
                .iter()
                .map(|p| p.doc.clone())
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn bad_extracts_and_merges_change_nothing() {
        let mut e = board();
        let dir = Dir::new("extract-bad");
        let stamp = e.stamp();
        let plain = dir.path().join("plain.emu");
        emulsion_io::project::write(&e.snapshot().unwrap(), &plain).unwrap();
        for (name, args) in [
            (
                "extract_storyboard_scenes",
                json!({"groups":[999],"path":dir.path().join("x.emu")}),
            ),
            (
                "extract_storyboard_scenes",
                json!({"groups":[1],"path":"x.emu"}),
            ),
            (
                "extract_storyboard_scenes",
                json!({"groups":[1],"path":dir.path().join("x.ora")}),
            ),
            ("merge_storyboard_extract", json!({"path":plain})),
            (
                "merge_storyboard_extract",
                json!({"path":dir.path().join("missing.emu")}),
            ),
            (
                "export_storyboard_layered_scenes",
                json!({"directory":dir.path().join("o"),"format":"tiff"}),
            ),
            (
                "export_storyboard_layered_scenes",
                json!({"directory":dir.path().join("o"),"scene_pattern":"{panel}"}),
            ),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
        assert!(!dir.path().join("x.emu").exists());
        assert!(!dir.path().join("o").exists());
    }
}
