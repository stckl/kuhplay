// C-ABI-Bridge zur Blackmagic-DeckLink-API (SDK 16.0, Header in
// third_party/decklink-sdk-16.0/<plattform>/include). Wird nur auf macOS/Linux
// kompiliert (Windows nutzt den COM-Pfad in decklink_com.rs). Die Dispatch-
// Funktion CreateDeckLinkIteratorInstance() kommt aus der SDK-eigenen
// DeckLinkAPIDispatch.cpp (liegt im Include-Verzeichnis, lokal gebaut, nicht
// verteilt). Ohne installierten Treiber liefert sie NULL – die App meldet das
// als „Treiber fehlt".
//
// Layout: Iterator → IDeckLink (Name) → QueryInterface IDeckLinkOutput →
// EnableVideoOutput(mode) → CreateVideoFrame (UYVY-Puffer) → pro Frame
// GetBytes + memcpy + DisplayVideoFrameSync.

#include "DeckLinkAPI.h"

#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

#if defined(__APPLE__)
#include <CoreFoundation/CoreFoundation.h>
#endif

static char g_last_error[512] = {0};

// IID_IUnknown als CFUUIDBytes (macOS-COM ohne IID_IUnknown-Symbol)
static const REFIID kIUnknownUUID = { 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xC0,0x00,0x00,0x00,0x00,0x00,0x00,0x46 };

// Display-Name des Geräts als std::string (macOS: CFStringRef, Linux: const char*).
static std::string device_display_name(IDeckLink *dev) {
#if defined(__APPLE__)
    CFStringRef name = nullptr;
    if (dev->GetDisplayName(&name) != S_OK || !name)
        return "";
    char buf[256];
    bool ok = CFStringGetCString(name, buf, sizeof(buf), kCFStringEncodingUTF8);
    CFRelease(name);
    return ok ? buf : "";
#else
    const char *name = nullptr;
    if (dev->GetDisplayName(&name) != S_OK || !name)
        return "";
    return name;
#endif
}

static void set_err(const char *msg) {
    snprintf(g_last_error, sizeof(g_last_error), "%s", msg ? msg : "");
}

extern "C" int kp_dl_last_error(char *out, int outlen) {
    if (!out || outlen <= 0)
        return 0;
    snprintf(out, (size_t)outlen, "%s", g_last_error);
    return (int)strlen(g_last_error);
}

static IDeckLinkIterator *make_iterator() {
    return CreateDeckLinkIteratorInstance();
}

extern "C" int kp_dl_available(void) {
    IDeckLinkIterator *it = make_iterator();
    if (!it)
        return 0;
    it->Release();
    return 1;
}

extern "C" int kp_dl_device_count(void) {
    IDeckLinkIterator *it = make_iterator();
    if (!it) {
        set_err("no DeckLink driver (Desktop Video not installed/registered)");
        return -1;
    }
    int n = 0;
    IDeckLink *dev = nullptr;
    while (it->Next(&dev) == S_OK && dev) {
        n++;
        dev->Release();
        dev = nullptr;
    }
    it->Release();
    return n;
}

extern "C" int kp_dl_device_name(int index, char *out, int outlen) {
    if (!out || outlen <= 0)
        return 0;
    out[0] = 0;
    IDeckLinkIterator *it = make_iterator();
    if (!it)
        return 0;
    int n = 0;
    int ok = 0;
    IDeckLink *dev = nullptr;
    while (it->Next(&dev) == S_OK && dev) {
        if (n == index) {
            CFStringRef name = nullptr;
            if (dev->GetDisplayName(&name) == S_OK && name) {
                char buf[256];
                if (CFStringGetCString(name, buf, sizeof(buf), kCFStringEncodingUTF8)) {
                    snprintf(out, (size_t)outlen, "%s", buf);
                    ok = (int)strlen(out);
                }
                CFRelease(name);
            }
            dev->Release();
            break;
        }
        n++;
        dev->Release();
        dev = nullptr;
    }
    it->Release();
    return ok;
}

