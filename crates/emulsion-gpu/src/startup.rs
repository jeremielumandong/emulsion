//! One bounded, app-wide compute readiness attempt. No GPU work runs on callers.
use crate::{GpuContext, readiness};
use anyhow::{Context, Result, bail, ensure};
use std::cell::RefCell;
use std::future::Future;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};
use std::task::{Poll, Wake, Waker};
use std::time::{Duration, Instant};

pub const STARTUP_BUDGET: Duration = Duration::from_secs(20);
const PENDING: u8 = 0;
const COMMITTING: u8 = 1;
const READY: u8 = 2;
const DISABLED: u8 = 3;
const UNAVAILABLE: u8 = 4;
const CANCELLED: u8 = 5;
const TIMED_OUT: u8 = 6;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitializationStatus {
    Pending,
    Ready,
    DisabledByConfiguration,
    Unavailable(String),
    Cancelled,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capability {
    Composite,
    Filters,
    Paint,
    PersistentPaint,
    Screen,
}
impl Capability {
    pub(crate) const ALL: [Self; 5] = [
        Self::Composite,
        Self::Filters,
        Self::Paint,
        Self::PersistentPaint,
        Self::Screen,
    ];
    pub(crate) fn bit(self) -> u8 {
        1 << self as u8
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Composite => "composite",
            Self::Filters => "filters",
            Self::Paint => "paint",
            Self::PersistentPaint => "persistent-paint",
            Self::Screen => "screen",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StartupProfile {
    pub(crate) disabled: bool,
    pub(crate) software: bool,
    pub(crate) preferred: bool,
    pub(crate) capabilities: u8,
}
impl StartupProfile {
    fn parse(mode: Option<&str>, brushes: Option<&str>) -> Self {
        let preferred = matches!(mode, Some("force" | "software"));
        let mut capabilities = Capability::Composite.bit() | Capability::Filters.bit();
        match brushes {
            Some("1") => capabilities |= Capability::Paint.bit(),
            Some("persistent") => capabilities |= Capability::PersistentPaint.bit(),
            _ => {}
        }
        if preferred {
            capabilities |= Capability::Screen.bit();
        }
        Self {
            disabled: mode == Some("cpu"),
            software: mode == Some("software"),
            preferred,
            capabilities,
        }
    }
    pub(crate) fn from_environment() -> Self {
        Self::parse(
            std::env::var("EMULSION_GPU").ok().as_deref(),
            std::env::var("EMULSION_GPU_BRUSHES").ok().as_deref(),
        )
    }
    #[cfg(test)]
    pub(crate) fn test_profile() -> Self {
        Self {
            capabilities: Capability::ALL
                .iter()
                .fold(0, |mask, item| mask | item.bit()),
            ..Self::from_environment()
        }
    }
}

trait Clock: Send + Sync {
    fn elapsed(&self) -> Duration;
}
struct MonotonicClock(Instant);
impl Clock for MonotonicClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

pub(crate) struct StartupControl {
    state: AtomicU8,
    reason: OnceLock<String>,
    clock: Arc<dyn Clock>,
    budget: Duration,
    watcher: OnceLock<std::thread::Thread>,
    publication_fault_gate: Mutex<()>,
    ready_context: Mutex<Option<Arc<GpuContext>>>,
}
impl StartupControl {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_clock(Arc::new(MonotonicClock(Instant::now())), STARTUP_BUDGET)
    }
    fn with_clock(clock: Arc<dyn Clock>, budget: Duration) -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(PENDING),
            reason: OnceLock::new(),
            clock,
            budget,
            watcher: OnceLock::new(),
            publication_fault_gate: Mutex::new(()),
            ready_context: Mutex::new(None),
        })
    }
    fn transition(&self, terminal: u8) {
        let _ = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, PENDING | COMMITTING).then_some(terminal)
            });
        if let Some(watcher) = self.watcher.get() {
            watcher.unpark();
        }
    }
    pub(crate) fn ready(&self) -> bool {
        self.state.load(Ordering::Acquire) == READY
    }
    fn expire(&self) {
        if self.clock.elapsed() >= self.budget {
            self.transition(TIMED_OUT);
        }
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.expire();
        ensure!(
            self.state.load(Ordering::Acquire) == PENDING,
            "GPU startup no longer pending: {:?}",
            self.status()
        );
        Ok(())
    }
    fn remaining(&self) -> Result<Duration> {
        self.check()?;
        Ok(self.budget.saturating_sub(self.clock.elapsed()))
    }
    fn status(&self) -> InitializationStatus {
        self.expire();
        match self.state.load(Ordering::Acquire) {
            PENDING | COMMITTING => InitializationStatus::Pending,
            READY => InitializationStatus::Ready,
            DISABLED => InitializationStatus::DisabledByConfiguration,
            UNAVAILABLE => InitializationStatus::Unavailable(
                self.reason
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "Compute preparation failed".into()),
            ),
            CANCELLED => InitializationStatus::Cancelled,
            TIMED_OUT => InitializationStatus::TimedOut,
            _ => unreachable!(),
        }
    }
    pub(crate) fn fail(&self, error: impl std::fmt::Display) {
        self.expire();
        let _ = self.reason.set(error.to_string());
        self.transition(UNAVAILABLE);
    }
    /// Serialize a recorded device fault with the final readiness commit. This
    /// gate is never used by status, cancellation, context, or hook readers.
    pub(crate) fn device_fault(&self, mark_failed: impl FnOnce()) {
        let _gate = self
            .publication_fault_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        mark_failed();
        self.fail("Compute device fault during readiness");
    }

    /// Install only inactive weak hooks, then make them visible with one final
    /// CAS. Cancellation/deadline can win throughout the registration section.
    /// Nothing GPU-blocking or user logging is allowed inside this section.
    fn commit(
        &self,
        healthy: impl Fn() -> bool,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.check()?;
        struct CommitGuard<'a>(&'a StartupControl);
        impl Drop for CommitGuard<'_> {
            fn drop(&mut self) {
                let _ = self.0.state.compare_exchange(
                    COMMITTING,
                    UNAVAILABLE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                if !self.0.ready() {
                    // Release both mutexes before device teardown: it may invoke
                    // a lost-device callback that needs the fault gate.
                    let abandoned = self
                        .0
                        .ready_context
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                    drop(abandoned);
                }
                if let Some(watcher) = self.0.watcher.get() {
                    watcher.unpark();
                }
            }
        }
        // Declaration order matters: the fault gate drops before this guard.
        let _guard = CommitGuard(self);
        let _fault_gate = self
            .publication_fault_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Compute publication gate poisoned"))?;
        self.check()?;
        ensure!(healthy(), "Compute device failed before publication");
        self.state
            .compare_exchange(PENDING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| anyhow::anyhow!("GPU startup cancelled before publication"))?;
        if let Err(error) = publish() {
            self.fail(&error);
            return Err(error);
        }
        self.expire();
        ensure!(healthy(), "Compute device failed during publication");
        self.state
            .compare_exchange(COMMITTING, READY, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                anyhow::anyhow!("GPU readiness publication prevented: {:?}", self.status())
            })?;
        Ok(())
    }
    fn watch(self: Arc<Self>) {
        let _ = self.watcher.set(std::thread::current());
        while matches!(self.state.load(Ordering::Acquire), PENDING | COMMITTING) {
            self.expire();
            if !matches!(self.state.load(Ordering::Acquire), PENDING | COMMITTING) {
                break;
            }
            std::thread::park_timeout(self.budget.saturating_sub(self.clock.elapsed()));
        }
    }
}

