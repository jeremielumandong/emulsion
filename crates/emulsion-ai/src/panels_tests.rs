use super::*;
use std::sync::Mutex;

/// Models stand-in: the subject is the left half, upscale repeats pixels,
/// fill paints a solid block over the hole's bounds. Records prompts.
#[derive(Default)]
struct Stub {
    missing: Option<String>,
    prompts: Mutex<Vec<Option<String>>>,
    /// Cancel this job when the fill is called this many times.
    cancel_after: Option<(usize, Arc<Job>)>,
    fail_named: Option<String>,
}

impl Backend for Stub {
    fn ready(&self, _: &Op) -> Result<(), String> {
        self.missing.clone().map_or(Ok(()), Err)
    }
    fn matte(&self, image: &Raster, _: &Job) -> Result<Mask, String> {
        let half = image.width() / 2;
        Ok(Mask::from_fn(
            image.width(),
            image.height(),
            0,
            move |x, _| {
                if x < half { 255 } else { 0 }
            },
        ))
    }
    fn upscale_factor(&self) -> u32 {
        4
    }
    fn upscale(&self, image: &Raster, _: &Job) -> Result<Raster, String> {
        let (w, h) = (image.width() as usize * 4, image.height() as usize * 4);
        Ok(Planes::from_raster(image).resized(w, h).to_raster())
    }
    fn denoise(&self, image: &Raster, _: &Job) -> Result<Raster, String> {
        Ok(image.clone())
    }
    fn fill(
        &self,
        image: &Raster,
        hole: &Mask,
        prompt: Option<&str>,
        _: &Job,
    ) -> Result<(Raster, IRect), String> {
        let mut prompts = self.prompts.lock().unwrap();
        prompts.push(prompt.map(str::to_string));
        if let Some((n, job)) = &self.cancel_after
            && prompts.len() >= *n
        {
            job.cancel();
        }
        if let (Some(fail), Some(p)) = (&self.fail_named, prompt)
            && p == fail
        {
            return Err("the image server said: HTTP 500".into());
        }
        let b = select::bounds(hole).intersect(&image.bounds());
        let green = emulsion_raster::color::f_to_px([0., 1., 0., 1.]);
        let layer = Raster::from_fn(b.w as u32, b.h as u32, [0; 4], |x, y| {
            if hole.get(x + b.x as u32, y + b.y as u32) > 127 {
                green
            } else {
                [0; 4]
            }
        });
        Ok((layer, b))
    }
    fn model_id(&self, _: &Op) -> String {
        "stub".into()
    }
}

