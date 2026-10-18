//! The `Spike` singleton: run configuration, the stall hooks for the positive
//! control, and markers for the ring buffer.

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;

        include!("qtspike/cpp/probe.h");
        fn spike_probe_setup(
            engine: Pin<&mut QQmlApplicationEngine>,
            art_threads: i32,
            asset_dir: &QString,
            text_render_type: i32,
        );
        fn spike_probe_install(engine: Pin<&mut QQmlApplicationEngine>);
        fn spike_mark(kind: u32, arg: u32);
        fn spike_stall_gui(ms: i32);
        fn spike_request_gui_stall(ms: i32);
        fn spike_request_render_stall(ms: i32);
        fn spike_log_memory();
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, rows, READ, CONSTANT)]
        #[qproperty(i32, weight, READ, CONSTANT)]
        #[qproperty(bool, art_cache, cxx_name = "artCache", READ, CONSTANT)]
        #[qproperty(i32, glow, READ, CONSTANT)]
        #[qproperty(bool, fresh_pages, cxx_name = "freshPages", READ, CONSTANT)]
        #[qproperty(bool, async_pages, cxx_name = "asyncPages", READ, CONSTANT)]
        #[qproperty(bool, memory_log, cxx_name = "memoryLog", READ, CONSTANT)]
        #[qproperty(i32, queue_length, cxx_name = "queueLength", READ, CONSTANT)]
        #[qproperty(i32, transition_ms, cxx_name = "transitionMs", READ, CONSTANT)]
        #[qproperty(i32, churn_per_second, cxx_name = "churnPerSecond", READ, CONSTANT)]
        #[qproperty(i32, queue_ops_per_second, cxx_name = "queueOpsPerSecond", READ, CONSTANT)]
        #[qproperty(bool, heartbeat, READ, CONSTANT)]
        #[qproperty(bool, animators, READ, CONSTANT)]
        #[qproperty(bool, queue_fix, cxx_name = "queueFix", READ, CONSTANT)]
        #[qproperty(bool, specimen, READ, CONSTANT)]
        #[qproperty(i32, start_page, cxx_name = "startPage", READ, CONSTANT)]
        #[qproperty(i32, quit_after_ms, cxx_name = "quitAfterMs", READ, CONSTANT)]
        #[qproperty(i32, burst_count, cxx_name = "burstCount", READ, CONSTANT)]
        #[qproperty(i32, burst_gap_ms, cxx_name = "burstGapMs", READ, CONSTANT)]
        #[qproperty(i32, burst_period_ms, cxx_name = "burstPeriodMs", READ, CONSTANT)]
        type Spike = super::SpikeRust;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        /// Busy-waits the GUI thread inside its next frame (positive control).
        #[qinvokable]
        fn stall_gui(self: &Spike, ms: i32);
        /// Busy-waits the GUI thread now, wherever that falls in the frame.
        #[qinvokable]
        fn stall_gui_now(self: &Spike, ms: i32);
        /// Busy-waits the render thread at its next frame (positive control).
        #[qinvokable]
        fn stall_render(self: &Spike, ms: i32);
        #[qinvokable]
        fn mark(self: &Spike, kind: u32, arg: u32);
        /// Appends a memory sample to $SPIKE_MEM_OUT.
        #[qinvokable]
        fn log_memory(self: &Spike);
    }
}

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub struct SpikeRust {
    rows: i32,
    weight: i32,
    art_cache: bool,
    glow: i32,
    fresh_pages: bool,
    async_pages: bool,
    memory_log: bool,
    queue_length: i32,
    transition_ms: i32,
    churn_per_second: i32,
    queue_ops_per_second: i32,
    heartbeat: bool,
    animators: bool,
    queue_fix: bool,
    specimen: bool,
    start_page: i32,
    quit_after_ms: i32,
    burst_count: i32,
    burst_gap_ms: i32,
    burst_period_ms: i32,
}

impl Default for SpikeRust {
    fn default() -> Self {
        Self {
            rows: env("SPIKE_ROWS", 1000),
            // 0 = text only, 1 = artwork, 2 = artwork + glow + hover states.
            weight: env("SPIKE_WEIGHT", 1),
            art_cache: env("SPIKE_ART_CACHE", 1) != 0,
            // 0 = none, 1 = glow on the play control, 2 = glow + blurred backdrop.
            glow: env("SPIKE_GLOW", 1),
            fresh_pages: env("SPIKE_FRESH_PAGES", 0) != 0,
            async_pages: env("SPIKE_ASYNC_PAGES", 0) != 0,
            memory_log: std::env::var_os("SPIKE_MEM_OUT").is_some(),
            queue_length: env("SPIKE_QUEUE", 40),
            transition_ms: env("SPIKE_TRANSITION_MS", 220),
            churn_per_second: env("SPIKE_CHURN", 0),
            queue_ops_per_second: env("SPIKE_QUEUE_OPS", 0),
            heartbeat: env("SPIKE_HEARTBEAT", 1) != 0,
            // Render-thread Animators halve the GUI thread's frame rate on
            // flip-model swap chains and keep presenting while the GUI thread
            // is blocked (hiding GUI stalls from PresentMon). Off by default.
            animators: env("SPIKE_ANIMATORS", 0) != 0,
            // Qt's documented handling of interrupted view transitions.
            queue_fix: env("SPIKE_QUEUE_FIX", 1) != 0,
            specimen: env("SPIKE_SPECIMEN", 0) != 0,
            start_page: env("SPIKE_PAGE", 0),
            // CI smoke runs quit on their own so the ring buffer gets dumped.
            quit_after_ms: env("SPIKE_QUIT_AFTER_MS", 0),
            burst_count: env("SPIKE_BURST_COUNT", 0),
            burst_gap_ms: env("SPIKE_BURST_GAP_MS", 30),
            burst_period_ms: env("SPIKE_BURST_PERIOD_MS", 1000),
        }
    }
}

impl ffi::Spike {
    fn stall_gui(&self, ms: i32) {
        ffi::spike_request_gui_stall(ms);
    }

    fn stall_gui_now(&self, ms: i32) {
        ffi::spike_stall_gui(ms);
    }

    fn stall_render(&self, ms: i32) {
        ffi::spike_request_render_stall(ms);
    }

    fn mark(&self, kind: u32, arg: u32) {
        ffi::spike_mark(kind, arg);
    }

    fn log_memory(&self) {
        ffi::spike_log_memory();
    }
}