struct KpDecklink {
    IDeckLink *device = nullptr;
    IDeckLinkOutput *output = nullptr;
    IDeckLinkMutableVideoFrame *frame = nullptr;
    IDeckLinkVideoBuffer *buffer = nullptr; // SDK 16.0: GetBytes liegt am VideoBuffer, nicht am Frame
    int width = 0, height = 0, rowBytes = 0;
    BMDDisplayMode mode = 0;
    long frames_pushed = 0;
    // Scheduled Playback (G4): die Karte ist Taktgeber, wir reihen Frames ein.
    struct KpCallback *callback = nullptr;
    std::mutex queue_mtx;
    std::vector<std::vector<uint8_t>> queue;
    BMDTimeValue next_time = 0;   // nächste Frame-Zeit (timeScale-Einheiten)
    BMDTimeScale time_scale = 0;  // Einheiten pro Sekunde
    BMDTimeValue frame_dur = 0;   // Frame-Dauer in timeScale-Einheiten
    bool scheduled = false;
    bool in_flight = false;
    long late = 0, dropped = 0, scheduled_count = 0;
};

// IDeckLinkVideoOutputCallback: bei jedem fertigen Frame das nächste aus der
// Queue einreihen; verspätete/verlorene zählen (G4).
struct KpCallback : public IDeckLinkVideoOutputCallback {
public:
    KpDecklink *owner;
    explicit KpCallback(KpDecklink *o) : owner(o), refs(1) {}

