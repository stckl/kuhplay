//! Native DeckLink-Ausgabe auf Windows per COM-API (`DeckLinkAPI64.dll`,
//! registriert via `regsvr32` aus „Blackmagic Desktop Video"). Alternative zum
//! ffmpeg-`decklink`-Muxer, den die gängigen Windows-ffmpeg-Builds (gyan etc.)
//! **nicht** enthalten.
//!
//! Ablauf: `CoCreateInstance(CDeckLinkIterator)` → Geräte enumerieren
//! (`IDeckLink::GetDisplayName`) → per `QueryInterface` `IDeckLinkOutput` →
//! `EnableVideoOutput(mode, default)` → pro Frame `CreateVideoFrame`-Puffer
//! befüllen (`GetBytes`) → `DisplayVideoFrameSync`.
//!
//! Frames kommen als RGBA vom Compositor und werden hier nach UYVY (8-Bit-YUV,
//! '2vuy') konvertiert – siehe [`super::decklink::rgba_to_uyvy422`].
//!
//! GUIDs aus dem Blackmagic DeckLink SDK (Win/include/DeckLinkAPI*.idl).

// COM-Methoden behalten ihre IDL-Namen (PascalCase) – vtable-Reihenfolge ist
// namensunabhängig, aber die Namen dokumentieren die API 1:1.
#![allow(non_snake_case)]
// Die vtable-Signaturen kommen aus der SDK-IDL (DoesSupportVideoMode hat 8 Parameter).
#![allow(clippy::too_many_arguments)]

use std::ffi::c_void;

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows_core::{interface, IUnknown, IUnknown_Vtbl, Interface, BSTR, GUID, HRESULT};

use crate::core::{ShowError, ShowResult};
use crate::engine::decklink::{
    bmd_display_mode, rgba_to_uyvy422_cs, scheduled_time_params, ColorSpace, DecklinkConfig,
    BMD_FORMAT_8BIT_YUV,
};
use crate::engine::frame::VideoFrame;

// --- COM-Interfaces -------------------------------------------------------
// H1: Methode fuer Methode aus third_party/decklink-sdk-16.0/windows/include/
// DeckLinkAPI.idl (SDK 16.0) uebernommen – vtable-Reihenfolge und IIDs exakt.
// Der Test tests/decklink_com_idl.rs vergleicht beides automatisch gegen die IDL.

/// IDeckLink (DeckLinkAPIDiscovery.idl).
#[interface("C418FBDD-0587-48ED-8FE5-640F0A14AF91")]
unsafe trait IDeckLink: IUnknown {
    fn GetModelName(&self, modelname: *mut BSTR) -> HRESULT;
    fn GetDisplayName(&self, displayname: *mut BSTR) -> HRESULT;
}

/// IDeckLinkIterator (DeckLinkAPI.idl).
#[interface("50FB36CD-3063-4B73-BDBB-958087F2D8BA")]
unsafe trait IDeckLinkIterator: IUnknown {
    fn Next(&self, decklinkinstance: *mut Option<IDeckLink>) -> HRESULT;
}

/// IDeckLinkVideoFrame (DeckLinkAPI.idl, SDK 16.0: IID 6502091C-…, GetBytes entfällt).
#[interface("6502091C-615F-4F51-BAF6-45C4256DD5B0")]
unsafe trait IDeckLinkVideoFrame: IUnknown {
    fn GetWidth(&self) -> i32;
    fn GetHeight(&self) -> i32;
    fn GetRowBytes(&self) -> i32;
    fn GetPixelFormat(&self) -> u32;
    fn GetFlags(&self) -> u32;
    fn GetTimecode(&self, format: u32, timecode: *mut *mut c_void) -> HRESULT;
    fn GetAncillaryData(&self, ancillary: *mut *mut c_void) -> HRESULT;
}

