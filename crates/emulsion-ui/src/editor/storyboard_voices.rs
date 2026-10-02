//! Scratch voices and dialogue (AI8, AI9): the **Voice cast** dialog (each
//! character in the Dialogue captions with a Piper or eSpeak NG voice, rate,
//! pitch and Preview), **Generate scratch dialogue** (the selection, the
//! active scene or the whole board, optionally lengthening panels to fit),
//! and the clip menu's **Enhance dialogue…** and **Regenerate line…**, all
//! reached from the Timeline's Timing menu and clip menu. Speech and
//! enhancement run on this computer through `emulsion_io::voices` and
//! `emulsion_io::audio::enhance`, off the UI thread with a progress card
//! that can cancel; each result lands as one Undo step through
//! `edit_board`.
use super::storyboard_player::Device;
use super::storyboard_timeline::menu_item;
use super::*;
use crate::playback::audio_out::Mix;
use emulsion_ai::jobs::Job;
use emulsion_core::project::PageId;
use emulsion_core::storyboard_voices::{
    Delivery, Emphasis, MAX_PITCH, PlannedLine, RATE_RANGE, ScratchLine, Voice, VoiceEngine,
    character_key,
};
use emulsion_core::timeline::audio::AssetId;
use emulsion_core::timeline::{AudioAsset, AudioClip, AudioTrack, FrameRate, Timeline};
use emulsion_io::voices::{self, Config, EspeakVoice, PiperModel};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::BTreeMap;

/// Where voices come from, as Settings › Storyboard says.
pub(crate) fn voice_config(cx: &App) -> Config {
    Config::from_preferences(&crate::app_state::settings(cx).storyboard)
}

/// Which panels Generate scratch dialogue speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScratchScope {
    Selection,
    Scene,
    Board,
}

type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A label, − value + row.
fn stepper(id: &str, label: &str, value: String, p: &Palette, down: Click, up: Click) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(mono(label.to_string(), 10., p.muted))
        .child(
            Button::new(SharedString::from(format!("{id}-down")))
                .label("−")
                .xsmall()
                .ghost()
                .on_click(down),
        )
        .child(
            div()
                .id(SharedString::from(format!("{id}-value")))
                .test_support()
                .min_w(px(38.))
                .text_size(px(11.))
                .child(value),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-up")))
                .label("+")
                .xsmall()
                .ghost()
                .on_click(up),
        )
}