    HRESULT STDMETHODCALLTYPE ScheduledFrameCompleted(IDeckLinkVideoFrame *completedFrame,
                                                      BMDOutputFrameCompletionResult result) override {
        (void)completedFrame;
        std::lock_guard<std::mutex> lock(owner->queue_mtx);
        owner->in_flight = false;
        if (result == bmdOutputFrameDropped)
            owner->dropped++;
        else if (result != bmdOutputFrameCompleted)
            owner->late++;
        schedule_next_locked();
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE ScheduledPlaybackHasStopped() override { return S_OK; }

    // requisitiere nächstes Frame, falls vorhanden
    void schedule_next_locked() {
        if (owner->queue.empty()) {
            // nichts eingereiht → Bild verloren (zählen, G4)
            if (owner->scheduled && !owner->in_flight)
                owner->dropped++;
            return;
        }
        if (owner->in_flight)
            return;
        auto data = std::move(owner->queue.front());
        owner->queue.erase(owner->queue.begin());
        void *buf = nullptr;
        if (owner->buffer->StartAccess(bmdBufferAccessWrite) == S_OK &&
            owner->buffer->GetBytes(&buf) == S_OK && buf) {
            memcpy(buf, data.data(), data.size());
            owner->buffer->EndAccess(bmdBufferAccessWrite);
        }
        owner->in_flight = true;
        owner->scheduled_count++;
        // Immer in der Zukunft der Karten-Uhr einreihen, sonst „late“ (G4).
        BMDTimeValue now_stream = 0;
        double speed = 1.0;
        if (owner->output->GetScheduledStreamTime(owner->time_scale, &now_stream, &speed) == S_OK
            && owner->next_time <= now_stream)
            owner->next_time = now_stream + owner->frame_dur;
        HRESULT hr = owner->output->ScheduleVideoFrame(owner->frame, owner->next_time, owner->frame_dur,
                                                       owner->time_scale);
        owner->next_time += owner->frame_dur;
        if (hr != S_OK) {
            owner->in_flight = false;
            owner->late++;
            char e[64];
            snprintf(e, sizeof(e), "ScheduleVideoFrame hr=0x%lx", (unsigned long)hr);
            set_err(e);
        }
    }

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **out) override {
        if (!out)
            return E_POINTER;
        if (memcmp(&iid, &IID_IDeckLinkVideoOutputCallback, sizeof(REFIID)) == 0 || memcmp(&iid, &kIUnknownUUID, sizeof(REFIID)) == 0) {
            *out = this;
            AddRef();
            return S_OK;
        }
        *out = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override {
        ULONG r = --refs;
        if (r == 0)
            delete this;
        return r;
    }

private:
    ULONG refs;
};

extern "C" void kp_dl_close(void *handle);

extern "C" void *kp_dl_open(const char *name_substr, uint32_t mode, int width, int height) {
    g_last_error[0] = 0;
    if (!mode) {
        set_err("unsupported video mode (use 1280x720, 1920x1080 or 3840x2160 with 23.98-60 fps)");
        return nullptr;
    }
    IDeckLinkIterator *it = make_iterator();
    if (!it) {
        set_err("no DeckLink driver (Desktop Video not installed/registered)");
        return nullptr;
    }
    KpDecklink *h = nullptr;
    IDeckLink *dev = nullptr;
    std::string want = name_substr ? name_substr : "";
    // leer/"auto" = erstes Gerät mit Output
    while (it->Next(&dev) == S_OK && dev) {
        std::string dn = device_display_name(dev);
        bool match = want.empty() || want == "auto" || dn.find(want) != std::string::npos;
        IDeckLinkOutput *out = nullptr;
        if (match && dev->QueryInterface(IID_IDeckLinkOutput, (void **)&out) == S_OK && out) {
            h = new KpDecklink();
            h->device = dev;
            h->output = out;
            break;
        }
        if (out)
            out->Release();
        dev->Release();
        dev = nullptr;
    }
    it->Release();
    if (!h) {
        set_err("device not found");
        return nullptr;
    }
    // Modus prüfen (neue Signatur mit Connection/ConversionMode).
    BMDDisplayMode actual = 0;
    bool supported = false;
    h->output->DoesSupportVideoMode(bmdVideoConnectionUnspecified, mode, bmdFormat8BitYUV,
                                    bmdNoVideoOutputConversion, bmdSupportedVideoModeDefault,
                                    &actual, &supported);
    if (!supported) {
        set_err("video mode not supported by this card");
        kp_dl_close(h);
        return nullptr;
    }
    if (h->output->EnableVideoOutput(mode, bmdVideoOutputFlagDefault) != S_OK) {
        set_err("EnableVideoOutput failed");
        kp_dl_close(h);
        return nullptr;
    }
    h->width = width;
    h->height = height;
    h->rowBytes = width * 2;
    h->mode = mode;
    // Zeitskala für Scheduled Playback (G4): timeScale = fps_num, frame_dur = fps_den.
    // 25 fps → timeScale 25000, dur 1000; 29,97 → 30000/1001.
    int fps_num = 25000, fps_den = 1000;
    switch (mode) {
    case bmdModeHD1080p2398: case bmdMode4K2160p2398: fps_num = 24000; fps_den = 1001; break;
    case bmdModeHD1080p24: case bmdMode4K2160p24: fps_num = 24000; fps_den = 1000; break;
    case bmdModeHD1080p25: case bmdMode4K2160p25: fps_num = 25000; fps_den = 1000; break;
    case bmdModeHD1080p2997: case bmdMode4K2160p2997: fps_num = 30000; fps_den = 1001; break;
    case bmdModeHD1080p30: case bmdMode4K2160p30: fps_num = 30000; fps_den = 1000; break;
    case bmdModeHD1080p50: case bmdMode4K2160p50: fps_num = 50000; fps_den = 1000; break;
    case bmdModeHD1080p5994: case bmdMode4K2160p5994: fps_num = 60000; fps_den = 1001; break;
    case bmdModeHD1080p6000: case bmdMode4K2160p60: fps_num = 60000; fps_den = 1000; break;
    case bmdModeHD720p50: fps_num = 50000; fps_den = 1000; break;
    case bmdModeHD720p5994: fps_num = 60000; fps_den = 1001; break;
    case bmdModeHD720p60: fps_num = 60000; fps_den = 1000; break;
    default: fps_num = 25000; fps_den = 1000; break;
    }
    h->time_scale = fps_num;
    h->frame_dur = fps_den;
    h->next_time = 0;
    h->callback = new KpCallback(h);
    if (h->output->CreateVideoFrame(width, height, h->rowBytes, bmdFormat8BitYUV, bmdFrameFlagDefault,
                                    &h->frame) != S_OK ||
        !h->frame) {
        set_err("CreateVideoFrame failed");
        kp_dl_close(h);
        return nullptr;
    }
    // SDK 16.0: Pixelpuffer über IDeckLinkVideoBuffer (GetBytes am Frame entfiel).
    if (h->frame->QueryInterface(IID_IDeckLinkVideoBuffer, (void **)&h->buffer) != S_OK || !h->buffer) {
        set_err("QueryInterface(IID_IDeckLinkVideoBuffer) failed");
        kp_dl_close(h);
        return nullptr;
    }
    return h;
}

extern "C" int kp_dl_mode_count(void *handle) {
    auto *h = (KpDecklink *)handle;
    if (!h || !h->output)
        return -1;
    IDeckLinkDisplayModeIterator *it = nullptr;
    if (h->output->GetDisplayModeIterator(&it) != S_OK || !it)
        return 0;
    int n = 0;
    IDeckLinkDisplayMode *m = nullptr;
    while (it->Next(&m) == S_OK && m) {
        n++;
        m->Release();
        m = nullptr;
    }
    it->Release();
    return n;
}

extern "C" void kp_dl_mode(void *handle, int index, int *out_w, int *out_h, int *fps_num, int *fps_den,
                           int *interlaced) {
    auto *h = (KpDecklink *)handle;
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
    if (!h || !h->output)
        return;
    IDeckLinkDisplayModeIterator *it = nullptr;
    if (h->output->GetDisplayModeIterator(&it) != S_OK || !it)
        return;
    int n = 0;
    IDeckLinkDisplayMode *m = nullptr;
    while (it->Next(&m) == S_OK && m) {
        if (n == index) {
            if (out_w) *out_w = (int)m->GetWidth();
            if (out_h) *out_h = (int)m->GetHeight();
            BMDTimeValue dur = 0;
            BMDTimeScale scale = 0;
            m->GetFrameRate(&dur, &scale);
            if (fps_num) *fps_num = scale > 0 ? (int)scale : 0;
            if (fps_den) *fps_den = dur > 0 ? (int)dur : 0;
            if (interlaced) *interlaced = (m->GetFieldDominance() != bmdProgressiveFrame) ? 1 : 0;
            m->Release();
            break;
        }
        n++;
        m->Release();
        m = nullptr;
    }
    it->Release();
}

extern "C" int kp_dl_start_scheduled(void *handle) {
    auto *h = (KpDecklink *)handle;
    if (!h || !h->output || !h->callback)
        return -1;
    if (h->output->SetScheduledFrameCompletionCallback(h->callback) != S_OK)
        return -2;
    if (h->output->StartScheduledPlayback(0, h->time_scale, 1.0) != S_OK)
        return -3;
    // Nächste Frame-Zeit an die aktuelle Stream-Zeit der Karte koppeln,
    // sonst wären alle ersten Frames sofort „late“ (G4).
    BMDTimeValue stream_time = 0;
    double speed = 1.0;
    if (h->output->GetScheduledStreamTime(h->time_scale, &stream_time, &speed) == S_OK)
        h->next_time = stream_time + h->frame_dur;
    else
        h->next_time = h->frame_dur;
    h->scheduled = true;
    return 0;
}

extern "C" void kp_dl_stats(void *handle, long *late, long *dropped, long *queued, long *scheduled_count) {
    auto *h = (KpDecklink *)handle;
    if (!h)
        return;
    std::lock_guard<std::mutex> lock(h->queue_mtx);
    if (late) *late = h->late;
    if (dropped) *dropped = h->dropped;
    if (queued) *queued = (long)h->queue.size();
    if (scheduled_count) *scheduled_count = h->scheduled_count;
}

extern "C" int kp_dl_push(void *handle, const unsigned char *uyvy, int len) {
    auto *h = (KpDecklink *)handle;
    if (!h || !h->output || !h->frame)
        return -1;
    if (len != h->rowBytes * h->height) {
        set_err("frame size mismatch");
        return -2;
    }
    // Scheduled Playback (G4): Frame einreihen; die Karte holt es im Takt.
    if (h->scheduled) {
        std::lock_guard<std::mutex> lock(h->queue_mtx);
        h->queue.emplace_back(uyvy, uyvy + len);
        h->callback->schedule_next_locked();
        h->frames_pushed++;
        return 0;
    }
    void *buf = nullptr;
    if (h->buffer->StartAccess(bmdBufferAccessWrite) != S_OK ||
        h->buffer->GetBytes(&buf) != S_OK || !buf) {
        set_err("video buffer access failed");
        return -3;
    }
    memcpy(buf, uyvy, (size_t)len);
    h->buffer->EndAccess(bmdBufferAccessWrite);
    if (h->output->DisplayVideoFrameSync(h->frame) != S_OK) {
        set_err("DisplayVideoFrameSync failed");
        return -4;
    }
    h->frames_pushed++;
    return 0;
}

extern "C" long kp_dl_frames_pushed(void *handle) {
    auto *h = (KpDecklink *)handle;
    return h ? h->frames_pushed : 0;
}

extern "C" void kp_dl_close(void *handle) {
    auto *h = (KpDecklink *)handle;
    if (!h)
        return;
    if (h->output) {
        if (h->scheduled)
            h->output->StopScheduledPlayback(0, nullptr, h->time_scale);
        h->output->SetScheduledFrameCompletionCallback(nullptr);
        h->output->DisableVideoOutput();
        h->output->Release();
    }
    if (h->callback)
        h->callback->Release();
    if (h->buffer)
        h->buffer->Release();
    if (h->frame)
        h->frame->Release();
    if (h->device)
        h->device->Release();
    delete h;
}