/// A 32×18 panel: white Background, and a 8×6 "Pose" layer shown at 2×.
fn panel() -> Document {
    let mut doc = Document::new(32, 18);
    for (name, raster, placement) in [
        (
            "Background",
            Raster::solid(32, 18, [1., 1., 1., 1.]),
            Placement::default(),
        ),
        (
            "Pose",
            Raster::solid(8, 6, [1., 0., 0., 1.]),
            Placement {
                scale_x: 2.,
                scale_y: 2.,
                ..Placement::at(4., 2.)
            },
        ),
    ] {
        Command::AddNode {
            node: Box::new(Node::raster(0, name, Arc::new(raster), placement)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    doc
}

fn id(doc: &Document, name: &str) -> NodeId {
    doc.nodes.iter().find(|n| n.name == name).unwrap().id
}

fn apply(doc: &Document, planned: &Planned) -> Document {
    let mut doc = doc.clone();
    for c in &planned.commands {
        c.apply(&mut doc).unwrap();
    }
    doc
}

fn request(op: Op, target: Target) -> Request {
    Request { op, target }
}

#[test]
fn subject_masks_become_a_selection_a_layer_mask_or_a_cut_out() {
    let doc = panel();
    let stub = Stub::default();
    let job = Job::new();
    let pose = id(&doc, "Pose");

    let sel = plan(
        &doc,
        &request(Op::Subject(MaskOutput::Selection), Target::Panel),
        &stub,
        &job,
    )
    .unwrap();
    let after = apply(&doc, &sel);
    assert_eq!(
        select::bounds(after.selection.as_deref().unwrap()),
        IRect::new(0, 0, 16, 18)
    );

    let masked = apply(
        &doc,
        &plan(
            &doc,
            &request(Op::Subject(MaskOutput::LayerMask), Target::Layer(pose)),
            &stub,
            &job,
        )
        .unwrap(),
    );
    let mask = masked.node(pose).unwrap().mask.clone().unwrap();
    assert_eq!(
        (mask.width(), mask.height()),
        (8, 6),
        "in the layer's pixels"
    );
    let again = plan(
        &masked,
        &request(Op::Subject(MaskOutput::LayerMask), Target::Layer(pose)),
        &stub,
        &job,
    );
    assert!(again.unwrap_err().contains("already has a layer mask"));

    let cut = apply(
        &doc,
        &plan(
            &doc,
            &request(
                Op::Subject(MaskOutput::CutOut),
                Target::Named("pose".into()),
            ),
            &stub,
            &job,
        )
        .unwrap(),
    );
    let new = cut.nodes.iter().find(|n| n.name == "Pose cut-out").unwrap();
    assert_eq!(new.origin.as_deref(), Some("ai:stub"));
    assert!(!cut.node(pose).unwrap().visible, "the original is hidden");
    let order = cut.children(None);
    assert_eq!(
        order.iter().position(|n| *n == new.id),
        order.iter().position(|n| *n == pose).map(|i| i + 1),
        "the cut-out sits right above its source"
    );
}

#[test]
fn upscale_keeps_the_panel_size_and_stores_more_detail() {
    let doc = panel();
    let pose = id(&doc, "Pose");
    let planned = plan(
        &doc,
        &request(Op::Upscale, Target::Layer(pose)),
        &Stub::default(),
        &Job::new(),
    )
    .unwrap();
    let after = apply(&doc, &planned);
    assert_eq!(
        (after.width, after.height),
        (32, 18),
        "the frame never grows"
    );
    let new = after.nodes.last().unwrap();
    let NodeKind::Raster { raster, placement } = &new.kind else {
        panic!("a pixel layer")
    };
    // Shown at 2×, so 4× (the model's most) keeps it sharp under a push-in.
    assert_eq!((raster.width(), raster.height()), (32, 24));
    assert_eq!((placement.scale_x, placement.scale_y), (0.5, 0.5));
    assert_eq!((placement.x, placement.y), (4., 2.));
    assert!(!after.node(pose).unwrap().visible);
    // A layer at its own size keeps twice its pixels.
    assert_eq!(detail_factor(&Placement::default(), 4), 2.0);
    assert_eq!(detail_factor(&Placement::default(), 2), 2.0);
}

#[test]
fn expand_shrinks_inside_the_frame_and_fills_the_border() {
    let doc = panel();
    let stub = Stub::default();
    let planned = plan(
        &doc,
        &request(
            Op::Expand {
                amount: 0.25,
                prompt: Some("more city".into()),
            },
            Target::Panel,
        ),
        &stub,
        &Job::new(),
    )
    .unwrap();
    let after = apply(&doc, &planned);
    assert_eq!((after.width, after.height), (32, 18));
    let new = after.nodes.last().unwrap();
    assert_eq!(new.name, "Panel expanded");
    let NodeKind::Raster { raster, .. } = &new.kind else {
        panic!()
    };
    assert_eq!((raster.width(), raster.height()), (32, 18));
    // The corner is the fill (green); the centre is the shrunk picture.
    let corner = emulsion_raster::color::px_to_f(raster.get(0, 0));
    assert!(corner[1] > 0.9 && corner[0] < 0.1, "{corner:?}");
    let centre = emulsion_raster::color::px_to_f(raster.get(16, 9));
    assert!(centre[0] > 0.9, "{centre:?}");
    assert_eq!(
        *stub.prompts.lock().unwrap(),
        vec![Some("more city".to_string())]
    );
}

#[test]
fn fill_keeps_the_original_and_finds_its_area() {
    let mut doc = panel();
    let stub = Stub::default();
    let job = Job::new();
    let fill = |area, prompt: Option<&str>| Op::Fill {
        prompt: prompt.map(str::to_string),
        area,
    };
    let none = plan(
        &doc,
        &request(fill(Area::Selection, Some("a tree")), Target::Panel),
        &stub,
        &job,
    );
    assert_eq!(none.unwrap_err(), "This panel has no selection.");
    assert!(
        plan(
            &doc,
            &request(fill(Area::Whole, None), Target::Panel),
            &stub,
            &job
        )
        .unwrap_err()
        .contains("needs a prompt")
    );
    // The named layer's pixels: Pose covers 4,2 16×12 on the panel, and
    // scaling it 2× leaves a partly covered pixel around that.
    let planned = plan(
        &doc,
        &request(fill(Area::Layer("Pose".into()), None), Target::Panel),
        &stub,
        &job,
    )
    .unwrap();
    let after = apply(&doc, &planned);
    let new = after.nodes.last().unwrap();
    assert_eq!(new.name, "AI fill");
    let NodeKind::Raster { raster, placement } = &new.kind else {
        panic!()
    };
    assert_eq!(
        (placement.x, placement.y, raster.width(), raster.height()),
        (3., 1., 18, 14)
    );
    assert!(after.nodes.iter().all(|n| n.visible), "nothing is hidden");

    doc.selection = Some(Arc::new(Mask::from_fn(32, 18, 0, |x, y| {
        if (2..6).contains(&x) && (3..5).contains(&y) {
            255
        } else {
            0
        }
    })));
    let planned = plan(
        &doc,
        &request(
            fill(Area::Selection, Some("a red barn at dusk")),
            Target::Panel,
        ),
        &stub,
        &job,
    )
    .unwrap();
    assert!(planned.summary.contains("Generated: a red barn at dusk"));
    assert_eq!(
        stub.prompts
            .lock()
            .unwrap()
            .last()
            .cloned()
            .flatten()
            .as_deref(),
        Some("a red barn at dusk"),
        "the prompt reaches the provider unchanged"
    );
}

fn inputs(n: usize) -> Vec<PanelInput> {
    (1..=n as u64)
        .map(|i| {
            let mut doc = panel();
            if i == 2 {
                let pose = id(&doc, "Pose");
                doc.nodes.retain(|n| n.id != pose);
            }
            PanelInput {
                panel: i,
                name: format!("Panel {i}"),
                doc,
                revision: 0,
            }
        })
        .collect()
}

#[test]
fn batches_report_each_panel_and_stop_on_cancel() {
    let stub = Stub::default();
    let job = Job::new();
    let req = request(Op::Upscale, Target::Named("Pose".into()));
    let report = run(&inputs(3), &req, &stub, &job).unwrap();
    assert!(!report.cancelled);
    assert_eq!(
        report.edits().keys().copied().collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(
        report.errors(),
        vec![(2, "Panel 2".into(), "No layer named “Pose”.".into())]
    );
    assert!((job.fraction() - 1.0).abs() < 1e-3);

    // Cancel while the second panel's model runs: it is dropped, the
    // third never starts.
    let job = Job::new();
    let stub = Stub {
        cancel_after: Some((2, job.clone())),
        ..Default::default()
    };
    let req = request(
        Op::Fill {
            prompt: Some("rain".into()),
            area: Area::Whole,
        },
        Target::Panel,
    );
    let report = run(&inputs(3), &req, &stub, &job).unwrap();
    assert!(report.cancelled);
    assert_eq!(report.results.len(), 1);
    assert_eq!(stub.prompts.lock().unwrap().len(), 2);

    // A provider error on one panel does not stop the others.
    let stub = Stub {
        fail_named: Some("rain".into()),
        ..Default::default()
    };
    let report = run(&inputs(2), &req, &stub, &Job::new()).unwrap();
    assert_eq!(report.errors().len(), 2);
    assert!(report.errors()[0].2.contains("HTTP 500"));
}

#[test]
fn nothing_runs_when_the_model_or_provider_is_missing() {
    let stub = Stub {
        missing: Some("Needs RMBG: install it under Settings › Local models.".into()),
        ..Default::default()
    };
    let req = request(Op::Subject(MaskOutput::CutOut), Target::Panel);
    let error = run(&inputs(2), &req, &stub, &Job::new()).unwrap_err();
    assert!(error.contains("Settings › Local models"));
    assert!(stub.prompts.lock().unwrap().is_empty());

    // The real backend: prompts need a configured provider, and nothing
    // is sent without one.
    let models = Models::default();
    let prompted = Op::Fill {
        prompt: Some("a dog".into()),
        area: Area::Whole,
    };
    assert_eq!(models.ready(&prompted).unwrap_err(), NO_PROVIDER);
    let keyless = Models {
        image: Some(generate::Config {
            provider: generate::Provider::OpenAi,
            endpoint: None,
            model: None,
            api_key: None,
        }),
    };
    assert!(keyless.ready(&prompted).unwrap_err().contains("API key"));
    // Local operations name the model to install when it is absent.
    if matte::available().is_none() {
        let e = models
            .ready(&Op::Subject(MaskOutput::Selection))
            .unwrap_err();
        assert!(e.contains("Settings › Local models"), "{e}");
    }
    if inpaint::available().is_none() {
        let e = models
            .ready(&Op::Fill {
                prompt: None,
                area: Area::Selection,
            })
            .unwrap_err();
        assert!(e.contains("Settings › Local models"), "{e}");
    }
    assert_eq!(
        Op::Fill {
            prompt: Some("  ".into()),
            area: Area::Whole
        }
        .normalized()
        .prompt(),
        None
    );
}

#[test]
fn the_prompt_and_the_panel_go_to_the_configured_provider() {
    let reply = image::RgbaImage::from_pixel(32, 18, image::Rgba([0, 0, 255, 255]));
    let (cfg, server) =
        generate::google::tests::server(generate::google::tests::image_reply(&reply));
    let models = Models { image: Some(cfg) };
    let mut doc = panel();
    doc.selection = Some(Arc::new(Mask::white(32, 18)));
    let req = request(
        Op::Fill {
            prompt: Some("storm clouds over the bay".into()),
            area: Area::Selection,
        },
        Target::Panel,
    );
    let report = run(
        &[PanelInput {
            panel: 7,
            name: "Shot 7".into(),
            doc,
            revision: 0,
        }],
        &req,
        &models,
        &Job::new(),
    )
    .unwrap();
    assert!(report.errors().is_empty(), "{:?}", report.errors());
    let (_, body) = server.join().unwrap();
    let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
    assert!(text.contains("storm clouds over the bay"), "{text}");
    let new = &report.edits()[&7];
    let Command::AddNode { node, .. } = &new[0] else {
        panic!()
    };
    assert!(node.origin.as_deref().unwrap().starts_with("ai:google/"));
}

fn storyboard(panels: usize) -> ProjectEditor {
    use emulsion_core::project::ProjectKind;
    use emulsion_core::storyboard::Panel;
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, panel()).unwrap();
    let first = p.page_list()[0].id;
    let items = (2..=panels)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(first), &panel(), items, None).unwrap();
    p
}

#[test]
fn a_batch_lands_as_one_undo_step_and_skips_locked_or_changed_panels() {
    let mut p = storyboard(4);
    let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
    p.edit_storyboard(|b| {
        b.panels.get_mut(&ids[3]).unwrap().locked = true;
        Ok(())
    })
    .unwrap();
    let before: Vec<usize> = ids
        .iter()
        .map(|id| p.page(*id).unwrap().doc.nodes.len())
        .collect();
    let (inputs, skipped) = gather(&p, &ids).unwrap();
    assert_eq!(inputs.len(), 3);
    assert_eq!(
        skipped,
        vec![(ids[3], "Panel 4".into(), "Locked panel.".into())]
    );
    let req = request(Op::Denoise, Target::Named("Pose".into()));
    let report = run(&inputs, &req, &Stub::default(), &Job::new()).unwrap();
    // Someone draws on panel 2 while the batch runs.
    p.set_active_page(ids[1]).unwrap();
    p.execute(Command::Rename {
        id: id(&p.doc, "Background"),
        name: "Paper".into(),
    })
    .unwrap();
    let applied = commit(&mut p, &inputs, report, skipped, req.op.label()).unwrap();
    assert_eq!(
        applied.done.iter().map(|d| d.0).collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert_eq!(applied.failed.len(), 2);
    let line = applied.summary("Denoise");
    assert!(line.contains("2 panels changed, 2 panels failed"), "{line}");
    assert_eq!(p.page(ids[0]).unwrap().doc.nodes.len(), before[0] + 1);
    assert_eq!(p.page(ids[2]).unwrap().doc.nodes.len(), before[2] + 1);
    // One Undo takes the whole batch back.
    assert!(p.undo());
    assert_eq!(p.page(ids[0]).unwrap().doc.nodes.len(), before[0]);
    assert_eq!(p.page(ids[2]).unwrap().doc.nodes.len(), before[2]);
    assert_eq!(
        p.page(ids[1]).unwrap().doc.nodes[0].name,
        "Paper",
        "the rename stays"
    );

    // A cancelled batch changes nothing.
    let report = Report {
        cancelled: true,
        ..Report::default()
    };
    let applied = commit(&mut p, &[], report, Vec::new(), "Denoise").unwrap();
    assert!(applied.cancelled && applied.done.is_empty());
    assert_eq!(
        applied.summary("Denoise"),
        "Denoise cancelled: no panel was changed."
    );
}