fn round(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

fn step_rate(rate: f32, dir: i32) -> f32 {
    round(rate + 0.05 * dir as f32, 0.05).clamp(*RATE_RANGE.start(), *RATE_RANGE.end())
}

impl EditorView {
    /// The playing panels `scope` covers, in board order.
    pub(crate) fn scratch_scope_panels(&self, scope: ScratchScope) -> Vec<PageId> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let playing: Vec<PageId> = board
            .playing(&layout)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        match scope {
            ScratchScope::Board => playing,
            ScratchScope::Selection => {
                let chosen = self.board_selection();
                playing
                    .into_iter()
                    .filter(|id| chosen.contains(id))
                    .collect()
            }
            ScratchScope::Scene => {
                let scene = board
                    .panels
                    .get(&self.editor.active_page())
                    .map(|p| p.scene);
                playing
                    .into_iter()
                    .filter(|id| Some(board.panels[id].scene) == scene)
                    .collect()
            }
        }
    }

    /// Run `work` off the UI thread under a progress card that can cancel,
    /// then `done` with its result. With `edits`, a board changed in the
    /// meantime drops the result.
    fn voice_job<T: Send + 'static>(
        &mut self,
        title: &str,
        edits: bool,
        work: impl FnOnce(&Job) -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Arc<Job> {
        let ticket = self.edit_ticket();
        let job = Job::new();
        job.set_stage(title);
        self.watch_job(job.clone(), title, cx);
        let worker = job.clone();
        let watched = job.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = work(&worker);
                    worker.finish();
                    result
                })
                .await;
            this.update(cx, |this, cx| match result {
                _ if watched.cancelled() => this.set_status("Canceled.", false, cx),
                Err(error) => this.set_status(format!("{error:#}"), true, cx),
                Ok(_) if edits && this.edit_ticket() != ticket => this.set_status(
                    "The storyboard changed while this ran. Try again.",
                    false,
                    cx,
                ),
                Ok(value) => done(this, value, cx),
            })
            .ok();
        })
        .detach();
        job
    }

    /// Speak the Dialogue captions of `panels` and lay the takes on the
    /// Scratch dialogue track, replacing earlier scratch takes of those
    /// panels, as one Undo step.
    pub(crate) fn generate_scratch_dialogue(
        &mut self,
        panels: Vec<PageId>,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Job>> {
        let board = self.editor.storyboard()?;
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let plan = board.scratch_plan(&layout, &panels);
        if plan.is_empty() {
            self.set_status(
                "No dialogue to speak there. Write lines like “MIA: Hello.” in the Dialogue caption.",
                true,
                cx,
            );
            return None;
        }
        let config = voice_config(cx);
        let job = self.voice_job(
            "Generating scratch dialogue",
            true,
            move |job| {
                voices::takes(&config, plan, job.cancel_flag(), |done, total| {
                    job.progress(done as f32 / total.max(1) as f32);
                    job.set_stage(format!(
                        "Speaking line {} of {total}",
                        (done + 1).min(total)
                    ));
                })
            },
            move |this, takes, cx| {
                if !this.prepare_page_action(cx) {
                    return;
                }
                let mut report = None;
                if this.edit_board(
                    |b| {
                        report = Some(b.apply_scratch(&layout, &panels, takes, extend)?);
                        Ok(())
                    },
                    cx,
                ) && let Some(report) = report
                {
                    let mut message = format!("Placed {} scratch line(s)", report.placed);
                    if !report.extended.is_empty() {
                        message += &format!("; {} panel(s) lengthened", report.extended.len());
                    }
                    if !report.locked.is_empty() {
                        message += &format!(
                            "; {} locked panel(s) are shorter than their lines",
                            report.locked.len()
                        );
                    }
                    this.timeline_ui.open = true;
                    this.set_status(format!("{message}."), false, cx);
                }
            },
            cx,
        );
        Some(job)
    }

    /// Enhance the dialogue of the clip at `at` into a new sound; the clip
    /// plays it, or with `new_track` a copy does on the Enhanced dialogue
    /// track. One Undo step; the original sound stays in the library.
    pub(crate) fn enhance_dialogue_clip(
        &mut self,
        at: (usize, usize),
        new_track: bool,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Job>> {
        let board = self.editor.storyboard()?;
        let source = board
            .timeline
            .tracks
            .get(at.0)
            .and_then(|t| t.clips.get(at.1))
            .and_then(|c| board.timeline.assets.get(&c.asset))
            .and_then(|a| a.source.clone());
        let Some(source) = source else {
            self.set_status("That clip's sound has no data to enhance.", true, cx);
            return None;
        };
        Some(self.voice_job(
            "Enhancing dialogue",
            true,
            move |job| emulsion_io::audio::enhance::enhance(&source, job.cancel_flag()),
            move |this, asset, cx| {
                if !this.prepare_page_action(cx) {
                    return;
                }
                let mut placed = None;
                if this.edit_board(
                    |b| {
                        placed = Some(b.place_enhanced(at, asset, new_track)?);
                        Ok(())
                    },
                    cx,
                ) && let Some(placed) = placed
                {
                    this.timeline_ui.track = Some(placed.0);
                    this.timeline_ui.clip = Some(placed);
                    this.set_status(
                        "Enhanced the dialogue; the original sound is still in the library.",
                        false,
                        cx,
                    );
                }
            },
            cx,
        ))
    }

    /// The line `asset` says, ready to speak with its cast (or default)
    /// voice and `delivery`.
    fn planned_line(&self, asset: AssetId, delivery: Delivery) -> Option<PlannedLine> {
        let board = self.editor.storyboard()?;
        let line = board.voices.lines.get(&asset)?;
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let key = character_key(&line.character);
        Some(PlannedLine {
            panel: line.panel,
            line: line.line,
            character: line.character.clone(),
            text: line.text.clone(),
            voice: board.voices.voice(&line.character).cloned(),
            cast_index: board
                .characters(&layout)
                .iter()
                .position(|c| character_key(c) == key)
                .unwrap_or(0),
            delivery,
        })
    }

    /// Speak scratch line `asset` again with `delivery`; its clips play the
    /// new take and the earlier take stays in the library. One Undo step.
    pub(crate) fn regenerate_scratch_line(
        &mut self,
        asset: AssetId,
        delivery: Delivery,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Job>> {
        let Some(line) = self.planned_line(asset, delivery) else {
            self.set_status("That clip is not a scratch line.", true, cx);
            return None;
        };
        let config = voice_config(cx);
        Some(self.voice_job(
            "Regenerating the line",
            true,
            move |job| {
                let voice = voices::voice_for(&config, &line)?;
                voices::take(&config, &voice, &delivery, &line.text, job.cancel_flag())
            },
            move |this, take, cx| {
                if !this.prepare_page_action(cx) {
                    return;
                }
                if this.edit_board(
                    |b| b.replace_scratch_take(asset, take, delivery).map(|_| ()),
                    cx,
                ) {
                    this.set_status("Regenerated the line.", false, cx);
                }
            },
            cx,
        ))
    }

    /// Speak `text` in `voice` (the default voice for the `cast_index`th
    /// character when `None`) and play it.
    pub(crate) fn preview_voice(
        &mut self,
        voice: Option<Voice>,
        cast_index: usize,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let config = voice_config(cx);
        let line = PlannedLine {
            panel: 0,
            line: 0,
            character: String::new(),
            text,
            voice,
            cast_index,
            delivery: Delivery::default(),
        };
        self.voice_job(
            "Previewing the voice",
            false,
            move |job| {
                let voice = voices::voice_for(&config, &line)?;
                voices::take(
                    &config,
                    &voice,
                    &line.delivery,
                    &line.text,
                    job.cancel_flag(),
                )
            },
            |this, asset, cx| this.play_sound(asset, cx),
            cx,
        );
    }

    /// Play a sound once through the audio output.
    fn play_sound(&mut self, asset: AudioAsset, cx: &mut Context<Self>) {
        let Device::Open(output) = &self.player.device else {
            self.playback_open_device(false, cx);
            self.set_status("Opening the sound output… press Preview again.", false, cx);
            return;
        };
        let output = output.clone();
        let rate = FrameRate::whole(24);
        let mut timeline = Timeline::default();
        timeline.tracks.push(AudioTrack::new("Preview"));
        let frames = rate
            .seconds_to_frames(asset.duration_ms as f64 / 1000.)
            .max(1);
        let Ok(id) = timeline.add_asset(asset) else {
            return;
        };
        let clip = AudioClip {
            asset: id,
            name: "Preview".into(),
            frames,
            ..AudioClip::default()
        };
        if timeline.place(0, clip).is_err() {
            return;
        }
        let len = emulsion_io::audio::mix::frame_sample(rate, timeline.end());
        let mix = Mix::new(
            emulsion_io::audio::RATE,
            len,
            Box::new(move |first, count| {
                emulsion_io::audio::mix::mix_samples(&timeline, rate, first, count)
                    .map_err(|e| format!("{e:#}"))
            }),
        );
        output.grain(mix, 0, len);
    }

    /// Replace the board's voice cast, as one Undo step.
    pub(crate) fn set_voice_cast(
        &mut self,
        cast: BTreeMap<String, Voice>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.prepare_page_action(cx)
            && self.edit_board(
                |b| {
                    b.voices.voices = cast;
                    Ok(())
                },
                cx,
            )
    }

    fn open_voice_dialog<V: Render>(
        &mut self,
        title: &'static str,
        width: f32,
        dialog: Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.open_dialog(cx, move |d, _, _| {
            d.title(title).width(px(width)).child(dialog.clone())
        });
    }

    /// Timing › Voice cast…
    pub(crate) fn open_voice_cast(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<VoiceCast>> {
        let board = self.editor.storyboard()?;
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let rows: Vec<CastRow> = board
            .characters(&layout)
            .into_iter()
            .map(|character| CastRow {
                voice: board.voices.voice(&character).cloned(),
                character,
            })
            .collect();
        let config = voice_config(cx);
        let editor = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            let mut dialog = VoiceCast {
                editor,
                rows,
                models: config.models(),
                config,
                espeak: None,
                message: None,
            };
            dialog.load_espeak(cx);
            dialog
        });
        self.open_voice_dialog("Voice cast", 640., dialog.clone(), window, cx);
        Some(dialog)
    }

    /// Timing › Generate scratch dialogue…
    pub(crate) fn open_scratch_dialogue(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ScratchDialog>> {
        self.editor.storyboard()?;
        let editor = cx.entity().downgrade();
        let scope = if self.board_selection().len() > 1 {
            ScratchScope::Selection
        } else {
            ScratchScope::Board
        };
        let dialog = cx.new(|_| ScratchDialog {
            editor,
            scope,
            extend: true,
        });
        self.open_voice_dialog(
            "Generate scratch dialogue",
            480.,
            dialog.clone(),
            window,
            cx,
        );
        Some(dialog)
    }

    /// Clip menu › Enhance dialogue…
    pub(crate) fn open_enhance_dialogue(
        &mut self,
        at: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EnhanceDialog> {
        let editor = cx.entity().downgrade();
        let dialog = cx.new(|_| EnhanceDialog {
            editor,
            at,
            new_track: false,
        });
        self.open_voice_dialog("Enhance dialogue", 460., dialog.clone(), window, cx);
        dialog
    }

    /// Clip menu › Regenerate line…
    pub(crate) fn open_regenerate_line(
        &mut self,
        asset: AssetId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<RegenerateDialog>> {
        let line = self.editor.storyboard()?.voices.lines.get(&asset)?.clone();
        let editor = cx.entity().downgrade();
        let dialog = cx.new(|_| RegenerateDialog {
            editor,
            asset,
            delivery: line.delivery,
            line,
        });
        self.open_voice_dialog("Regenerate line", 480., dialog.clone(), window, cx);
        Some(dialog)
    }

    /// The Timing menu's voice items.
    pub(super) fn timeline_voice_items(menu: PopupMenu, owner: &WeakEntity<Self>) -> PopupMenu {
        menu.separator()
            .item(menu_item(owner, "Voice cast…", |e, window, cx| {
                e.open_voice_cast(window, cx);
            }))
            .item(menu_item(
                owner,
                "Generate scratch dialogue…",
                |e, window, cx| {
                    e.open_scratch_dialogue(window, cx);
                },
            ))
    }

    /// The clip menu's dialogue items.
    pub(super) fn timeline_clip_dialogue_items(
        menu: PopupMenu,
        owner: &WeakEntity<Self>,
        at: (usize, usize),
        clip: &AudioClip,
        scratch: bool,
    ) -> PopupMenu {
        let asset = clip.asset;
        let menu = menu.item(menu_item(
            owner,
            "Enhance dialogue…",
            move |e, window, cx| {
                e.open_enhance_dialogue(at, window, cx);
            },
        ));
        if scratch {
            menu.item(menu_item(
                owner,
                "Regenerate line…",
                move |e, window, cx| {
                    e.open_regenerate_line(asset, window, cx);
                },
            ))
        } else {
            menu
        }
    }
}

/// One character in the Voice cast dialog.
struct CastRow {
    character: String,
    /// `None` speaks with the default voice.
    voice: Option<Voice>,
}

/// eSpeak NG languages and variants.
type EspeakLists = (Vec<EspeakVoice>, Vec<EspeakVoice>);

/// The Voice cast dialog.
pub(crate) struct VoiceCast {
    editor: WeakEntity<EditorView>,
    rows: Vec<CastRow>,
    config: Config,
    models: Vec<PiperModel>,
    /// eSpeak NG languages and variants, or why they are missing; `None`
    /// while listing.
    espeak: Option<Result<EspeakLists, String>>,
    message: Option<String>,
}

impl VoiceCast {
    fn load_espeak(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    if !voices::espeak_available() {
                        return Err("eSpeak NG is not installed.".to_string());
                    }
                    voices::espeak_voices()
                        .and_then(|l| Ok((l, voices::espeak_variants()?)))
                        .map_err(|e| e.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                this.espeak = Some(listed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Row `i`'s voice, made concrete from the default when it has none.
    fn voice_mut(&mut self, i: usize) -> Option<&mut Voice> {
        if self.rows[i].voice.is_none() {
            match self.config.default_voice(i) {
                Ok(voice) => self.rows[i].voice = Some(voice),
                Err(error) => {
                    self.message = Some(error.to_string());
                    return None;
                }
            }
        }
        self.rows[i].voice.as_mut()
    }

    fn adjust(&mut self, i: usize, change: impl FnOnce(&mut Voice), cx: &mut Context<Self>) {
        if let Some(voice) = self.voice_mut(i) {
            change(voice);
        }
        cx.notify();
    }

    fn set_engine(&mut self, i: usize, engine: Option<VoiceEngine>, cx: &mut Context<Self>) {
        let row = &mut self.rows[i];
        row.voice = engine.map(|engine| Voice {
            engine,
            ..row
                .voice
                .clone()
                .unwrap_or_else(|| Voice::new(VoiceEngine::Espeak { voice: "en".into() }))
        });
        cx.notify();
    }

    fn preview(&mut self, i: usize, cx: &mut Context<Self>) {
        let row = &self.rows[i];
        let text = format!("Hello, this is {}.", row.character.to_lowercase());
        let voice = row.voice.clone();
        self.editor
            .update(cx, |e, cx| e.preview_voice(voice, i, text, cx))
            .ok();
    }

    fn save(&mut self, cx: &mut Context<Self>) -> bool {
        let cast: BTreeMap<String, Voice> = self
            .rows
            .iter()
            .filter_map(|r| Some((character_key(&r.character), r.voice.clone()?)))
            .collect();
        let saved = self
            .editor
            .update(cx, |e, cx| e.set_voice_cast(cast, cx))
            .unwrap_or(false);
        if !saved {
            self.message = Some("The cast could not be saved; see the status bar.".into());
            cx.notify();
        }
        saved
    }

    fn voice_menu(&self, i: usize, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let pick = |label: String, engine: Option<VoiceEngine>, cx: &mut Context<Self>| {
            PopupMenuItem::new(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_engine(i, engine.clone(), cx)))
        };
        let mut menu =
            menu.scrollable(true)
                .max_h(px(420.))
                .item(pick("Default voice".into(), None, cx));
        if !self.models.is_empty() {
            menu = menu.label("Piper");
            for model in &self.models {
                let engine = VoiceEngine::Piper {
                    model: model.file_name(),
                    speaker: (!model.speakers.is_empty()).then_some(0),
                };
                menu = menu.item(pick(model.name.clone(), Some(engine), cx));
            }
        }
        if let Some(Ok((languages, _))) = &self.espeak {
            menu = menu.label("eSpeak NG");
            let variant = match self.rows[i].voice.as_ref().map(|v| &v.engine) {
                Some(VoiceEngine::Espeak { voice }) => voice
                    .split_once('+')
                    .map(|(_, v)| format!("+{v}"))
                    .unwrap_or_default(),
                _ => String::new(),
            };
            for language in languages {
                let engine = VoiceEngine::Espeak {
                    voice: format!("{}{variant}", language.id),
                };
                menu = menu.item(pick(
                    format!("{} ({})", language.name, language.id),
                    Some(engine),
                    cx,
                ));
            }
        }
        menu
    }

    fn variant_menu(&self, i: usize, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let Some(Ok((_, variants))) = &self.espeak else {
            return menu;
        };
        let language = match self.rows[i].voice.as_ref().map(|v| &v.engine) {
            Some(VoiceEngine::Espeak { voice }) => voice
                .split_once('+')
                .map_or(voice.as_str(), |(l, _)| l)
                .to_string(),
            _ => return menu,
        };
        let pick = |label: String, voice: String, cx: &mut Context<Self>| {
            PopupMenuItem::new(label).on_click(cx.listener(move |this, _, _, cx| {
                let voice = voice.clone();
                this.adjust(i, |v| v.engine = VoiceEngine::Espeak { voice }, cx)
            }))
        };
        let mut menu = menu.scrollable(true).max_h(px(420.)).item(pick(
            "No variant".into(),
            language.clone(),
            cx,
        ));
        for variant in variants {
            menu = menu.item(pick(
                format!("{} ({})", variant.name, variant.gender),
                format!("{language}+{}", variant.id),
                cx,
            ));
        }
        menu
    }

    fn row(&self, i: usize, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let row = &self.rows[i];
        let voice = row.voice.clone();
        let label = voice
            .as_ref()
            .map_or_else(|| "Default voice".to_string(), |v| v.engine.label());
        let id = |what: &str| SharedString::from(format!("voice-cast-{i}-{what}"));
        let entity = cx.entity().downgrade();
        let menu_entity = entity.clone();
        let mut line = div()
            .id(id("row"))
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .py(px(4.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .w(px(110.))
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(row.character.clone()),
            )
            .child(
                Button::new(id("voice"))
                    .label(format!("{label} ▾"))
                    .xsmall()
                    .outline()
                    .dropdown_menu(move |menu, _, cx| match menu_entity.upgrade() {
                        Some(e) => e.update(cx, |this, cx| this.voice_menu(i, menu, cx)),
                        None => menu,
                    }),
            );
        match voice.as_ref().map(|v| &v.engine) {
            Some(VoiceEngine::Espeak { voice }) => {
                let variant = voice.split_once('+').map_or("none", |(_, v)| v).to_string();
                let variant_entity = entity.clone();
                line = line.child(
                    Button::new(id("variant"))
                        .label(format!("Variant: {variant} ▾"))
                        .xsmall()
                        .ghost()
                        .dropdown_menu(move |menu, _, cx| match variant_entity.upgrade() {
                            Some(e) => e.update(cx, |this, cx| this.variant_menu(i, menu, cx)),
                            None => menu,
                        }),
                );
            }
            Some(VoiceEngine::Piper {
                model,
                speaker: Some(speaker),
            }) => {
                let count = self
                    .models
                    .iter()
                    .find(|m| m.file_name() == *model)
                    .map_or(1, |m| m.speakers.len().max(1)) as u32;
                let step = move |dir: i32| -> Click {
                    let entity = entity.clone();
                    Box::new(move |_, _, cx| {
                        entity
                            .update(cx, |this, cx| {
                                this.adjust(
                                    i,
                                    |v| {
                                        if let VoiceEngine::Piper {
                                            speaker: Some(s), ..
                                        } = &mut v.engine
                                        {
                                            *s = (*s as i64 + dir as i64).clamp(0, count as i64 - 1)
                                                as u32;
                                        }
                                    },
                                    cx,
                                )
                            })
                            .ok();
                    })
                };
                line = line.child(stepper(
                    &id("speaker"),
                    "Speaker",
                    speaker.to_string(),
                    p,
                    step(-1),
                    step(1),
                ));
            }
            _ => {}
        }
        let rate = voice.as_ref().map_or(1., |v| v.rate);
        let pitch = voice.as_ref().map_or(50, |v| v.pitch);
        let listener = |cx: &mut Context<Self>, change: fn(&mut Voice, i32), dir: i32| -> Click {
            Box::new(cx.listener(move |this, _, _, cx| this.adjust(i, |v| change(v, dir), cx)))
        };
        let rate_step: fn(&mut Voice, i32) = |v, dir| v.rate = step_rate(v.rate, dir);
        let pitch_step: fn(&mut Voice, i32) =
            |v, dir| v.pitch = (i32::from(v.pitch) + 5 * dir).clamp(0, MAX_PITCH.into()) as u8;
        line.child(stepper(
            &id("rate"),
            "Rate",
            format!("×{rate:.2}"),
            p,
            listener(cx, rate_step, -1),
            listener(cx, rate_step, 1),
        ))
        .child(stepper(
            &id("pitch"),
            "Pitch",
            pitch.to_string(),
            p,
            listener(cx, pitch_step, -1),
            listener(cx, pitch_step, 1),
        ))
        .child(
            Button::new(id("preview"))
                .label("▶ Preview")
                .xsmall()
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| this.preview(i, cx))),
        )
        .into_any_element()
    }
}

impl Render for VoiceCast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let rows: Vec<AnyElement> = (0..self.rows.len()).map(|i| self.row(i, &p, cx)).collect();
        let empty = self.rows.is_empty();
        let note = match &self.espeak {
            Some(Err(error)) if self.models.is_empty() => {
                format!("{error} {}", voices::MISSING)
            }
            _ => "Voices are made on this computer by Piper or eSpeak NG; nothing is sent over the network. Choose Piper's voices folder and the engine in Settings › Storyboard.".into(),
        };
        div()
            .id("voice-cast")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .when(empty, |d| {
                d.child(mono(
                    "No characters yet: write lines like “MIA: Hello.” in the Dialogue captions.",
                    10.5,
                    p.muted,
                ))
            })
            .child(
                div()
                    .id("voice-cast-rows")
                    .flex()
                    .flex_col()
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(mono(note, 10., p.muted))
            .children(self.message.clone().map(|message| {
                div()
                    .id("voice-cast-message")
                    .test_support()
                    .text_color(p.accent)
                    .child(message)
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("voice-cast-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("voice-cast-save")
                            .label("Save cast")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.save(cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

/// The Generate scratch dialogue dialog.
pub(crate) struct ScratchDialog {
    editor: WeakEntity<EditorView>,
    pub(crate) scope: ScratchScope,
    pub(crate) extend: bool,
}

impl ScratchDialog {
    pub(crate) fn generate(&mut self, cx: &mut Context<Self>) -> bool {
        let (scope, extend) = (self.scope, self.extend);
        self.editor
            .update(cx, |e, cx| {
                let panels = e.scratch_scope_panels(scope);
                e.generate_scratch_dialogue(panels, extend, cx).is_some()
            })
            .unwrap_or(false)
    }
}

impl Render for ScratchDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let lines = self.editor.upgrade().map_or(0, |e| {
            let e = e.read(cx);
            let panels = e.scratch_scope_panels(self.scope);
            e.editor.storyboard().map_or(0, |b| {
                let layout: Vec<PageId> = e.editor.page_list().iter().map(|m| m.id).collect();
                b.scratch_plan(&layout, &panels).len()
            })
        });
        let scope = |id: &'static str, text: &'static str, value: ScratchScope| {
            chip(id, text, self.scope == value, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.scope = value;
                    cx.notify();
                }))
        };
        div()
            .id("scratch-dialogue")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .child(mono("Speak", 10., p.muted))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(scope(
                        "scratch-scope-selection",
                        "Selected panels",
                        ScratchScope::Selection,
                    ))
                    .child(scope(
                        "scratch-scope-scene",
                        "Active scene",
                        ScratchScope::Scene,
                    ))
                    .child(scope(
                        "scratch-scope-board",
                        "Whole board",
                        ScratchScope::Board,
                    )),
            )
            .child(
                chip(
                    "scratch-extend",
                    "Lengthen panels to fit their lines",
                    self.extend,
                    &p,
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.extend = !this.extend;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .id("scratch-summary")
                    .test_support()
                    .child(format!("{lines} line(s) of dialogue to speak.")),
            )
            .child(mono(
                "Each panel's lines play back to back from its start on the Scratch dialogue track, with the cast's voices (Timing › Voice cast…). Earlier scratch lines of these panels are replaced; your own sounds and recordings stay. Voices are made on this computer; nothing is sent over the network.",
                10.,
                p.muted,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("scratch-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("scratch-generate")
                            .label("Generate")
                            .small()
                            .primary()
                            .disabled(lines == 0)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.generate(cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

/// The Enhance dialogue dialog.
pub(crate) struct EnhanceDialog {
    editor: WeakEntity<EditorView>,
    at: (usize, usize),
    pub(crate) new_track: bool,
}

impl Render for EnhanceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let option = |id: &'static str, text: &'static str, value: bool| {
            chip(id, text, self.new_track == value, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.new_track = value;
                    cx.notify();
                }))
        };
        div()
            .id("enhance-dialogue")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .child(mono(
                "Cuts rumble below 80 Hz, reduces noise, tames sibilance, evens levels and sets loudness to −16 LUFS, into a new sound. The original sound stays in the library. Runs on this computer through FFmpeg.",
                10.,
                p.muted,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(option(
                        "enhance-replace",
                        "The clip plays the enhanced sound",
                        false,
                    ))
                    .child(option(
                        "enhance-new-track",
                        "Keep the clip; add the enhanced one on the Enhanced dialogue track",
                        true,
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("enhance-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("enhance-ok")
                            .label("Enhance")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                let (at, new_track) = (this.at, this.new_track);
                                this.editor
                                    .update(cx, |e, cx| e.enhance_dialogue_clip(at, new_track, cx))
                                    .ok();
                                window.close_dialog(cx);
                            })),
                    ),
            )
    }
}

/// The Regenerate line dialog: intonation controls for one scratch line.
pub(crate) struct RegenerateDialog {
    editor: WeakEntity<EditorView>,
    asset: AssetId,
    line: ScratchLine,
    pub(crate) delivery: Delivery,
}

impl Render for RegenerateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let d = self.delivery;
        let step = |cx: &mut Context<Self>, change: fn(&mut Delivery, i32), dir: i32| -> Click {
            Box::new(cx.listener(move |this, _, _, cx| {
                change(&mut this.delivery, dir);
                cx.notify();
            }))
        };
        let rate: fn(&mut Delivery, i32) = |d, dir| d.rate = step_rate(d.rate, dir);
        let pitch: fn(&mut Delivery, i32) =
            |d, dir| d.pitch = (i32::from(d.pitch) + 5 * dir).clamp(-50, 50) as i8;
        let variation: fn(&mut Delivery, i32) =
            |d, dir| d.variation = round(d.variation + 0.1 * dir as f32, 0.1).clamp(0., 1.);
        let emphasis_row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(mono("Emphasis", 10., p.muted))
            .children(
                [
                    ("regenerate-emphasis-none", "None", Emphasis::None),
                    (
                        "regenerate-emphasis-moderate",
                        "Moderate",
                        Emphasis::Moderate,
                    ),
                    ("regenerate-emphasis-strong", "Strong", Emphasis::Strong),
                ]
                .map(|(id, text, value)| {
                    chip(id, text, d.emphasis == value, &p)
                        .test_support()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.delivery.emphasis = value;
                            cx.notify();
                        }))
                }),
            );
        let steppers = [
            stepper(
                "regenerate-rate",
                "Rate",
                format!("×{:.2}", d.rate),
                &p,
                step(cx, rate, -1),
                step(cx, rate, 1),
            ),
            stepper(
                "regenerate-pitch",
                "Pitch",
                format!("{:+}", d.pitch),
                &p,
                step(cx, pitch, -1),
                step(cx, pitch, 1),
            ),
            stepper(
                "regenerate-variation",
                "Variation (Piper)",
                format!("{:.1}", d.variation),
                &p,
                step(cx, variation, -1),
                step(cx, variation, 1),
            ),
        ];
        let speaker = if self.line.character.is_empty() {
            String::new()
        } else {
            format!("{}: ", self.line.character)
        };
        div()
            .id("regenerate-line")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .child(div().child(format!("{speaker}{}", self.line.text)))
            .children(steppers)
            .child(emphasis_row)
            .child(mono(
                "The clip plays the new take; the earlier take stays in the library. Rate and pitch add to the character's voice.",
                10.,
                p.muted,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("regenerate-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("regenerate-ok")
                            .label("Regenerate")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                let (asset, delivery) = (this.asset, this.delivery);
                                this.editor
                                    .update(cx, |e, cx| {
                                        e.regenerate_scratch_line(asset, delivery, cx)
                                    })
                                    .ok();
                                window.close_dialog(cx);
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard_voices::SCRATCH_TRACK;

    fn storyboard_editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        project
            .edit_storyboard(|b| {
                let field = b.dialogue_field().unwrap();
                let first = *b.panels.keys().next().unwrap();
                b.panels
                    .get_mut(&first)
                    .unwrap()
                    .captions
                    .insert(field, "MIA: Is anyone there?\nTOM: Only me.".into());
                b.voices.voices.insert(
                    "MIA".into(),
                    Voice::new(VoiceEngine::Espeak {
                        voice: "en-us+f2".into(),
                    }),
                );
                b.voices.voices.insert(
                    "TOM".into(),
                    Voice::new(VoiceEngine::Espeak {
                        voice: "en-us+m3".into(),
                    }),
                );
                Ok(())
            })
            .unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        (editor, cx)
    }

    #[test]
    fn rates_step_within_range() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(step_rate(1., 1), 1.05));
        assert!(close(step_rate(2., 1), 2.));
        assert!(close(step_rate(0.5, -1), 0.5));
    }

    #[gpui_kit::test]
    fn dialogs_list_characters_and_count_lines(cx: &mut TestAppContext) {
        let (e, cx) = storyboard_editor(cx);
        let cast = cx
            .update(|window, cx| e.update(cx, |e, cx| e.open_voice_cast(window, cx)))
            .unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| {
            let names: Vec<_> = cast
                .read(cx)
                .rows
                .iter()
                .map(|r| r.character.clone())
                .collect();
            assert_eq!(names, ["MIA", "TOM"]);
        });
        // Saving the cast is one Undo step; a no-op saves nothing.
        cx.update(|_, cx| {
            cast.update(cx, |c, cx| {
                c.rows[1].voice = None;
                assert!(c.save(cx));
            })
        });
        cx.update(|_, cx| {
            let board = e.read(cx).editor.storyboard().unwrap().clone();
            assert!(board.voices.voice("TOM").is_none() && board.voices.voice("MIA").is_some());
            e.update(cx, |e, cx| e.undo(cx));
            assert!(
                e.read(cx)
                    .editor
                    .storyboard()
                    .unwrap()
                    .voices
                    .voice("TOM")
                    .is_some()
            );
        });
        cx.update(|window, cx| window.close_dialog(cx));
        let scratch = cx
            .update(|window, cx| e.update(cx, |e, cx| e.open_scratch_dialogue(window, cx)))
            .unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(scratch.read(cx).scope, ScratchScope::Board));
        cx.update(|_, cx| {
            let e = e.read(cx);
            let panels = e.scratch_scope_panels(ScratchScope::Board);
            let layout: Vec<PageId> = e.editor.page_list().iter().map(|m| m.id).collect();
            let plan = e
                .editor
                .storyboard()
                .unwrap()
                .scratch_plan(&layout, &panels);
            assert_eq!(plan.len(), 2);
        });
    }

    #[gpui_kit::test]
    fn scratch_dialogue_generates_as_one_undo_step(cx: &mut TestAppContext) {
        if !voices::espeak_available() || !emulsion_io::ffmpeg::available() {
            eprintln!("skipped: needs eSpeak NG and FFmpeg");
            return;
        }
        let (e, cx) = storyboard_editor(cx);
        let job = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                let panels = e.scratch_scope_panels(ScratchScope::Board);
                e.generate_scratch_dialogue(panels, true, cx)
            })
        });
        let job = job.unwrap();
        while !job.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            cx.run_until_parked();
        }
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let board = e.editor.storyboard().unwrap();
            let track = board
                .timeline
                .tracks
                .iter()
                .position(|t| t.name == SCRATCH_TRACK)
                .unwrap_or_else(|| panic!("{:?}", e.status));
            assert_eq!(board.timeline.tracks[track].clips.len(), 2);
            assert_eq!(board.voices.lines.len(), 2);
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        cx.update(|_, cx| {
            let board = e.read(cx).editor.storyboard().unwrap().clone();
            assert!(board.voices.lines.is_empty());
            assert!(board.timeline.tracks.iter().all(|t| t.clips.is_empty()));
        });
    }
}
