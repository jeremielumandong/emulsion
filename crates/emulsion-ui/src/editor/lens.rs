//! Lens profiles: read what the picture says about its camera and lens,
//! look the lens up in the lensfun database, and add a `Lens profile`
//! filter with its measured distortion and vignetting to the selected
//! pixel node (made a smart layer first, so the correction stays editable).

use super::*;
use emulsion_filters::Filter;
use emulsion_io::lensfun;

impl EditorView {
    /// Correct the selected node's lens from its EXIF, or say what is missing.
    pub fn lens_profile_auto(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            self.set_status(t!("editor.lens.select_layer"), false, cx);
            return;
        };
        let kind = self.editor.doc.node(id).map(|n| n.kind.tag());
        if !matches!(kind, Some("pixels") | Some("smart")) {
            self.set_status(t!("editor.lens.pixel_only"), false, cx);
            return;
        }
        let Some(info) = self.editor.doc.info.clone() else {
            self.set_status(t!("editor.lens.no_exif"), true, cx);
            return;
        };
        if !lensfun::installed() {
            self.set_status(t!("editor.lens.install_db"), true, cx);
            return;
        }
        self.set_status(t!("editor.lens.looking_up"), false, cx);
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let db = lensfun::Database::load().map_err(|e| e.to_string())?;
                    lensfun::profile_for(
                        &db,
                        &info.make,
                        &info.model,
                        &info.lens,
                        info.focal_mm,
                        info.f_number,
                    )
                    .ok_or_else(|| {
                        t!(
                            "editor.lens.no_profile",
                            lens = if info.lens.is_empty() {
                                t!("editor.lens.this_lens").into_owned()
                            } else {
                                info.lens.clone()
                            },
                            make = info.make,
                            model = info.model
                        )
                        .into_owned()
                    })
                })
                .await;
            this.update(cx, |this, cx| match found {
                Ok(p) => this.apply_lens_profile(id, &p, cx),
                Err(e) => this.set_status(e, true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn apply_lens_profile(
        &mut self,
        id: NodeId,
        p: &lensfun::Profile,
        cx: &mut Context<Self>,
    ) {
        let [a, b, c] = p.distortion.unwrap_or([0.0; 3]);
        let [k1, k2, k3] = p.vignetting.unwrap_or([0.0; 3]);
        let f = Filter::LensProfile {
            a,
            b,
            c,
            k1,
            k2,
            k3,
            scale: p.scale,
            distortion: 100.0,
            vignette: 100.0,
        };
        self.editor.begin("Lens profile");
        self.set_layer_selection(vec![id], Some(id));
        if self.editor.doc.node(id).map(|n| n.kind.tag()) == Some("pixels") {
            self.convert_smart(cx);
        }
        self.add_filter(id, f, cx);
        self.editor.end();
        let what = match (p.distortion.is_some(), p.vignetting.is_some()) {
            (true, true) => t!("editor.lens.corrected_both"),
            (true, false) => t!("editor.lens.corrected_distortion"),
            (false, true) => t!("editor.lens.corrected_vignetting"),
            (false, false) => t!("editor.lens.corrected_nothing"),
        };
        self.set_status(
            t!("editor.lens.corrected", lens = p.lens, what = what),
            false,
            cx,
        );
    }
}
