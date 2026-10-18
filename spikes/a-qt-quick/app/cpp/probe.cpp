#include "probe.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
// macOS CI smoke builds only: a monotonic clock in 100 ns units stands in
// for QPC. Timing is only measured on Windows.
#include <chrono>
#include <functional>
#include <thread>
#include <unistd.h>
#endif

#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtCore/QFile>
#include <QtCore/QRunnable>
#include <QtCore/QThreadPool>
#include <QtGui/QFontDatabase>
#include <QtGui/QGuiApplication>
#include <QtGui/QImageReader>
#include <QtGui/QPainter>
#include <QtGui/QPainterPath>
#include <QtQuick/QQuickImageProvider>
#include <QtQuick/QQuickWindow>

#include <atomic>
#include <memory>
#include <mutex>
#include <cstdio>
#include <vector>

namespace {

struct Event {
    int64_t qpc;
    uint32_t kind;
    uint32_t arg;
    uint32_t tid;
    uint32_t pad;
};

// Preallocated and pre-touched in spike_probe_setup, before anything is measured.
std::vector<Event> g_ring;
uint64_t g_mask = 0;
std::atomic<uint64_t> g_next{0};
std::atomic<int> g_render_stall_ms{0};
std::atomic<int> g_gui_stall_ms{0};
int64_t g_qpc_freq = 0;

inline int64_t qpc_now() {
#ifdef _WIN32
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return t.QuadPart;
#else
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch()).count() / 100;
#endif
}

inline uint32_t thread_id() {
#ifdef _WIN32
    return GetCurrentThreadId();
#else
    return static_cast<uint32_t>(std::hash<std::thread::id>{}(std::this_thread::get_id()));
#endif
}

inline void record(uint32_t kind, uint32_t arg) {
    if (g_mask == 0)
        return;
    const uint64_t i = g_next.fetch_add(1, std::memory_order_relaxed) & g_mask;
    Event &e = g_ring[i];
    e.qpc = qpc_now();
    e.kind = kind;
    e.arg = arg;
    e.tid = thread_id();
}

void spin_for_ms(int ms) {
    const int64_t end = qpc_now() + (g_qpc_freq * ms) / 1000;
    while (qpc_now() < end) {
#ifdef _WIN32
        YieldProcessor();
#endif
    }
}

class InputRecorder : public QObject {
public:
    using QObject::QObject;

protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        switch (event->type()) {
        case QEvent::KeyPress:
        case QEvent::Wheel:
        case QEvent::MouseButtonPress:
        case QEvent::MouseButtonRelease:
        case QEvent::TouchBegin:
        case QEvent::TouchEnd:
            // Only the window receives the event once, so this records arrival
            // on the GUI thread without duplicates from item propagation.
            if (qobject_cast<QQuickWindow *>(watched))
                record(EvInput, static_cast<uint32_t>(event->type()));
            break;
        default:
            break;
        }
        return false;
    }
};

// The response can be deleted by the pixmap reader (when the Image goes away)
// while its job still runs. The job only touches it through this guard.
struct ArtGuard {
    std::mutex mutex;
    class ArtResponse *response = nullptr;
};

class ArtResponse : public QQuickImageResponse {
public:
    ArtResponse() : m_guard(std::make_shared<ArtGuard>()) { m_guard->response = this; }
    ~ArtResponse() override {
        std::lock_guard lock(m_guard->mutex);
        m_guard->response = nullptr;
    }
    QQuickTextureFactory *textureFactory() const override {
        return QQuickTextureFactory::textureFactoryForImage(m_image);
    }
    std::shared_ptr<ArtGuard> guard() const { return m_guard; }
    void deliver(QImage image) {
        m_image = std::move(image);
        emit finished();
    }

private:
    std::shared_ptr<ArtGuard> m_guard;
    QImage m_image;
};