/// IDeckLinkMutableVideoFrame (SDK 16.0: IID CF9EB134-…).
#[interface("CF9EB134-0374-4C5B-95FA-1EC14819FF62")]
unsafe trait IDeckLinkMutableVideoFrame: IDeckLinkVideoFrame {
    fn SetFlags(&self, newflags: u32) -> HRESULT;
    fn SetTimecode(&self, format: u32, timecode: *mut c_void) -> HRESULT;
    fn SetTimecodeFromComponents(
        &self,
        format: u32,
        hours: u8,
        minutes: u8,
        seconds: u8,
        frames: u8,
        flags: u32,
    ) -> HRESULT;
    fn SetAncillaryData(&self, ancillary: *mut c_void) -> HRESULT;
    fn SetTimecodeUserBits(&self, format: u32, userbits: u64) -> HRESULT;
    fn SetInterfaceProvider(&self, iid: *const GUID, iface: *mut c_void) -> HRESULT;
}

/// IDeckLinkVideoBuffer (SDK 16.0: Pixelpuffer, GetBytes liegt hier).
#[interface("81F03D70-DE13-4B17-873A-C8AC9689C682")]
unsafe trait IDeckLinkVideoBuffer: IUnknown {
    fn GetBytes(&self, buffer: *mut *mut c_void) -> HRESULT;
    fn GetSize(&self, size: *mut u64) -> HRESULT;
    fn StartAccess(&self, flags: u32) -> HRESULT;
    fn EndAccess(&self, flags: u32) -> HRESULT;
}

/// IDeckLinkVideoOutputCallback (SDK 16.0: IID 5BE6DF26-…).
#[interface("5BE6DF26-02CE-433E-99D9-9A87C3AC171F")]
unsafe trait IDeckLinkVideoOutputCallback: IUnknown {
    fn ScheduledFrameCompleted(&self, completedframe: *mut c_void, result: u32) -> HRESULT;
    fn ScheduledPlaybackHasStopped(&self) -> HRESULT;
}

/// IDeckLinkAPIInformation (DeckLinkAPI.idl, SDK 16.0) – Treiberversion u. a.
#[interface("7BEA3C68-730D-4322-AF34-8A7152B532A4")]
unsafe trait IDeckLinkAPIInformation: IUnknown {
    fn GetFlag(&self, cfgid: u32, value: *mut i32) -> HRESULT;
    fn GetInt(&self, cfgid: u32, value: *mut i64) -> HRESULT;
    fn GetFloat(&self, cfgid: u32, value: *mut f64) -> HRESULT;
    fn GetString(&self, cfgid: u32, value: *mut BSTR) -> HRESULT;
}

/// IDeckLinkDisplayMode (DeckLinkAPIModes.idl, SDK 16.0).
#[interface("3EB2C1AB-0A3D-4523-A3AD-F40D7FB14E78")]
unsafe trait IDeckLinkDisplayMode: IUnknown {
    fn GetName(&self, name: *mut BSTR) -> HRESULT;
    fn GetDisplayMode(&self) -> u32;
    fn GetWidth(&self) -> i32;
    fn GetHeight(&self) -> i32;
    fn GetFrameRate(&self, frameduration: *mut i64, timescale: *mut i64) -> HRESULT;
    fn GetFieldDominance(&self) -> u32;
    fn GetFlags(&self) -> u32;
}

/// IDeckLinkDisplayModeIterator (DeckLinkAPIModes.idl, SDK 16.0).
#[interface("9C88499F-F601-4021-B80B-032E4EB41C35")]
unsafe trait IDeckLinkDisplayModeIterator: IUnknown {
    fn Next(&self, mode: *mut Option<IDeckLinkDisplayMode>) -> HRESULT;
}

