//! Shared storyboards through cloud sync: who saved the file, other
//! artists' saves waiting to be merged, scene claims, and merging a
//! downloaded revision three ways against the common version. The merge
//! works on revision files already on disk (the editor's Review and merge
//! fetches them), so these tools never touch the network.
use super::def;
use super::export::output;
use crate::ToolDef;
use emulsion_core::{
    project::ProjectEditor,
    storyboard::Storyboard,
    storyboard_merge::{BoardMergeReport, ConflictKey, Resolution},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn definitions() -> Vec<ToolDef> {
    let scenes = json!({"type":"array","minItems":1,"maxItems":500,"items":{"type":"integer","minimum":1},"description":"Scene IDs from describe_storyboard."});
    let file =
        |what: &str| json!({"type":"string","minLength":2,"maxLength":4096,"description":what});
    vec![
        def(
            "describe_storyboard_sharing",
            "Read-only. The open storyboard's sharing state: its project ID, scene claims (scene, claimant, device, time), the cloud revision it last merged, and, when the file is synced, its provider, the revision it was last saved as, queued uploads, the collaborators seen in its revisions (author names and devices) and the other artists' saves (heads) waiting to be merged, from the last cloud listing the app made. Use before claiming scenes or merging.",
            json!({}),
            &[],
        ),
        def(
            "claim_storyboard_scenes",
            "Claim scenes for an artist (\"I'm working on scenes 4–6\"). Claims are advisory: anyone can still edit, and the app warns when someone edits a scene another artist claimed. A claim replaces an earlier one on the same scene; claims are saved and travel with cloud revisions, merging as a union where the latest claim or release of each scene wins. One Undo step.",
            json!({
                "scenes":scenes,
                "claimant":{"type":"string","minLength":1,"maxLength":100,"description":"The artist's name. Default: the name in Settings › Storyboard."},
                "device":{"type":"string","maxLength":64,"description":"The installation claiming. Default: this one."}
            }),
            &["scenes"],
        ),
        def(
            "release_storyboard_scenes",
            "Release the claims on scenes (anyone's: claims are advisory). The release is kept with its time so it wins over the older claim when copies merge. One Undo step.",
            json!({"scenes":scenes}),
            &["scenes"],
        ),
        def(
            "merge_storyboard_revision",
            "Merge another artist's copy of this storyboard (a downloaded cloud revision, `path`) into the open board three ways against the version both started from (`base_path`). Panels match by ID: what only one side changed comes from that side; panels added on either side are kept beside their neighbours; a panel deleted on one side and unchanged on the other is deleted. Board data (scenes, captions per field, timing, transitions, cameras, audio and video clips, library, settings) merges the same way; review notes and board versions are unions and claims a latest-wins union. What both sides changed differently is a conflict with a key (panel:ID, order, group:ID, camera:SCENE, field:ID, audio:TRACK, video:TRACK, sound:ID, library:ID, board:NAME); without a choice it keeps mine. Run with dry_run first and show the person their changes and the conflicts, then apply with `resolutions` (mine, theirs, or for panels both: keep mine and theirs as a new panel after it). Applying is one Undo step; `revision` (the cloud revision ID merged) is recorded so the next save uploads a revision with both heads as parents.",
            json!({
                "path":file("Absolute path of the other copy (.emu), e.g. a downloaded cloud revision."),
                "base_path":file("Absolute path of the common version (.emu) both copies descend from."),
                "dry_run":{"type":"boolean","description":"Report their changes and the conflicts without changing the board."},
                "resolutions":{"type":"array","maxItems":4096,"items":{"type":"object","additionalProperties":false,"required":["conflict","take"],"properties":{
                    "conflict":{"type":"string","minLength":1,"maxLength":300,"description":"A conflict key from the dry run."},
                    "take":{"enum":["mine","theirs","both"]}
                }},"description":"Choices for conflicts; others keep mine."},
                "revision":{"type":"string","minLength":36,"maxLength":36,"description":"The cloud revision ID of `path`, recorded as the merge's second parent."}
            }),
            &["path", "base_path"],
        ),
    ]
}

fn report_json(report: &BoardMergeReport) -> Value {
    let changes = |list: &[emulsion_core::storyboard_changes::PanelChange]| -> Vec<Value> {
        list.iter()
            .filter(|c| c.is_change())
            .map(|c| json!({"panel":c.panel(),"name":c.name,"change":c.summary()}))
            .collect()
    };
    json!({
        "their_changes":changes(&report.theirs),
        "my_changes":changes(&report.ours),
        "conflicts":report.conflicts,
        "panels":report.panels,
        "frames":report.frames,
        "took_theirs":report.took_theirs,
        "renumbered":report.renumbered,
        "versions_added":report.versions_added,
    })
}

fn claimant(args: &Value) -> String {
    args["claimant"].as_str().map_or_else(
        || {
            emulsion_io::settings::Settings::load()
                .storyboard
                .review_author
                .trim()
                .to_string()
        },
        |s| s.trim().to_string(),
    )
}

fn cloud_json(editor: &ProjectEditor) -> Value {
    let Some(path) = editor.path.as_deref() else {
        return json!({"synced":false,"note":"The storyboard has not been saved."});
    };
    let store = emulsion_io::cloud::store();
    match emulsion_io::cloud::shared::sharing(&store, path, None) {
        Ok(Some(s)) => {
            let queued = store
                .read()
                .map(|i| {
                    i.jobs
                        .iter()
                        .filter(|j| j.revision.project == s.binding.project)
                        .count()
                })
                .unwrap_or(0);
            json!({
                "synced":true,
                "provider":s.binding.provider.label(),
                "paused":s.binding.paused,
                "saved_revision":s.binding.base,
                "queued_uploads":queued,
                "revisions":s.revisions,
                "this_device":s.device,
                "collaborators":s.collaborators.iter().map(|c| json!({"author":c.author,"device":c.device,"revisions":c.revisions,"last":c.last})).collect::<Vec<_>>(),
                "waiting":s.waiting.iter().map(|r| json!({"revision":r.revision.id,"author":r.revision.author,"device":r.revision.device,"created":r.revision.created})).collect::<Vec<_>>(),
            })
        }
        Ok(None) => json!({"synced":false,"note":"This file is not synced to a cloud account."}),
        Err(e) => json!({"synced":false,"error":e.to_string()}),
    }
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "describe_storyboard_sharing" => {
            let layout: Vec<_> = editor.page_list().iter().map(|m| m.id).collect();
            let claims: Vec<Value> = board
                .active_claims(&layout)
                .into_iter()
                .map(|c| {
                    json!({
                        "scene":c.scene,
                        "scene_name":board.scenes.get(&c.scene).map(|s| s.name.clone()),
                        "claimant":c.claimant,
                        "device":c.device,
                        "time":c.time,
                    })
                })
                .collect();
            Ok(json!({
                "project_id":board.project_id,
                "claims":claims,
                "merged_revision":board.sharing.merged_revision,
                "cloud":cloud_json(editor),
            }))
        }
        "claim_storyboard_scenes" => (|| {
            let scenes = super::ids(&args["scenes"]);
            let claimant = claimant(args);
            let device = args["device"]
                .as_str()
                .map_or_else(emulsion_io::cloud::shared::device_id, str::to_string);
            let now = emulsion_core::storyboard_review::now();
            editor.edit_storyboard(|b| b.claim_scenes(&scenes, &claimant, &device, now))?;
            Ok(json!({"scenes":scenes,"claimant":claimant}))
        })(),
        "release_storyboard_scenes" => (|| {
            let scenes = super::ids(&args["scenes"]);
            let now = emulsion_core::storyboard_review::now();
            let mut released = 0;
            editor.edit_storyboard(|b| {
                released = b.release_scenes(&scenes, now)?;
                Ok(())
            })?;
            Ok(json!({"released":released}))
        })(),
        "merge_storyboard_revision" => (|| {
            let read = |key: &str| -> Result<_, String> {
                let path = output(args, key, Some("emu"))?;
                emulsion_io::project::read(&path).map_err(|e| format!("{}: {e}", path.display()))
            };
            let theirs = read("path")?;
            let base = read("base_path")?;
            let mut resolutions = BTreeMap::new();
            for r in args["resolutions"].as_array().into_iter().flatten() {
                let text = r["conflict"].as_str().unwrap_or_default();
                let key = ConflictKey::parse(text)
                    .ok_or_else(|| format!("{text} is not a conflict key."))?;
                let take = match r["take"].as_str() {
                    Some("theirs") => Resolution::Theirs,
                    Some("both") => Resolution::Both,
                    _ => Resolution::Mine,
                };
                resolutions.insert(key, take);
            }
            if args["dry_run"] == true {
                let report = editor.plan_board_merge(&base, &theirs, &resolutions)?;
                return Ok(json!({"dry_run":true,"report":report_json(&report)}));
            }
            let report =
                editor.merge_board(&base, &theirs, &resolutions, args["revision"].as_str())?;
            Ok(json!({"merged":true,"report":report_json(&report)}))
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
    fn scenes_are_claimed_released_and_described() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"start":"scene","group_name":"Chase","panels":[{"frames":24}]}),
        );
        let ids = scenes(&mut e);
        let claimed = call(
            &mut e,
            "claim_storyboard_scenes",
            json!({"scenes":[ids[1]],"claimant":"Ravi","device":"ravi-device"}),
        );
        assert_eq!(claimed["claimant"], "Ravi");
        let shared = call(&mut e, "describe_storyboard_sharing", json!({}));
        assert_eq!(shared["claims"][0]["scene"], ids[1]);
        assert_eq!(shared["claims"][0]["scene_name"], "Chase");
        assert_eq!(shared["claims"][0]["claimant"], "Ravi");
        assert_eq!(shared["cloud"]["synced"], false);
        call(
            &mut e,
            "release_storyboard_scenes",
            json!({"scenes":[ids[1]]}),
        );
        let shared = call(&mut e, "describe_storyboard_sharing", json!({}));
        assert!(shared["claims"].as_array().unwrap().is_empty());
        assert!(e.undo(), "a release is one Undo step");
        let stamp = e.stamp();
        for (name, args) in [
            (
                "claim_storyboard_scenes",
                json!({"scenes":[999],"claimant":"Ravi","device":"d"}),
            ),
            (
                "claim_storyboard_scenes",
                json!({"scenes":[ids[0]],"claimant":" ","device":"d"}),
            ),
            ("release_storyboard_scenes", json!({"scenes":[ids[0]]})),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
    }

    #[test]
    fn a_downloaded_revision_merges_three_ways_after_a_dry_run() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{"frames":24},{"frames":24}]}),
        );
        let dir = Dir::new("sharing-merge");
        let (base_path, theirs_path) = (dir.path().join("base.emu"), dir.path().join("theirs.emu"));
        let base = e.snapshot().unwrap();
        emulsion_io::project::write(&base, &base_path).unwrap();
        let panels: Vec<u64> = e.page_list().iter().map(|m| m.id).collect();
        // Their copy: panel 2 longer, panel 3 longer; here panel 3 shorter.
        let mut theirs = ProjectEditor::open(base, None).unwrap();
        call(
            &mut theirs,
            "update_storyboard_panel",
            json!({"panel":panels[1],"frames":48}),
        );
        call(
            &mut theirs,
            "update_storyboard_panel",
            json!({"panel":panels[2],"frames":36}),
        );
        emulsion_io::project::write(&theirs.snapshot().unwrap(), &theirs_path).unwrap();
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":panels[2],"frames":12}),
        );
        let stamp = e.stamp();

        let dry = call(
            &mut e,
            "merge_storyboard_revision",
            json!({"path":theirs_path,"base_path":base_path,"dry_run":true}),
        );
        assert_eq!(e.stamp(), stamp, "a dry run changes nothing");
        let key = format!("panel:{}", panels[2]);
        assert_eq!(dry["report"]["conflicts"][0]["key"], key);
        assert_eq!(dry["report"]["conflicts"][0]["keep_both"], true);
        assert_eq!(dry["report"]["their_changes"].as_array().unwrap().len(), 2);
        let revision = "1b2c3d4e-0000-4000-8000-000000000001";
        let merged = call(
            &mut e,
            "merge_storyboard_revision",
            json!({"path":theirs_path,"base_path":base_path,"revision":revision,
                   "resolutions":[{"conflict":key,"take":"both"}]}),
        );
        assert_eq!(merged["report"]["panels"], 4);
        let shared = call(&mut e, "describe_storyboard_sharing", json!({}));
        assert_eq!(shared["merged_revision"], revision);
        let frames: Vec<u64> = {
            let b = e.storyboard().unwrap();
            e.page_list()
                .iter()
                .map(|m| u64::from(b.panels[&m.id].frames))
                .collect()
        };
        assert_eq!(
            frames,
            [48, 48, 12, 36],
            "theirs kept as a new panel after mine"
        );
        assert!(e.undo());
        assert_eq!(e.stamp(), stamp, "one Undo step");
        for args in [
            json!({"path":theirs_path,"base_path":base_path,"resolutions":[{"conflict":"nope","take":"mine"}]}),
            json!({"path":theirs_path,"base_path":base_path,"resolutions":[{"conflict":"panel:1","take":"mine"}]}),
            json!({"path":dir.path().join("missing.emu"),"base_path":base_path}),
        ] {
            assert!(
                execute(&mut e, "merge_storyboard_revision", &args).is_error,
                "{args}"
            );
            assert_eq!(e.stamp(), stamp);
        }
    }
}
