//! Editor timing (M9): the last relayout and page-paint durations, and the slowest paint since
//! the last reset. The autotest reads them; `PENKAKU_PERF=1` also logs each one to stderr.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static RELAYOUT_US: AtomicU64 = AtomicU64::new(0);
static PAINT_US: AtomicU64 = AtomicU64::new(0);
static PAINT_MAX_US: AtomicU64 = AtomicU64::new(0);
static PAINTS: AtomicU64 = AtomicU64::new(0);

fn logging() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| penkaku_core::env_var("PERF").is_some())
}

pub enum Kind {
    Relayout,
    Paint,
}

fn record(kind: Kind, us: u64) {
    let name = match kind {
        Kind::Relayout => {
            RELAYOUT_US.store(us, Ordering::Relaxed);
            "relayout"
        }
        Kind::Paint => {
            PAINT_US.store(us, Ordering::Relaxed);
            PAINT_MAX_US.fetch_max(us, Ordering::Relaxed);
            PAINTS.fetch_add(1, Ordering::Relaxed);
            "paint"
        }
    };
    if logging() {
        eprintln!("PERF {name} {:.2} ms", us as f64 / 1000.0);
    }
}

/// Time `f` and record it under `kind`.
pub fn time<T>(kind: Kind, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    record(kind, t.elapsed().as_micros() as u64);
    out
}

/// Records a paint from creation to drop (for a long paint closure).
pub struct PaintTimer(Instant);

impl PaintTimer {
    pub fn start() -> Self {
        Self(Instant::now())
    }
}

impl Drop for PaintTimer {
    fn drop(&mut self) {
        record(Kind::Paint, self.0.elapsed().as_micros() as u64);
    }
}

/// (last relayout, last paint, slowest paint since `reset`) in ms, and the paint count.
pub fn snapshot() -> (f64, f64, f64, u64) {
    let ms = |a: &AtomicU64| a.load(Ordering::Relaxed) as f64 / 1000.0;
    (ms(&RELAYOUT_US), ms(&PAINT_US), ms(&PAINT_MAX_US), PAINTS.load(Ordering::Relaxed))
}

pub fn reset() {
    PAINT_MAX_US.store(0, Ordering::Relaxed);
    PAINTS.store(0, Ordering::Relaxed);
}