/// IDeckLinkOutput (DeckLinkAPI.idl, SDK 16.0) – komplette vtable-Liste in Reihenfolge.
#[interface("5F227C95-39D7-46C7-8B7D-9C81795FBBE4")]
unsafe trait IDeckLinkOutput: IUnknown {
    fn DoesSupportVideoMode(
        &self,
        connection: u32,
        requestedmode: u32,
        requestedpixelformat: u32,
        conversionmode: u32,
        flags: u32,
        actualmode: *mut u32,
        supported: *mut i32,
    ) -> HRESULT;
    fn GetDisplayMode(&self, displaymode: u32, resultdisplaymode: *mut *mut c_void) -> HRESULT;
    fn GetDisplayModeIterator(
        &self,
        iterator: *mut Option<IDeckLinkDisplayModeIterator>,
    ) -> HRESULT;
    fn SetScreenPreviewCallback(&self, previewcallback: *mut c_void) -> HRESULT;
    fn EnableVideoOutput(&self, displaymode: u32, flags: u32) -> HRESULT;
    fn DisableVideoOutput(&self) -> HRESULT;
    fn CreateVideoFrame(
        &self,
        width: i32,
        height: i32,
        rowbytes: i32,
        pixelformat: u32,
        flags: u32,
        outframe: *mut Option<IDeckLinkMutableVideoFrame>,
    ) -> HRESULT;
    fn CreateVideoFrameWithBuffer(
        &self,
        width: i32,
        height: i32,
        rowbytes: i32,
        pixelformat: u32,
        flags: u32,
        buffer: *mut c_void,
        outframe: *mut Option<IDeckLinkMutableVideoFrame>,
    ) -> HRESULT;
    fn RowBytesForPixelFormat(&self, pixelformat: u32, width: i32, rowbytes: *mut i32) -> HRESULT;
    fn CreateAncillaryData(&self, pixelformat: u32, outbuffer: *mut *mut c_void) -> HRESULT;
    fn DisplayVideoFrameSync(&self, theframe: *mut c_void) -> HRESULT;
    fn ScheduleVideoFrame(
        &self,
        theframe: *mut c_void,
        displaytime: i64,
        displayduration: i64,
        timescale: i64,
    ) -> HRESULT;
    fn SetScheduledFrameCompletionCallback(&self, thecallback: *mut c_void) -> HRESULT;
    fn GetBufferedVideoFrameCount(&self, bufferedframecount: *mut u32) -> HRESULT;
    fn EnableAudioOutput(
        &self,
        samplerate: u32,
        sampletype: u32,
        channelcount: u32,
        streamtype: u32,
    ) -> HRESULT;
    fn DisableAudioOutput(&self) -> HRESULT;
    fn WriteAudioSamplesSync(
        &self,
        buffer: *mut c_void,
        sampleframecount: u32,
        sampleframeswritten: *mut u32,
    ) -> HRESULT;
    fn BeginAudioPreroll(&self) -> HRESULT;
    fn EndAudioPreroll(&self) -> HRESULT;
    fn ScheduleAudioSamples(
        &self,
        buffer: *mut c_void,
        sampleframecount: u32,
        streamtime: i64,
        timescale: i64,
        sampleframeswritten: *mut u32,
    ) -> HRESULT;
    fn GetBufferedAudioSampleFrameCount(&self, bufferedsampleframecount: *mut u32) -> HRESULT;
    fn FlushBufferedAudioSamples(&self) -> HRESULT;
    fn SetAudioCallback(&self, thecallback: *mut c_void) -> HRESULT;
    fn StartScheduledPlayback(
        &self,
        playbackstarttime: i64,
        timescale: i64,
        playbackspeed: f64,
    ) -> HRESULT;
    fn StopScheduledPlayback(
        &self,
        stopplaybackattime: i64,
        actualstoptime: *mut i64,
        timescale: i64,
    ) -> HRESULT;
    fn IsScheduledPlaybackRunning(&self, active: *mut i32) -> HRESULT;
    fn GetScheduledStreamTime(
        &self,
        desiredtimescale: i64,
        streamtime: *mut i64,
        playbackspeed: *mut f64,
    ) -> HRESULT;
    fn GetReferenceStatus(&self, referencestatus: *mut u32) -> HRESULT;
    fn GetHardwareReferenceClock(
        &self,
        desiredtimescale: i64,
        hardwaretime: *mut i64,
        timeinframe: *mut i64,
        ticksperframe: *mut i64,
    ) -> HRESULT;
    fn GetFrameCompletionReferenceTimestamp(
        &self,
        theframe: *mut c_void,
        desiredtimescale: i64,
        framecompletiontimestamp: *mut i64,
    ) -> HRESULT;
}