class ArtJob : public QRunnable {
public:
    ArtJob(std::shared_ptr<ArtGuard> guard, QString path, QSize size, uint32_t id, int radius_pct)
        : m_guard(std::move(guard)), m_path(std::move(path)), m_size(size), m_id(id),
          m_radius_pct(radius_pct) {}
    void run() override {
        {
            std::lock_guard lock(m_guard->mutex);
            if (!m_guard->response)
                return; // cancelled before it started
        }
        QImageReader reader(m_path);
        if (m_size.isValid())
            reader.setScaledSize(m_size);
        QImage image = reader.read();
        if (m_radius_pct > 0 && !image.isNull()) {
            // Round the corners here, on the worker, so the scene graph draws a
            // plain texture: no layer, mask or shape per row.
            QImage rounded(image.size(), QImage::Format_ARGB32_Premultiplied);
            rounded.fill(Qt::transparent);
            QPainter painter(&rounded);
            painter.setRenderHint(QPainter::Antialiasing);
            QPainterPath path;
            const qreal r = image.width() * m_radius_pct / 100.0;
            path.addRoundedRect(QRectF(rounded.rect()), r, r);
            painter.setClipPath(path);
            painter.drawImage(0, 0, image);
            painter.end();
            image = std::move(rounded);
        }
        record(EvArtDecoded, m_id);
        std::lock_guard lock(m_guard->mutex);
        if (ArtResponse *response = m_guard->response) {
            QMetaObject::invokeMethod(
                response, [response, image = std::move(image)]() mutable {
                    response->deliver(std::move(image));
                },
                Qt::QueuedConnection);
        }
    }

private:
    std::shared_ptr<ArtGuard> m_guard;
    QString m_path;
    QSize m_size;
    uint32_t m_id;
    int m_radius_pct;
};

// image://art/<n>[/r<radius %>][/u<unique>]. The unique part defeats the
// pixmap cache so that every bind decodes again.
class ArtProvider : public QQuickAsyncImageProvider {
public:
    ArtProvider(QString dir, int threads) : m_dir(std::move(dir)) {
        m_pool.setMaxThreadCount(threads);
    }
    QQuickImageResponse *requestImageResponse(const QString &id, const QSize &size) override {
        const QStringList parts = id.split(u'/');
        const uint32_t n = parts.first().toUInt();
        int radius_pct = 0;
        for (const QString &part : parts) {
            if (part.startsWith(u'r'))
                radius_pct = part.mid(1).toInt();
        }
        auto *response = new ArtResponse;
        m_pool.start(new ArtJob(response->guard(),
                                m_dir + QStringLiteral("/art/%1.jpg").arg(n % 512, 3, 10, QChar(u'0')),
                                size, n, radius_pct));
        return response;
    }

private:
    QString m_dir;
    QThreadPool m_pool;
};

} // namespace

void spike_probe_setup(QQmlApplicationEngine &engine, int art_threads, const QString &asset_dir,
                       int text_render_type) {
#ifdef _WIN32
    LARGE_INTEGER f;
    QueryPerformanceFrequency(&f);
    g_qpc_freq = f.QuadPart;
#else
    g_qpc_freq = 10'000'000;
#endif

    uint64_t entries = 1u << 20;
    const QByteArray ring_env = qgetenv("SPIKE_RING");
    if (!ring_env.isEmpty())
        entries = ring_env.toULongLong();
    uint64_t pow2 = 1;
    while (pow2 < entries)
        pow2 <<= 1;
    g_ring.assign(pow2, Event{0, 0, 0, 0, 0}); // touches every page now
    g_mask = pow2 - 1;

    for (const char *font : {"SchibstedGrotesk-Bold.ttf", "SchibstedGrotesk-ExtraBold.ttf",
                             "DMMono-Regular.ttf", "DMMono-Medium.ttf"}) {
        QFontDatabase::addApplicationFont(asset_dir + QStringLiteral("/fonts/") +
                                          QString::fromLatin1(font));
    }
    QQuickWindow::setTextRenderType(
        static_cast<QQuickWindow::TextRenderType>(text_render_type));
    engine.addImageProvider(QStringLiteral("art"), new ArtProvider(asset_dir, art_threads));
    qApp->installEventFilter(new InputRecorder(qApp));
}

