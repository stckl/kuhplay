//! Route-Renderer-PNGs: erzeugt Bilder aus dem Route-Renderer (zwei Regionen
//! mit Überlapp, alle Skalierungen, Grid) und prüft sie programatisch. Die
//! PNGs landen in /tmp/kuhplay-verify und lassen sich ansehen.

use std::collections::HashMap;
use std::sync::Arc;

use kuhplay::core::{OutputRoute, Region, RouteScaling, ScreenRotation, Stage, StageGeometry};
use kuhplay::engine::frame::VideoFrame;
use kuhplay::engine::route_renderer::{auto_blends, present_on_device, render_route};

fn solid(w: u32, h: u32, color: [u8; 3]) -> Arc<VideoFrame> {
    let [r, g, b] = color;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    Arc::new(VideoFrame::new(w, h, rgba))
}

/// Split-Frame: links rot, rechts blau (harte Kante bei split).
fn split(w: u32, h: u32, split: u32) -> Arc<VideoFrame> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let c = if x < split { [255, 0, 0] } else { [0, 64, 255] };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    Arc::new(VideoFrame::new(w, h, rgba))
}

fn save_png(name: &str, frame: &VideoFrame) {
    let dir = "/tmp/kuhplay-verify";
    std::fs::create_dir_all(dir).unwrap();
    let path = format!("{dir}/{name}.png");
    let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap();
    img.save(&path).expect("PNG schreiben");
    println!("PNG: {path} ({}x{})", frame.width, frame.height);
}

fn pixel(frame: &VideoFrame, x: u32, y: u32) -> [u8; 3] {
    let i = (y as usize * frame.width as usize + x as usize) * 4;
    [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
}

#[test]
fn verify_two_regions_with_overlap_pngs() {
    // Stage 2200 breit: Region A 0..1200 (Route L 1200x1080), B 1000..2200
    // (Route R 1200x1080), 200 px Überlappung, Auto-Blend.
    let route_l = OutputRoute::new("L", 1200, 1080);
    let route_r = OutputRoute::new("R", 1200, 1080);
    let mut stage = Stage::new("S", StageGeometry::new(2200.0, 1080.0));
    let mut a = Region::new(0.0, 0.0, 1200.0, 1080.0);
    a.route = Some(route_l.id);
    let mut b = Region::new(1000.0, 0.0, 1200.0, 1080.0);
    b.route = Some(route_r.id);
    stage.regions = vec![a, b];
    let frames: HashMap<String, Arc<VideoFrame>> =
        std::iter::once(("S".to_string(), split(2200, 1080, 1100))).collect();
    let left = render_route(&route_l, std::slice::from_ref(&stage), &frames);
    let right = render_route(&route_r, std::slice::from_ref(&stage), &frames);
    save_png("zwei-regionen-route-links", &left);
    save_png("zwei-regionen-route-rechts", &right);
    // Kern voll, Blende dunkler Richtung Kante.
    assert_eq!(pixel(&left, 10, 540), [255, 0, 0]);
    assert!(pixel(&left, 1190, 540)[0] < 100, "rechts abgeblendet");
    assert!(pixel(&right, 10, 540)[0] < 100, "links abgeblendet");
    assert_eq!(pixel(&right, 1190, 540), [0, 64, 255]);
}

#[test]
fn verify_scalings_png() {
    // 1920x1080-Route auf 1000x1000-Device, alle vier Skalierungen.
    let frame = split(1920, 1080, 960);
    let mut route = OutputRoute::new("D", 1920, 1080);
    for (name, scaling) in [
        ("fit", RouteScaling::Fit),
        ("fill", RouteScaling::Fill),
        ("stretch", RouteScaling::Stretch),
        ("center", RouteScaling::Center),
    ] {
        route.scaling = scaling;
        let out = present_on_device(&frame, &route, 1000, 1000);
        save_png(&format!("skalierung-{name}"), &out);
        assert_eq!((out.width, out.height), (1000, 1000));
    }
    // Drehung 90 Grad und Spiegelung als Bilder.
    route.scaling = RouteScaling::Stretch;
    route.rotation = ScreenRotation::Deg90;
    let out = present_on_device(&frame, &route, 400, 700);
    save_png("drehung-90", &out);
    route.rotation = ScreenRotation::Deg0;
    route.rear_projection = true;
    let out = present_on_device(&frame, &route, 400, 700);
    save_png("rueckprojektion", &out);
}

#[test]
fn verify_region_grid_png() {
    // Route mit Testbild + Region mit Grid: Raster nur über der Region.
    let route = OutputRoute::new("T", 640, 360);
    // Route-Testbild aus: Der Nachweis gilt dem Region-Grid allein.
    let mut stage = Stage::new("S", StageGeometry::new(1280.0, 720.0));
    let mut a = Region::new(0.0, 0.0, 640.0, 360.0);
    a.route = Some(route.id);
    a.grid = true;
    let mut b = Region::new(640.0, 0.0, 640.0, 360.0);
    let route2 = OutputRoute::new("T2", 640, 360);
    b.route = Some(route2.id);
    stage.regions = vec![a, b];
    let frames: HashMap<String, Arc<VideoFrame>> =
        std::iter::once(("S".to_string(), solid(1280, 720, [12, 34, 56]))).collect();
    let out = render_route(&route, std::slice::from_ref(&stage), &frames);
    save_png("route-testbild-mit-region-grid", &out);
    assert!(pixel(&out, 0, 0)[0] > 200, "Grid-Rahmen am Rand");
    assert!(pixel(&out, 300, 180)[0] > 40, "Grid-Linien im Bild");
}

#[test]
fn verify_auto_blend_edges() {
    let a = Region::new(0.0, 0.0, 1200.0, 1080.0);
    let b = Region::new(1000.0, 0.0, 1200.0, 1080.0);
    let mut stage = Stage::new("S", StageGeometry::new(2200.0, 1080.0));
    stage.regions = vec![a.clone(), b.clone()];
    let blends = auto_blends(&stage.regions);
    assert_eq!(blends[0].right, 200.0);
    assert_eq!(blends[1].left, 200.0);
}