// BMDBufferAccessFlags (DeckLinkAPI.idl, SDK 16.0): bmdBufferAccessWrite = 1 << 1.
const BMD_BUFFER_ACCESS_WRITE: u32 = 1 << 1;

// CDeckLinkIterator-CLSIDs (SDK 16.0: BA6C6F44-… ist die aktuelle; die alten
// v10_8/v10_11-CLSIDs aus älteren Desktop-Video-Versionen bleiben als Fallback).
const ITERATOR_CLSIDS: [GUID; 4] = [
    GUID::from_u128(0xBA6C6F44_6DA5_4DCE_94AA_EE2D1372A676), // SDK 16.0 (aktuell)
    GUID::from_u128(0x1073A05C_D885_47E9_B3C6_129B3F9F648B), // v10.8
    GUID::from_u128(0x1F2E109A_8F4F_49E4_9203_135595CB6FA5), // v10.8
    GUID::from_u128(0x652615D4_26CD_4514_B161_2FD5072ED008), // v10.11
];

/// U3: Treiberversion (Desktop Video) über CDeckLinkAPIInformation/'vers'.
pub fn driver_version() -> Option<String> {
    const CLSID_API_INFORMATION: GUID = GUID::from_u128(0x263CA19F_ED09_482E_9F9D_84005783A237);
    const BMD_DECKLINK_API_VERSION: u32 = 0x76657273; // 'vers'
    unsafe {
        let info: IDeckLinkAPIInformation =
            match CoCreateInstance(&CLSID_API_INFORMATION, None, CLSCTX_INPROC_SERVER) {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("CDeckLinkAPIInformation: {e}");
                    return None;
                }
            };
        let mut v: BSTR = BSTR::default();
        let hr = info.GetString(BMD_DECKLINK_API_VERSION, &mut v);
        if hr != HRESULT(0) {
            tracing::warn!("GetString(vers): {hr:?}");
            return None;
        }
        let s = v.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

fn create_iterator() -> ShowResult<IDeckLinkIterator> {
    unsafe {
        // COM initialisieren (Thread-lokal; Mehrfach-Calls sind OK).
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        for clsid in &ITERATOR_CLSIDS {
            if let Ok(it) = CoCreateInstance::<_, IDeckLinkIterator>(
                clsid,
                None::<&IUnknown>,
                CLSCTX_INPROC_SERVER,
            ) {
                return Ok(it);
            }
        }
        Err(ShowError::Engine(
            r"DeckLink COM: kein CDeckLinkIterator registriert – DeckLinkAPI64.dll per regsvr32 registrieren (liegt in C:/Program Files/Blackmagic Design/Blackmagic Desktop Video)"
                .into(),
        ))
    }
}

/// Listet die Display-Namen aller DeckLink-Geräte (für Logs/Device-Auswahl).
pub fn list_devices() -> ShowResult<Vec<String>> {
    let it = create_iterator()?;
    let mut names = Vec::new();
    loop {
        let mut dev: Option<IDeckLink> = None;
        let hr = unsafe { it.Next(&mut dev) };
        if hr != HRESULT(0) || dev.is_none() {
            break;
        }
        let dev = dev.unwrap();
        let mut name: BSTR = BSTR::default();
        let hr = unsafe { dev.GetDisplayName(&mut name) };
        if hr == HRESULT(0) && !name.is_empty() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Native DeckLink-Ausgabe per COM.
/// Geteilter State für Scheduled Playback (G4): Queue + Zähler, vom
/// COM-Callback (Treiber-Thread) und vom Engine-Thread (Push) genutzt.
/// Frames, die vor dem Start bei der Karte liegen (Preroll).
const PREROLL_FRAMES: usize = 3;
/// Eigene Frames für die geplante Ausgabe; sind alle unterwegs, wird verworfen.
const POOL_FRAMES: usize = 6;

/// Ein vorbereiteter Frame der Karte samt Pixelpuffer.
struct PoolFrame {
    frame: IDeckLinkMutableVideoFrame,
    buffer: IDeckLinkVideoBuffer,
}

/// Zustand der geplanten Ausgabe: vom Engine-Thread (Push) und vom
/// Treiber-Thread (Callback) genutzt. Treiberaufrufe laufen außerhalb des Locks.
pub struct ScheduledState {
    free: Vec<usize>,
    /// Frames bei der Karte, in Ausgabereihenfolge.
    in_flight: std::collections::VecDeque<usize>,
    next_time: i64,
    time_scale: i64,
    frame_dur: i64,
    started: bool,
    late: u64,
    dropped: u64,
}

#[windows_core::implement(IDeckLinkVideoOutputCallback)]
struct ScheduledCallback {
    state: std::sync::Arc<std::sync::Mutex<ScheduledState>>,
}

impl IDeckLinkVideoOutputCallback_Impl for ScheduledCallback_Impl {
    unsafe fn ScheduledFrameCompleted(&self, _completedframe: *mut c_void, result: u32) -> HRESULT {
        let mut st = self.state.lock().unwrap();
        if let Some(index) = st.in_flight.pop_front() {
            st.free.push(index);
        }
        // bmdOutputFrameCompletionResult: 0=Completed, 1=DisplayedLate, 2=Dropped, 3=Flushed
        match result {
            1 => st.late += 1,
            2 => st.dropped += 1,
            _ => {}
        }
        HRESULT(0)
    }
    unsafe fn ScheduledPlaybackHasStopped(&self) -> HRESULT {
        HRESULT(0)
    }
}

pub struct ComDeckLinkOutput {
    output: IDeckLinkOutput,
    frame: IDeckLinkMutableVideoFrame,
    buffer: IDeckLinkVideoBuffer,
    sched: Option<std::sync::Arc<std::sync::Mutex<ScheduledState>>>,
    pool: Vec<PoolFrame>,
    uyvy: Vec<u8>,
    width: u32,
    height: u32,
    fps: u32,
    device_name: String,
    frames_pushed: u64,
    active: bool,
}

impl ComDeckLinkOutput {
    /// Modi, die die Karte meldet: (Breite, Höhe, fps-Zähler, fps-Nenner, Halbbilder).
    pub fn display_modes(&self) -> Vec<(u32, u32, u32, u32, bool)> {
        /// bmdProgressiveFrame und bmdProgressiveSegmentedFrame.
        const PROGRESSIVE: [u32; 2] = [0x7072_6F67, 0x7073_6620];
        let mut modes = Vec::new();
        let mut iterator: Option<IDeckLinkDisplayModeIterator> = None;
        unsafe {
            if self.output.GetDisplayModeIterator(&mut iterator) != HRESULT(0) {
                return modes;
            }
            let Some(iterator) = iterator else {
                return modes;
            };
            loop {
                let mut mode: Option<IDeckLinkDisplayMode> = None;
                if iterator.Next(&mut mode) != HRESULT(0) {
                    break;
                }
                let Some(mode) = mode else {
                    break;
                };
                let (mut duration, mut scale) = (0i64, 0i64);
                if mode.GetFrameRate(&mut duration, &mut scale) == HRESULT(0)
                    && duration > 0
                    && scale > 0
                {
                    modes.push((
                        mode.GetWidth() as u32,
                        mode.GetHeight() as u32,
                        scale as u32,
                        duration as u32,
                        !PROGRESSIVE.contains(&mode.GetFieldDominance()),
                    ));
                }
            }
        }
        modes
    }

    /// Öffnet das Gerät (Name-Substring, case-insensitive; leer/„auto" = erstes
    /// Gerät mit Output) und aktiviert das Display-Mode.
    pub fn start(config: &DecklinkConfig) -> ShowResult<Self> {
        let mode = bmd_display_mode(config.width, config.height, config.fps).ok_or_else(|| {
            ShowError::Engine(format!(
                "decklink COM: kein BMD-DisplayMode für {}x{}@{}",
                config.width, config.height, config.fps
            ))
        })?;

        let it = create_iterator()?;
        let want = config.device.trim().to_lowercase();
        let mut chosen: Option<(IDeckLink, String)> = None;
        loop {
            let mut dev: Option<IDeckLink> = None;
            let hr = unsafe { it.Next(&mut dev) };
            if hr != HRESULT(0) || dev.is_none() {
                break;
            }
            let dev = dev.unwrap();
            let mut name: BSTR = BSTR::default();
            let _ = unsafe { dev.GetDisplayName(&mut name) };
            let name = name.to_string();
            let matches = want.is_empty() || want == "auto" || name.to_lowercase().contains(&want);
            if matches {
                if let Ok(output) = dev.cast::<IDeckLinkOutput>() {
                    chosen = Some((dev, name));
                    let _ = output; // Output-Interface wird unten erneut geholt
                    break;
                }
            }
        }
        let Some((dev, name)) = chosen else {
            let devices = list_devices().unwrap_or_default();
            return Err(ShowError::Engine(format!(
                "decklink COM: Gerät '{}' nicht gefunden (vorhanden: {})",
                config.device,
                if devices.is_empty() {
                    "keine".to_string()
                } else {
                    devices.join(", ")
                }
            )));
        };
        let output: IDeckLinkOutput = dev
            .cast()
            .map_err(|e| ShowError::Engine(format!("decklink COM: kein IDeckLinkOutput: {e}")))?;

        unsafe {
            output
                .EnableVideoOutput(mode, 0)
                .ok()
                .map_err(|e| ShowError::Engine(format!("EnableVideoOutput fehlgeschlagen: {e}")))?;
            let row_bytes = config.width as i32 * 2;
            let mut frame: Option<IDeckLinkMutableVideoFrame> = None;
            output
                .CreateVideoFrame(
                    config.width as i32,
                    config.height as i32,
                    row_bytes,
                    BMD_FORMAT_8BIT_YUV,
                    0,
                    &mut frame,
                )
                .ok()
                .map_err(|e| ShowError::Engine(format!("CreateVideoFrame fehlgeschlagen: {e}")))?;
            let frame = frame
                .ok_or_else(|| ShowError::Engine("CreateVideoFrame lieferte kein Frame".into()))?;
            let buffer: IDeckLinkVideoBuffer = frame
                .cast()
                .map_err(|e| ShowError::Engine(format!("QI IDeckLinkVideoBuffer: {e}")))?;
            Ok(Self {
                output,
                frame,
                buffer,
                sched: None,
                pool: Vec::new(),
                uyvy: vec![0u8; (config.width * config.height * 2) as usize],
                width: config.width,
                height: config.height,
                fps: config.fps,
                device_name: name,
                frames_pushed: 0,
                active: true,
            })
        }
    }

    /// Gerätename (DisplayName) des geöffneten Ports.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Pusht ein RGBA-Frame: RGBA→UYVY konvertieren, in den Card-Puffer
    /// kopieren, synchron ausgeben.
    pub fn push_frame(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        if frame.width != self.width || frame.height != self.height {
            return Err(ShowError::Engine(format!(
                "frame {}x{} passt nicht zur Decklink-Ausgabe {}x{}",
                frame.width, frame.height, self.width, self.height
            )));
        }
        // Review G1: Farbraum aus der Auflösung (HD/UHD → BT.709, SD → BT.601).
        let cs = ColorSpace::from_height(self.height);
        rgba_to_uyvy422_cs(frame, &mut self.uyvy, cs);
        let uyvy = std::mem::take(&mut self.uyvy);
        let r = self.push_uyvy(&uyvy);
        self.uyvy = uyvy;
        r
    }

    /// Pusht ein fertiges UYVY-Frame (rowBytes × height) direkt auf die Karte.
    /// SDK 16.0: Pixelpuffer über IDeckLinkVideoBuffer (StartAccess/GetBytes/EndAccess).
    pub fn push_uyvy(&mut self, uyvy: &[u8]) -> ShowResult<()> {
        if !self.active {
            return Err(ShowError::Engine(
                "decklink COM: Ausgabe deaktiviert".into(),
            ));
        }
        if self.sched.is_some() {
            return self.schedule_uyvy(uyvy);
        }
        unsafe {
            self.buffer
                .StartAccess(BMD_BUFFER_ACCESS_WRITE)
                .ok()
                .map_err(|e| ShowError::Engine(format!("StartAccess fehlgeschlagen: {e}")))?;
            let mut buf: *mut c_void = std::ptr::null_mut();
            self.buffer
                .GetBytes(&mut buf)
                .ok()
                .map_err(|e| ShowError::Engine(format!("GetBytes fehlgeschlagen: {e}")))?;
            if buf.is_null() {
                return Err(ShowError::Engine("GetBytes lieferte null".into()));
            }
            std::ptr::copy_nonoverlapping(uyvy.as_ptr(), buf as *mut u8, uyvy.len());
            self.buffer
                .EndAccess(BMD_BUFFER_ACCESS_WRITE)
                .ok()
                .map_err(|e| ShowError::Engine(format!("EndAccess fehlgeschlagen: {e}")))?;
            self.output
                .DisplayVideoFrameSync(self.frame.as_raw())
                .ok()
                .map_err(|e| {
                    ShowError::Engine(format!("DisplayVideoFrameSync fehlgeschlagen: {e}"))
                })?;
        }
        self.frames_pushed += 1;
        Ok(())
    }

    /// Plant einen Frame ein; nach dem Preroll startet die Ausgabe.
    fn schedule_uyvy(&mut self, uyvy: &[u8]) -> ShowResult<()> {
        let state = self.sched.clone().expect("geplante Ausgabe aktiv");
        self.frames_pushed += 1;
        let (index, mut time, frame_dur, time_scale, started) = {
            let mut st = state.lock().unwrap();
            let Some(index) = st.free.pop() else {
                st.dropped += 1;
                return Ok(());
            };
            st.in_flight.push_back(index);
            (index, st.next_time, st.frame_dur, st.time_scale, st.started)
        };
        let pooled = &self.pool[index];
        unsafe {
            pooled
                .buffer
                .StartAccess(BMD_BUFFER_ACCESS_WRITE)
                .ok()
                .map_err(|e| ShowError::Engine(format!("StartAccess fehlgeschlagen: {e}")))?;
            let mut buf: *mut c_void = std::ptr::null_mut();
            pooled
                .buffer
                .GetBytes(&mut buf)
                .ok()
                .map_err(|e| ShowError::Engine(format!("GetBytes fehlgeschlagen: {e}")))?;
            if buf.is_null() {
                return Err(ShowError::Engine("GetBytes lieferte null".into()));
            }
            std::ptr::copy_nonoverlapping(uyvy.as_ptr(), buf as *mut u8, uyvy.len());
            let _ = pooled.buffer.EndAccess(BMD_BUFFER_ACCESS_WRITE);
            // Frame käme zu spät: hinter die aktuelle Karten-Uhr legen und zählen.
            let mut underrun = false;
            if started {
                let (mut now, mut speed) = (0i64, 1.0f64);
                if self
                    .output
                    .GetScheduledStreamTime(time_scale, &mut now, &mut speed)
                    == HRESULT(0)
                    && time <= now
                {
                    time = now + frame_dur;
                    underrun = true;
                }
            }
            let hr =
                self.output
                    .ScheduleVideoFrame(pooled.frame.as_raw(), time, frame_dur, time_scale);
            let start_now = {
                let mut st = state.lock().unwrap();
                if underrun {
                    st.late += 1;
                }
                if hr != HRESULT(0) {
                    st.in_flight.pop_back();
                    st.free.push(index);
                    st.late += 1;
                    return Err(ShowError::Engine(format!("ScheduleVideoFrame: {hr:?}")));
                }
                st.next_time = time + frame_dur;
                let start = !st.started && st.in_flight.len() >= PREROLL_FRAMES;
                st.started |= start;
                start
            };
            if start_now {
                self.output
                    .StartScheduledPlayback(0, time_scale, 1.0)
                    .ok()
                    .map_err(|e| ShowError::Engine(format!("StartScheduledPlayback: {e}")))?;
            }
        }
        Ok(())
    }

    /// Bereitet die geplante Ausgabe vor: Die Karte ist Taktgeber (G4).
    pub fn start_scheduled(&mut self) -> ShowResult<()> {
        // H2: Zeitskala aus der konfigurierten Bildrate ableiten (25 → 25000/1000,
        // 29,97 → 30000/1001, …), analog decklink_bridge.cpp.
        let (time_scale, frame_dur) = scheduled_time_params(self.fps);
        let mut pool = Vec::with_capacity(POOL_FRAMES);
        for _ in 0..POOL_FRAMES {
            let mut frame: Option<IDeckLinkMutableVideoFrame> = None;
            unsafe {
                self.output
                    .CreateVideoFrame(
                        self.width as i32,
                        self.height as i32,
                        self.width as i32 * 2,
                        BMD_FORMAT_8BIT_YUV,
                        0,
                        &mut frame,
                    )
                    .ok()
                    .map_err(|e| ShowError::Engine(format!("CreateVideoFrame: {e}")))?;
            }
            let frame = frame
                .ok_or_else(|| ShowError::Engine("CreateVideoFrame lieferte kein Frame".into()))?;
            let buffer: IDeckLinkVideoBuffer = frame
                .cast()
                .map_err(|e| ShowError::Engine(format!("QI IDeckLinkVideoBuffer: {e}")))?;
            pool.push(PoolFrame { frame, buffer });
        }
        let state = std::sync::Arc::new(std::sync::Mutex::new(ScheduledState {
            free: (0..POOL_FRAMES).rev().collect(),
            in_flight: std::collections::VecDeque::new(),
            next_time: 0,
            time_scale,
            frame_dur,
            started: false,
            late: 0,
            dropped: 0,
        }));
        let callback: IDeckLinkVideoOutputCallback = ScheduledCallback {
            state: state.clone(),
        }
        .into();
        unsafe {
            self.output
                .SetScheduledFrameCompletionCallback(callback.as_raw())
                .ok()
                .map_err(|e| {
                    ShowError::Engine(format!("SetScheduledFrameCompletionCallback: {e}"))
                })?;
        }
        self.pool = pool;
        self.sched = Some(state);
        Ok(())
    }

    /// (late, dropped, queued) – G4-Statistik.
    pub fn stats(&self) -> (u64, u64, u64) {
        self.sched
            .as_ref()
            .map(|s| {
                let st = s.lock().unwrap();
                (st.late, st.dropped, st.in_flight.len() as u64)
            })
            .unwrap_or((0, 0, 0))
    }

    /// Anzahl geschriebener Frames.
    pub fn frames_pushed(&self) -> u64 {
        self.frames_pushed
    }

    /// Konfigurierte Frame-Breite.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Konfigurierte Frame-Höhe.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Läuft die Ausgabe noch?
    pub fn is_running(&mut self) -> bool {
        self.active
    }
}

// COM-Objekte der DeckLink-API werden nur vom Engine-Thread genutzt
// (Erzeugung und push_frame laufen dort); der Engine-Thread selbst braucht Send.
unsafe impl Send for ComDeckLinkOutput {}

impl Drop for ComDeckLinkOutput {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                if let Some(state) = &self.sched {
                    let (started, time_scale) = {
                        let st = state.lock().unwrap();
                        (st.started, st.time_scale)
                    };
                    if started {
                        let mut stopped_at = 0i64;
                        let _ = self
                            .output
                            .StopScheduledPlayback(0, &mut stopped_at, time_scale);
                    }
                    let _ = self
                        .output
                        .SetScheduledFrameCompletionCallback(std::ptr::null_mut());
                }
                let _ = self.output.DisableVideoOutput();
            }
            self.active = false;
        }
    }
}