/// Clones are observers, not independent cancellation owners. Dropping one
/// cannot cancel app-wide startup; the app's single service owner calls cancel.
#[derive(Clone)]
pub struct InitializationHandle {
    pub(crate) control: Arc<StartupControl>,
}
impl InitializationHandle {
    pub fn status(&self) -> InitializationStatus {
        self.control.status()
    }
    pub fn cancel(&self) {
        self.control.transition(CANCELLED);
    }
    pub(crate) fn wait(&self) {
        while self.status() == InitializationStatus::Pending {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

pub(crate) struct PreparedContext {
    gpu: Arc<GpuContext>,
    profile: StartupProfile,
}
impl PreparedContext {
    pub(crate) fn prepare(profile: StartupProfile, control: &Arc<StartupControl>) -> Result<Self> {
        control.check()?;
        ensure!(!profile.disabled, "CPU requested");
        let gpu = Arc::new(with_startup(control, true, || {
            GpuContext::new_with_preference(profile.software)
        })?);
        validate_capabilities(profile, control, |capability, preparation| {
            ensure!(gpu.available(), "Compute device failed during readiness");
            if preparation {
                with_startup(control, true, || readiness::validate(&gpu, capability))
            } else {
                with_startup(control, false, || readiness::validate(&gpu, capability))
            }
        })?;
        // Retain pipelines only, not fixtures or high-water scratch buffers.
        gpu.finish_preparation(profile.capabilities)?;
        #[cfg(test)]
        {
            assert_eq!(
                gpu.retained_resources(),
                (profile.capabilities.count_ones() as usize, 0)
            );
            assert_eq!(gpu.persistent_sessions.load(Ordering::SeqCst), 0);
        }
        control.check()?;
        ensure!(gpu.available(), "Compute device failed after readiness");
        Ok(Self { gpu, profile })
    }
    pub(crate) fn publish(self, control: &Arc<StartupControl>) -> Result<()> {
        control.commit(
            || self.gpu.available(),
            || {
                crate::publish(self.gpu.clone(), self.profile, control.clone())?;
                *control
                    .ready_context
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Compute publication owner poisoned"))? =
                    Some(self.gpu.clone());
                Ok(())
            },
        )?;
        tracing::info!(adapter = ?self.gpu.adapter_info(), capabilities = ?self.gpu.prepared_capabilities(), elapsed = ?control.clock.elapsed(), "GPU image acceleration ready");
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn into_context(self) -> Arc<GpuContext> {
        self.gpu
    }
}

fn validate_capabilities(
    profile: StartupProfile,
    control: &StartupControl,
    mut validate: impl FnMut(Capability, bool) -> Result<()>,
) -> Result<()> {
    for preparation in [true, false] {
        for capability in Capability::ALL {
            if profile.capabilities & capability.bit() == 0 {
                continue;
            }
            control.check()?;
            validate(capability, preparation)?;
            control.check()?;
        }
    }
    Ok(())
}

pub(crate) fn begin(profile: StartupProfile) -> InitializationHandle {
    let control = StartupControl::new();
    let handle = InitializationHandle {
        control: control.clone(),
    };
    if profile.disabled {
        control.transition(DISABLED);
        return handle;
    }
    // At most one watchdog and one driver-owning worker per process attempt.
    // Neither is joined from the UI. A blocked synchronous driver call may
    // outlive the deadline; the independent controller still prevents publish.
    let watcher = control.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("compute-readiness-deadline".into())
        .spawn(move || watcher.watch())
    {
        control.fail(error);
        return handle;
    }
    let worker = control.clone();
    if let Err(error) = std::thread::Builder::new().name("compute-readiness".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PreparedContext::prepare(profile, &worker)?.publish(&worker)));
        match result {
            Ok(Ok(())) => {},
            Ok(Err(error)) => { worker.fail(&error); tracing::info!(%error, status = ?worker.status(), "GPU image acceleration unavailable; using CPU"); }
            Err(_) => worker.fail("Compute preparation panicked"),
        }
    }) { control.fail(error); }
    handle
}

// Startup allowance is worker-scoped, never a context-wide runtime timeout.
// The second validation pass and every published request take the unchanged
// ordinary two-second paths. Raw fault-test devices remain raw devices.
thread_local! { static PREPARATION: RefCell<Option<(Arc<StartupControl>, bool)>> = const { RefCell::new(None) }; }
pub(crate) fn preparation_control() -> Option<Arc<StartupControl>> {
    PREPARATION.with(|slot| slot.borrow().as_ref().map(|(control, _)| control.clone()))
}
fn extended_wait() -> bool {
    PREPARATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|(_, extended)| *extended)
    })
}
fn with_startup<T>(
    control: &Arc<StartupControl>,
    extended: bool,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    struct Reset(Option<(Arc<StartupControl>, bool)>);
    impl Drop for Reset {
        fn drop(&mut self) {
            PREPARATION.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    control.check()?;
    let _reset = Reset(PREPARATION.with(|slot| slot.replace(Some((control.clone(), extended)))));
    let result = work();
    control.check()?;
    result
}

/// Bounded future polling for startup adapter/device/error-scope futures.
/// Calls to Future::poll or synchronous driver entry points can themselves
/// block; only the independent control plane can bound visibility in that case.
pub(crate) fn wait_future<F: Future>(future: F) -> Result<F::Output> {
    let Some(control) = preparation_control() else {
        return Ok(pollster::block_on(future));
    };
    struct Signal(std::thread::Thread);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Signal(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        control.check()?;
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            control.check()?;
            return Ok(value);
        }
        std::thread::park_timeout(control.remaining()?.min(POLL_INTERVAL));
    }
}

pub(crate) fn wait_submission(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    submission: wgpu::SubmissionIndex,
) -> Result<()> {
    if extended_wait() {
        let control = preparation_control().expect("startup wait has a controller");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        queue.on_submitted_work_done(move || {
            let _ = tx.send(());
        });
        loop {
            control.check()?;
            device
                .poll(wgpu::PollType::Poll)
                .context("Compute preparation poll failed")?;
            match rx.try_recv() {
                Ok(()) => {
                    control.check()?;
                    return Ok(());
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    bail!("Compute preparation callback disconnected")
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(control.remaining()?.min(POLL_INTERVAL))
                }
            }
        }
    } else {
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(2)),
            })
            .context("Compute device did not complete")?;
        if let Some(control) = preparation_control() {
            control.check()?;
        }
        Ok(())
    }
}

