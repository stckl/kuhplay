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

use std::ffi::c_void;

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows_core::{interface, IUnknown, IUnknown_Vtbl, Interface, BSTR, GUID, HRESULT};

use crate::core::{ShowError, ShowResult};
use crate::engine::decklink::{
    bmd_display_mode, rgba_to_uyvy422_cs, ColorSpace, DecklinkConfig, BMD_FORMAT_8BIT_YUV,
};
use crate::engine::frame::VideoFrame;

// --- COM-Interfaces (vtable-Reihenfolge wie im IDL) -------------------------

/// IDeckLink (DeckLinkAPIDiscovery.idl).
#[interface("C418FBDD-0587-48ED-8FE5-640F0A14AF91")]
unsafe trait IDeckLink: IUnknown {
    fn GetModelName(&self, modelname: *mut BSTR) -> HRESULT;
    fn GetDisplayName(&self, displayname: *mut BSTR) -> HRESULT;
}

/// IDeckLinkIterator (DeckLinkAPI.idl) – enumeriert die Hardware.
#[interface("50FB36CD-3063-4B73-BDBB-958087F2D8BA")]
unsafe trait IDeckLinkIterator: IUnknown {
    fn Next(&self, decklinkinstance: *mut Option<IDeckLink>) -> HRESULT;
}

/// IDeckLinkVideoFrame (DeckLinkAPI.idl).
#[interface("3F716FE0-F023-4111-BE5D-EF4414C05B17")]
unsafe trait IDeckLinkVideoFrame: IUnknown {
    fn GetWidth(&self) -> i32;
    fn GetHeight(&self) -> i32;
    fn GetRowBytes(&self) -> i32;
    fn GetPixelFormat(&self) -> u32;
    fn GetFlags(&self) -> u32;
    // SDK 16.0: GetBytes liegt nicht mehr am Frame (v14_2_1-vtable), sondern
    // am IDeckLinkVideoBuffer (siehe unten).
}

/// IDeckLinkVideoBuffer (DeckLinkAPI.h, SDK 16.0) – Pixelpuffer des Frames.
#[interface("81F03D70-DE13-4B17-873A-C8AC9689C682")]
unsafe trait IDeckLinkVideoBuffer: IUnknown {
    fn GetBytes(&self, buffer: *mut *mut c_void) -> HRESULT;
    fn GetSize(&self, size: *mut u64) -> HRESULT;
    fn StartAccess(&self, flags: u32) -> HRESULT;
    fn EndAccess(&self, flags: u32) -> HRESULT;
}

/// bmdBufferAccessWrite (DeckLinkAPITypes.idl).
const BMD_BUFFER_ACCESS_WRITE: u32 = 1 << 1;

/// IDeckLinkMutableVideoFrame (DeckLinkAPI.idl) – von CreateVideoFrame.
#[interface("69E2639F-40DA-4E19-B6F2-20ACE815C390")]
unsafe trait IDeckLinkMutableVideoFrame: IDeckLinkVideoFrame {
    fn SetFlags(&self, newflags: u32) -> HRESULT;
}

/// IDeckLinkOutput (DeckLinkAPI.idl) – nur das benötigte vtable-Präfix.
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
    fn GetDisplayModeIterator(&self, iterator: *mut *mut c_void) -> HRESULT;
    fn SetScreenPreviewCallback(&self, previewcallback: *mut c_void) -> HRESULT;
    fn EnableVideoOutput(&self, displaymode: u32, flags: u32) -> HRESULT;
    fn DisableVideoOutput(&self) -> HRESULT;
    fn SetVideoOutputFrameMemoryAllocator(&self, theallocator: *mut c_void) -> HRESULT;
    fn CreateVideoFrame(
        &self,
        width: i32,
        height: i32,
        rowbytes: i32,
        pixelformat: u32,
        flags: u32,
        outframe: *mut Option<IDeckLinkMutableVideoFrame>,
    ) -> HRESULT;
    fn CreateAncillaryData(&self, pixelformat: u32, outbuffer: *mut *mut c_void) -> HRESULT;
    fn DisplayVideoFrameSync(&self, theframe: *mut c_void) -> HRESULT;
}

// CDeckLinkIterator-CLSIDs (SDK 16.0: BA6C6F44-… ist die aktuelle; die alten
// v10_8/v10_11-CLSIDs aus älteren Desktop-Video-Versionen bleiben als Fallback).
const ITERATOR_CLSIDS: [GUID; 4] = [
    GUID::from_u128(0xBA6C6F44_6DA5_4DCE_94AA_EE2D1372A676), // SDK 16.0 (aktuell)
    GUID::from_u128(0x1073A05C_D885_47E9_B3C6_129B3F9F648B), // v10.8
    GUID::from_u128(0x1F2E109A_8F4F_49E4_9203_135595CB6FA5), // v10.8
    GUID::from_u128(0x652615D4_26CD_4514_B161_2FD5072ED008), // v10.11
];

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
pub struct ComDeckLinkOutput {
    output: IDeckLinkOutput,
    frame: IDeckLinkMutableVideoFrame,
    buffer: IDeckLinkVideoBuffer,
    uyvy: Vec<u8>,
    width: u32,
    height: u32,
    device_name: String,
    frames_pushed: u64,
    active: bool,
}

impl ComDeckLinkOutput {
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
                uyvy: vec![0u8; (config.width * config.height * 2) as usize],
                width: config.width,
                height: config.height,
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
                .DisplayVideoFrameSync(self.frame.as_raw() as *mut c_void)
                .ok()
                .map_err(|e| {
                    ShowError::Engine(format!("DisplayVideoFrameSync fehlgeschlagen: {e}"))
                })?;
        }
        self.frames_pushed += 1;
        Ok(())
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
                let _ = self.output.DisableVideoOutput();
            }
            self.active = false;
        }
    }
}