void spike_probe_install(QQmlApplicationEngine &engine) {
    const auto roots = engine.rootObjects();
    QQuickWindow *window = roots.isEmpty() ? nullptr : qobject_cast<QQuickWindow *>(roots.first());
    if (!window) {
        std::fprintf(stderr, "probe: no QQuickWindow root\n");
        return;
    }
    // afterAnimating is emitted on the GUI thread. The others come from the
    // render thread under the threaded render loop (beforeSynchronizing runs
    // while the GUI thread is blocked for the sync).
    // A frame-aligned GUI stall runs here, inside the frame's GUI phase
    // (after the animation tick, before polish and sync), which is where real
    // binding updates and delegate creation cost time.
    QObject::connect(window, &QQuickWindow::afterAnimating, window, [] {
        record(EvGuiAfterAnimating, 0);
        const int ms = g_gui_stall_ms.exchange(0);
        if (ms > 0)
            spike_stall_gui(ms);
    }, Qt::DirectConnection);
    QObject::connect(window, &QQuickWindow::beforeSynchronizing, window,
                     [] { record(EvSyncBegin, 0); }, Qt::DirectConnection);
    QObject::connect(window, &QQuickWindow::afterSynchronizing, window,
                     [] { record(EvSyncEnd, 0); }, Qt::DirectConnection);
    QObject::connect(window, &QQuickWindow::beforeRendering, window, [] {
        record(EvRenderBegin, 0);
        const int ms = g_render_stall_ms.exchange(0);
        if (ms > 0) {
            record(EvStallRenderBegin, static_cast<uint32_t>(ms));
            spin_for_ms(ms);
            record(EvStallRenderEnd, static_cast<uint32_t>(ms));
        }
    }, Qt::DirectConnection);
    QObject::connect(window, &QQuickWindow::afterRendering, window,
                     [] { record(EvRenderEnd, 0); }, Qt::DirectConnection);
    QObject::connect(window, &QQuickWindow::frameSwapped, window,
                     [] { record(EvFrameSwapped, 0); }, Qt::DirectConnection);

    const QByteArray dump = qgetenv("SPIKE_RING_OUT");
    if (!dump.isEmpty()) {
        const QString path = QString::fromLocal8Bit(dump);
        QObject::connect(qApp, &QCoreApplication::aboutToQuit, [path] { spike_dump(path); });
    }
}

void spike_mark(uint32_t kind, uint32_t arg) { record(kind, arg); }

void spike_stall_gui(int ms) {
    record(EvStallGuiBegin, static_cast<uint32_t>(ms));
    spin_for_ms(ms);
    record(EvStallGuiEnd, static_cast<uint32_t>(ms));
}

void spike_request_render_stall(int ms) { g_render_stall_ms.store(ms); }

void spike_request_gui_stall(int ms) { g_gui_stall_ms.store(ms); }

int64_t spike_dump(const QString &path) {
#ifdef _WIN32
    FILE *f = _wfopen(reinterpret_cast<const wchar_t *>(path.utf16()), L"wb");
#else
    FILE *f = std::fopen(QFile::encodeName(path).constData(), "wb");
#endif
    if (!f)
        return -1;
    const uint64_t total = g_next.load();
    const uint64_t size = g_mask + 1;
    const uint64_t first = total > size ? total - size : 0;
    std::fprintf(f, "# qpc_freq=%lld gui_tid=%lu pid=%lu dropped=%llu\n",
                 static_cast<long long>(g_qpc_freq), static_cast<unsigned long>(thread_id()),
                 static_cast<unsigned long>(QCoreApplication::applicationPid()),
                 static_cast<unsigned long long>(first));
    std::fprintf(f, "qpc,kind,arg,tid\n");
    for (uint64_t i = first; i < total; ++i) {
        const Event &e = g_ring[i & g_mask];
        std::fprintf(f, "%lld,%u,%u,%u\n", static_cast<long long>(e.qpc), e.kind, e.arg, e.tid);
    }
    std::fclose(f);
    return static_cast<int64_t>(total - first);
}