pub(crate) fn readback<T>(rx: &std::sync::mpsc::Receiver<T>) -> Result<T> {
    // Keep the ordinary two-second callback ceiling. During startup also
    // observe cancellation/deadline in short intervals, without resetting it.
    let Some(control) = preparation_control() else {
        return rx
            .recv_timeout(Duration::from_secs(2))
            .context("Readback timed out");
    };
    let deadline = Instant::now() + control.remaining()?.min(Duration::from_secs(2));
    loop {
        control.check()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Readback timed out");
        match rx.recv_timeout(remaining.min(POLL_INTERVAL)) {
            Ok(value) => {
                control.check()?;
                return Ok(value);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("Readback callback disconnected")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
    };

    #[derive(Default)]
    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }
    impl FakeClock {
        fn advance(&self, milliseconds: u64) {
            self.0.fetch_add(milliseconds, Ordering::SeqCst);
        }
    }
    fn fixture() -> (Arc<FakeClock>, Arc<StartupControl>) {
        let clock = Arc::new(FakeClock::default());
        let control = StartupControl::with_clock(clock.clone(), Duration::from_millis(20));
        (clock, control)
    }
    fn profile() -> StartupProfile {
        StartupProfile::parse(Some("software"), Some("persistent"))
    }

    #[test]
    fn launch_profiles_do_not_expand_default_capabilities() {
        let automatic = StartupProfile::parse(None, None);
        assert!(!automatic.disabled && !automatic.software && !automatic.preferred);
        assert_eq!(
            automatic.capabilities,
            Capability::Composite.bit() | Capability::Filters.bit()
        );
        for (mode, brush, optional) in [
            (None, Some("1"), Capability::Paint),
            (
                Some("software"),
                Some("persistent"),
                Capability::PersistentPaint,
            ),
        ] {
            let selected = StartupProfile::parse(mode, brush);
            assert_ne!(selected.capabilities & optional.bit(), 0);
            let other = if optional == Capability::Paint {
                Capability::PersistentPaint
            } else {
                Capability::Paint
            };
            assert_eq!(selected.capabilities & other.bit(), 0);
        }
        assert!(StartupProfile::parse(Some("force"), None).preferred);
        assert!(!StartupProfile::parse(Some("force"), None).software);
    }

    #[test]
    fn cpu_override_starts_no_worker_or_device_and_repeated_requests_share_attempt() {
        let once = OnceLock::new();
        let starts = AtomicUsize::new(0);
        for _ in 0..10 {
            let handle = once.get_or_init(|| {
                starts.fetch_add(1, Ordering::SeqCst);
                begin(StartupProfile::parse(Some("cpu"), Some("persistent")))
            });
            assert_eq!(
                handle.status(),
                InitializationStatus::DisabledByConfiguration
            );
            assert!(handle.control.watcher.get().is_none());
            handle.cancel();
            assert_eq!(
                handle.status(),
                InitializationStatus::DisabledByConfiguration
            );
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pending_requests_do_not_wait_for_worker_and_every_selected_readback_precedes_publish() {
        let (_, control) = fixture();
        let worker_lock = Mutex::new(());
        let _held = worker_lock.lock().unwrap();
        // Readiness is independent of the held preparation/device lock. These
        // are the same lock-free guard used by compositor/filter/paint proxies.
        assert!(!control.ready());
        assert_eq!(control.status(), InitializationStatus::Pending);
        let mut checks = Vec::new();
        validate_capabilities(profile(), &control, |capability, preparation| {
            assert!(!control.ready());
            checks.push((capability, preparation));
            Ok(())
        })
        .unwrap();
        let wanted = [
            Capability::Composite,
            Capability::Filters,
            Capability::PersistentPaint,
            Capability::Screen,
        ];
        assert_eq!(
            checks,
            [true, false]
                .into_iter()
                .flat_map(|preparation| wanted.map(|item| (item, preparation)))
                .collect::<Vec<_>>()
        );
        control
            .commit(
                || true,
                || {
                    assert_eq!(checks.len(), 8);
                    assert!(!control.ready());
                    Ok(())
                },
            )
            .unwrap();
        assert!(control.ready());
    }

    #[test]
    fn fault_at_any_capability_or_readback_never_publishes_or_retries() {
        // These errors exercise the common terminal path used by the actual
        // scoped validation, map, oracle, and device-health checks.
        for reason in [
            "wrong pixels",
            "nonfinite",
            "wrong length",
            "map failure",
            "validation",
            "out of memory",
            "device lost",
            "uncaptured error",
            "normal two-second timeout",
        ] {
            for failed_stage in 0..8 {
                let (_, control) = fixture();
                let mut stages = 0;
                let result = validate_capabilities(profile(), &control, |_, _| {
                    let stage = stages;
                    stages += 1;
                    if stage == failed_stage {
                        bail!("{reason}");
                    }
                    Ok(())
                });
                assert!(result.is_err());
                control.fail(result.unwrap_err());
                let published = AtomicBool::new(false);
                assert!(
                    control
                        .commit(
                            || true,
                            || {
                                published.store(true, Ordering::SeqCst);
                                Ok(())
                            }
                        )
                        .is_err()
                );
                assert!(!published.load(Ordering::SeqCst));
                assert_eq!(stages, failed_stage + 1);
                assert_eq!(
                    control.status(),
                    InitializationStatus::Unavailable(reason.into())
                );
            }
        }
    }

    #[test]
    fn aggregate_budget_is_not_reset_between_kernels_or_validation_passes() {
        let (clock, control) = fixture();
        let mut stages = 0;
        let result = validate_capabilities(profile(), &control, |_, _| {
            stages += 1;
            clock.advance(6);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(stages, 4);
        assert_eq!(control.status(), InitializationStatus::TimedOut);
        assert!(
            control
                .commit(|| true, || panic!("late publication"))
                .is_err()
        );
    }

    struct Resource(Arc<AtomicUsize>);
    impl Drop for Resource {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn cancellation_before_device_and_at_every_stage_cleans_once() {
        let (_, control) = fixture();
        control.transition(CANCELLED);
        assert!(
            validate_capabilities(profile(), &control, |_, _| panic!("device must not start"))
                .is_err()
        );
        for cancel_stage in 0..8 {
            let (_, control) = fixture();
            let drops = Arc::new(AtomicUsize::new(0));
            let mut stages = 0;
            let result = validate_capabilities(profile(), &control, |_, _| {
                let _resource = Resource(drops.clone());
                if stages == cancel_stage {
                    control.transition(CANCELLED);
                }
                stages += 1;
                Ok(())
            });
            assert!(result.is_err());
            assert_eq!(stages, cancel_stage + 1);
            assert_eq!(drops.load(Ordering::SeqCst), stages);
            assert!(
                control
                    .commit(|| true, || panic!("cancelled publication"))
                    .is_err()
            );
            control.transition(CANCELLED);
            control.fail("late callback");
            assert_eq!(control.status(), InitializationStatus::Cancelled);
        }
    }

    #[test]
    fn both_cancel_versus_commit_orders_have_one_clear_winner() {
        let (_, control) = fixture();
        control.transition(CANCELLED);
        assert!(control.commit(|| true, || panic!("cancel wins")).is_err());
        let (_, control) = fixture();
        let count = AtomicUsize::new(0);
        control
            .commit(
                || true,
                || {
                    // Hooks still decline until the final Ready transition.
                    assert_eq!(control.status(), InitializationStatus::Pending);
                    assert!(!control.ready());
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .unwrap();
        control.transition(CANCELLED);
        assert_eq!(control.status(), InitializationStatus::Ready);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(
            control
                .commit(|| true, || panic!("second publication"))
                .is_err()
        );
    }

    #[test]
    fn blocked_driver_outlives_deadline_without_blocking_observers_or_late_publication() {
        let (clock, control) = fixture();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let drops = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (control, entered, release, drops) = (
                control.clone(),
                entered.clone(),
                release.clone(),
                drops.clone(),
            );
            std::thread::spawn(move || {
                let result = validate_capabilities(profile(), &control, |_, _| {
                    let _resource = Resource(drops.clone());
                    entered.wait();
                    release.wait();
                    Ok(())
                });
                assert!(result.is_err());
                assert!(
                    control
                        .commit(|| true, || panic!("late driver published"))
                        .is_err()
                );
            })
        };
        entered.wait();
        clock.advance(20);
        assert_eq!(control.status(), InitializationStatus::TimedOut);
        assert!(!control.ready());
        assert_eq!(drops.load(Ordering::SeqCst), 0); // honestly still owned by driver
        release.wait();
        thread.join().unwrap(); // test only; never part of UI cancellation
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(control.status(), InitializationStatus::TimedOut);
    }

    #[test]
    fn cancelled_mapping_and_late_callback_cannot_revive_readiness() {
        let (_, control) = fixture();
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);
        let result = with_startup(&control, true, || {
            control.transition(CANCELLED);
            readback(&rx)
        });
        assert!(result.is_err());
        drop(rx);
        assert!(tx.send(Ok(())).is_err()); // same ignored closed-receiver callback
        control.fail("late device error");
        assert_eq!(control.status(), InitializationStatus::Cancelled);
        assert!(preparation_control().is_none());
    }

    #[test]
    fn publication_conflict_fault_or_panic_never_marks_ready() {
        let (_, control) = fixture();
        assert!(
            control
                .commit(|| false, || panic!("lost device published"))
                .is_err()
        );
        control.fail("lost device");
        assert!(!control.ready());
        let (_, control) = fixture();
        assert!(
            control
                .commit(|| true, || bail!("registration conflict"))
                .is_err()
        );
        assert_eq!(
            control.status(),
            InitializationStatus::Unavailable("registration conflict".into())
        );
        let (_, control) = fixture();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            control.commit(|| true, || panic!("publisher panicked"))
        }));
        assert!(matches!(
            control.status(),
            InitializationStatus::Unavailable(_)
        ));
    }

    #[test]
    fn validation_does_not_inherit_startup_allowance_and_future_poll_is_bounded() {
        let (clock, control) = fixture();
        with_startup(&control, true, || {
            assert!(extended_wait());
            Ok(())
        })
        .unwrap();
        with_startup(&control, false, || {
            assert!(!extended_wait());
            Ok(())
        })
        .unwrap();
        assert!(preparation_control().is_none());
        struct PendingFuture(Arc<FakeClock>);
        impl Future for PendingFuture {
            type Output = ();
            fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<()> {
                self.0.advance(20);
                Poll::Pending
            }
        }
        assert!(with_startup(&control, true, || wait_future(PendingFuture(clock))).is_err());
        assert_eq!(control.status(), InitializationStatus::TimedOut);
        assert!(preparation_control().is_none());
    }
    #[test]
    fn deadline_and_cancellation_during_registration_prevent_ready() {
        for cancel in [false, true] {
            let (clock, control) = fixture();
            let result = control.commit(
                || true,
                || {
                    assert!(!control.ready());
                    if cancel {
                        control.transition(CANCELLED);
                    } else {
                        clock.advance(20);
                    }
                    assert!(!control.ready());
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert_eq!(
                control.status(),
                if cancel {
                    InitializationStatus::Cancelled
                } else {
                    InitializationStatus::TimedOut
                }
            );
        }
    }

    #[test]
    fn fault_event_and_ready_commit_are_serialized_in_both_orders() {
        let (_, control) = fixture();
        let failed = AtomicBool::new(false);
        control.device_fault(|| failed.store(true, Ordering::Release));
        assert!(
            control
                .commit(
                    || !failed.load(Ordering::Acquire),
                    || panic!("fault published")
                )
                .is_err()
        );
        assert!(!control.ready());

        let (_, control) = fixture();
        let failed = Arc::new(AtomicBool::new(false));
        let start_fault = Arc::new(Barrier::new(2));
        let thread = {
            let (control, failed, start_fault) =
                (control.clone(), failed.clone(), start_fault.clone());
            std::thread::spawn(move || {
                start_fault.wait();
                control.device_fault(|| failed.store(true, Ordering::Release));
            })
        };
        control
            .commit(
                || !failed.load(Ordering::Acquire),
                || {
                    start_fault.wait();
                    // Fault cannot change the predicate midway through the commit.
                    assert!(!failed.load(Ordering::Acquire));
                    Ok(())
                },
            )
            .unwrap();
        thread.join().unwrap();
        assert_eq!(control.status(), InitializationStatus::Ready); // startup completed first
        assert!(failed.load(Ordering::Acquire)); // runtime guard now declines
    }
}
