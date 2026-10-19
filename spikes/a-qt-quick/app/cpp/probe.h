// Frame probe for spike A: a preallocated ring buffer of QPC timestamps, the
// render-thread stall hook, and a thread-pooled artwork provider.
//
// Nothing in the per-frame path allocates, logs or does I/O. The buffer is
// written to disk only by spike_dump(), which runs after the measured window.
#pragma once

#include <QtQml/QQmlApplicationEngine>
#include <QtCore/QString>
#include <cstdint>

enum SpikeEvent : uint32_t {
    EvGuiAfterAnimating = 1,
    EvSyncBegin = 2,
    EvSyncEnd = 3,
    EvRenderBegin = 4,
    EvRenderEnd = 5,
    EvFrameSwapped = 6,
    EvStallGuiBegin = 7,
    EvStallGuiEnd = 8,
    EvStallRenderBegin = 9,
    EvStallRenderEnd = 10,
    EvInput = 11,
    EvMark = 12,
    EvQueueInsert = 13,
    EvQueueRemove = 14,
    EvQueueMove = 15,
    EvPageShown = 16,
    EvArtDecoded = 17,
};

// Called before the QML is loaded: fonts, text render type, artwork provider.
void spike_probe_setup(QQmlApplicationEngine &engine, int art_threads, const QString &asset_dir,
                       int text_render_type);
// Called after the QML is loaded: hooks the QQuickWindow signals.
void spike_probe_install(QQmlApplicationEngine &engine);
void spike_mark(uint32_t kind, uint32_t arg);
// Busy-waits the calling (GUI) thread for exactly `ms` milliseconds on QPC.
void spike_stall_gui(int ms);
// Arms a GUI-thread busy-wait of `ms` milliseconds at the next afterAnimating.
void spike_request_gui_stall(int ms);
// Arms a busy-wait of `ms` milliseconds at the next beforeRendering.
void spike_request_render_stall(int ms);
// Appends one memory sample to $SPIKE_MEM_OUT (long soaks).
void spike_log_memory();
// Writes the ring buffer as CSV. Returns the number of events written.
int64_t spike_dump(const QString &path);
