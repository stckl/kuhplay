//! Vollbild-Ausgabe auf macOS: Fenster exakt auf den Screen legen.
//!
//! macOS schiebt normale Fenster unter die Menüleiste, das Bild wäre dann versetzt
//! und unten abgeschnitten. Auf der Ebene des Bildschirmschoners gilt diese
//! Begrenzung nicht, das Fenster liegt dann auch über der Menüleiste. egui bietet
//! dafür nichts an, deshalb direkt über AppKit.

use objc2_06::runtime::{AnyClass, AnyObject};
use objc2_06::{msg_send, rc::Retained};
use objc2_foundation_03::{NSPoint, NSRect, NSSize, NSString};

/// `NSScreenSaverWindowLevel`.
const OUTPUT_LEVEL: isize = 1000;
/// `canJoinAllSpaces | stationary | fullScreenAuxiliary`.
const OUTPUT_BEHAVIOR: usize = (1 << 0) | (1 << 4) | (1 << 8);

/// Legt das Fenster mit dem Titel `title` auf das Rechteck in globalen
/// Display-Koordinaten (Ursprung oben links). Muss im Main-Thread laufen.
pub fn place_output_window(title: &str, x: f64, y: f64, width: f64, height: f64) {
    let (Some(app), Some(screen_class)) =
        (AnyClass::get(c"NSApplication"), AnyClass::get(c"NSScreen"))
    else {
        return;
    };
    // SAFETY: Standard-AppKit-Selektoren mit passenden Signaturen; alle Aufrufer
    // sind Viewport-Callbacks auf dem Main-Thread.
    unsafe {
        let screens: *mut AnyObject = msg_send![screen_class, screens];
        let screen_count: usize = msg_send![screens, count];
        if screen_count == 0 {
            return;
        }
        // Cocoa zählt Y von unten, bezogen auf das Hauptdisplay (erstes Element).
        let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
        let primary_frame: NSRect = msg_send![primary, frame];
        let target = NSRect::new(
            NSPoint::new(x, primary_frame.size.height - y - height),
            NSSize::new(width, height),
        );

        let app: *mut AnyObject = msg_send![app, sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let window_title: Option<Retained<NSString>> = msg_send![window, title];
            if window_title.is_none_or(|t| t.to_string() != title) {
                continue;
            }
            let level: isize = msg_send![window, level];
            if level != OUTPUT_LEVEL {
                let _: () = msg_send![window, setLevel: OUTPUT_LEVEL];
                let _: () = msg_send![window, setCollectionBehavior: OUTPUT_BEHAVIOR];
            }
            let frame: NSRect = msg_send![window, frame];
            let differs = (frame.origin.x - target.origin.x).abs() > 0.5
                || (frame.origin.y - target.origin.y).abs() > 0.5
                || (frame.size.width - target.size.width).abs() > 0.5
                || (frame.size.height - target.size.height).abs() > 0.5;
            if differs {
                let _: () = msg_send![window, setFrame: target, display: true];
            }
        }
    }
}
