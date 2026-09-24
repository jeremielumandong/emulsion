//! Serialize settings writes off the UI thread, with best-effort draining at exit.

use std::future::Future;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use emulsion_io::settings::Settings;
use gpui_kit::{App, BackgroundExecutor, Global, Task};

type SaveResult = Result<(), String>;

struct SaveRequest {
    settings: Settings,
    reply: Sender<SaveResult>,
}

struct SettingsWriter {
    requests: Sender<SaveRequest>,
    worker: Option<Task<()>>,
}

impl Global for SettingsWriter {}

impl SettingsWriter {
    fn new<F, Fut>(executor: &BackgroundExecutor, write: F) -> Self
    where
        F: FnMut(Settings) -> Fut + Send + 'static,
        Fut: Future<Output = SaveResult> + Send + 'static,
    {
        let (requests, receiver) = async_channel::unbounded();
        let worker = executor.spawn(write_requests(receiver, write));
        Self {
            requests,
            worker: Some(worker),
        }
    }

    fn enqueue(&self, settings: Settings) -> Receiver<SaveResult> {
        let (reply, result) = async_channel::bounded(1);
        if let Err(error) = self.requests.try_send(SaveRequest { settings, reply }) {
            let _ = error
                .into_inner()
                .reply
                .try_send(Err("Settings writer is shutting down".into()));
        }
        result
    }

    fn shutdown(&mut self) -> Option<Task<()>> {
        // Closing the sender rejects new work but leaves queued requests readable.
        self.requests.close();
        self.worker.take()
    }
}

async fn write_requests<F, Fut>(requests: Receiver<SaveRequest>, mut write: F)
where
    F: FnMut(Settings) -> Fut,
    Fut: Future<Output = SaveResult>,
{
    while let Ok(request) = requests.recv().await {
        let start = Instant::now();
        let result = write(request.settings).await;
        if let Err(error) = &result {
            tracing::warn!(%error, "could not save settings");
        }
        if start.elapsed() > Duration::from_millis(50) {
            tracing::warn!(
                elapsed_ms = start.elapsed().as_millis() as u64,
                "settings save was slow"
            );
        }
        // A caller dropping its receipt must not cancel persistence or the queue.
        let _ = request.reply.try_send(result);
    }
}

/// Queue a snapshot immediately. Dropping the returned receipt does not cancel
/// the write; awaiting it reports the result of this particular snapshot.
pub(crate) fn save(settings: Settings, cx: &mut App) -> Task<SaveResult> {
    if !cx.has_global::<SettingsWriter>() {
        cx.set_global(SettingsWriter::new(
            cx.background_executor(),
            |settings| async move { settings.save().map_err(|error| error.to_string()) },
        ));
        cx.on_app_quit(|cx| {
            let worker = cx.global_mut::<SettingsWriter>().shutdown();
            async move {
                if let Some(worker) = worker {
                    worker.await;
                }
            }
        })
        .detach();
    }
    let result = cx.global::<SettingsWriter>().enqueue(settings);
    cx.background_executor().spawn(async move {
        result
            .recv()
            .await
            .unwrap_or_else(|_| Err("Settings writer stopped before saving".into()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn snapshot(draw_mode: bool) -> Settings {
        Settings {
            draw_mode,
            ..Settings::default()
        }
    }

    #[gpui_kit::test]
    fn slow_write_leaves_ui_runnable_and_preserves_snapshot_order(cx: &mut TestAppContext) {
        let (permit, wait) = async_channel::unbounded();
        let started = Arc::new(Mutex::new(Vec::new()));
        let saved = Arc::new(Mutex::new(Vec::new()));
        let mut writer = SettingsWriter::new(&cx.executor(), {
            let started = started.clone();
            let saved = saved.clone();
            move |settings| {
                let wait = wait.clone();
                let started = started.clone();
                let saved = saved.clone();
                async move {
                    started.lock().push(settings.draw_mode);
                    wait.recv().await.unwrap();
                    saved.lock().push(settings.draw_mode);
                    Ok(())
                }
            }
        });
        let first = writer.enqueue(snapshot(true));
        let second = writer.enqueue(snapshot(false));
        let (ui_ran, ui_result) = async_channel::bounded(1);
        cx.spawn(async move |_| ui_ran.send(()).await.unwrap())
            .detach();
        cx.run_until_parked();
        assert_eq!(ui_result.try_recv(), Ok(()));
        assert_eq!(*started.lock(), vec![true]);
        assert!(saved.lock().is_empty());
        assert!(first.try_recv().is_err());
        assert!(second.try_recv().is_err());

        permit.try_send(()).unwrap();
        permit.try_send(()).unwrap();
        cx.run_until_parked();
        assert_eq!(*saved.lock(), vec![true, false]);
        assert_eq!(first.try_recv(), Ok(Ok(())));
        assert_eq!(second.try_recv(), Ok(Ok(())));
        writer.shutdown().unwrap().detach();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn failed_write_and_dropped_receipt_do_not_stop_later_saves(cx: &mut TestAppContext) {
        let saved = Arc::new(Mutex::new(Vec::new()));
        let mut writer = SettingsWriter::new(&cx.executor(), {
            let saved = saved.clone();
            move |settings| {
                let saved = saved.clone();
                async move {
                    if settings.draw_mode {
                        Err("disk unavailable".into())
                    } else {
                        saved.lock().push(false);
                        Ok(())
                    }
                }
            }
        });
        let failed = writer.enqueue(snapshot(true));
        drop(writer.enqueue(snapshot(false)));
        let last = writer.enqueue(snapshot(false));
        cx.run_until_parked();
        assert_eq!(failed.try_recv(), Ok(Err("disk unavailable".into())));
        assert_eq!(last.try_recv(), Ok(Ok(())));
        assert_eq!(*saved.lock(), vec![false, false]);
        writer.shutdown().unwrap().detach();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn shutdown_waits_for_in_flight_and_queued_snapshots(cx: &mut TestAppContext) {
        let (permit, wait) = async_channel::unbounded();
        let saved = Arc::new(Mutex::new(Vec::new()));
        let mut writer = SettingsWriter::new(&cx.executor(), {
            let saved = saved.clone();
            move |settings| {
                let wait = wait.clone();
                let saved = saved.clone();
                async move {
                    wait.recv().await.unwrap();
                    saved.lock().push(settings.draw_mode);
                    Ok(())
                }
            }
        });
        let first = writer.enqueue(snapshot(true));
        cx.run_until_parked();
        let last = writer.enqueue(snapshot(false));
        let worker = writer.shutdown().unwrap();
        assert!(writer.enqueue(snapshot(true)).try_recv().unwrap().is_err());
        let (finished, completion) = async_channel::bounded(1);
        cx.executor()
            .spawn(async move {
                worker.await;
                finished.send(()).await.unwrap();
            })
            .detach();
        cx.run_until_parked();
        assert!(completion.try_recv().is_err());

        permit.try_send(()).unwrap();
        permit.try_send(()).unwrap();
        cx.run_until_parked();
        assert_eq!(completion.try_recv(), Ok(()));
        assert_eq!(first.try_recv(), Ok(Ok(())));
        assert_eq!(last.try_recv(), Ok(Ok(())));
        assert_eq!(*saved.lock(), vec![true, false]);
    }
}
