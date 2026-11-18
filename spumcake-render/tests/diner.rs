//! The scene contract's check of the renderer, on Last order's own diner, where Last order is on disk: set
//! `LAST_ORDER` to its folder. Without it, the test says so and checks nothing. It renders on the system's
//! software renderer, Mesa's lavapipe, so the result does not depend on the GPU.
//!
//! - Each published grey render, from the scene file of the builder that made it: no more than 0.5% of pixels
//!   differ, and no more than 0.02% lie more than 2 pixels from an edge in either picture.
//! - Each group of objects the builder's record lists in frame appears in the object pass, with its frame box
//!   within 2 pixels.
//! - Both door swings, the grey one and the simple one, encoded the builder's way, stay within a mean
//!   difference of 0.05 grey levels per frame of the published MP4s. That needs ffmpeg, as the builders did.

use spumcake_render::{Frame, Look, Renderer, Request, View, scene};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn last_order() -> Option<PathBuf> {
    match std::env::var_os("LAST_ORDER") {
        Some(folder) => Some(PathBuf::from(folder)),
        None => {
            eprintln!("LAST_ORDER is not set, so the renderer is not checked against Last order's diner");
            None
        }
    }
}

/// The newest scene file for each builder, by the builder's part of its name.
fn scenes(last_order: &Path) -> BTreeMap<String, (String, scene::Scene)> {
    let resources = last_order.join("prompts/resources");
    let mut names: Vec<String> = std::fs::read_dir(&resources)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("-diner-scene-") && n.ends_with(".glb"))
        .collect();
    names.sort();
    let mut out = BTreeMap::new();
    for name in names {
        let builder = name.split("-diner-scene-").nth(1).unwrap().trim_end_matches(".glb").to_string();
        let scene = scene::read(&std::fs::read(resources.join(&name)).unwrap()).unwrap();
        out.insert(builder, (name, scene));
    }
    out
}

/// A PNG as grey levels, from its first channel; the published renders are grey.
fn grey_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let channels = info.color_type.samples();
    let grey = buf[..info.buffer_size()].chunks(channels).map(|px| px[0]).collect();
    (info.width, info.height, grey)
}

/// Whether each pixel lies within `reach` pixels of an edge, the edge grey, in either picture.
fn near_edges(a: &[u8], b: &[u8], width: usize, edge: u8, reach: usize) -> Vec<bool> {
    let height = a.len() / width;
    let mut near = vec![false; a.len()];
    for y in 0..height {
        for x in 0..width {
            let k = y * width + x;
            if a[k] != edge && b[k] != edge {
                continue;
            }
            for yy in y.saturating_sub(reach)..=(y + reach).min(height - 1) {
                for xx in x.saturating_sub(reach)..=(x + reach).min(width - 1) {
                    near[yy * width + xx] = true;
                }
            }
        }
    }
    near
}

/// The stills' bounds: the share of pixels that differ, and the share that differ away from any edge.
fn differences(published: &[u8], ours: &[u8], width: usize, edge: u8) -> (f64, f64) {
    let near = near_edges(published, ours, width, edge, 2);
    let n = published.len() as f64;
    let differ = published.iter().zip(ours).filter(|(a, b)| a != b).count() as f64;
    let away = published.iter().zip(ours).zip(&near).filter(|((a, b), near)| a != b && !**near).count() as f64;
    (differ / n, away / n)
}

/// The group a builder's record names an object by: the rule of the builders that write records.
fn group(name: &str) -> String {
    let first = name.split(' ').next().unwrap();
    for whole in ["front door", "kitchen door", "back-bar"] {
        if name.starts_with(whole) {
            return whole.to_string();
        }
    }
    let grouped = ["stool", "lamp", "booth", "window", "napkin", "sugar", "stacked", "coffee", "counter", "register", "shelf", "canister"];
    if grouped.contains(&first) { first.to_string() } else { name.to_string() }
}

#[test]
fn every_published_grey_render_is_drawn_again_within_the_contract() {
    let Some(last_order) = last_order() else { return };
    let scenes = scenes(&last_order);
    let renderer = Renderer::software().unwrap();
    let mut checked = 0;
    for (builder, (name, scene)) in &scenes {
        for camera in &scene.cameras {
            let Some(reference) = camera.reference_file.as_ref().filter(|r| r.ends_with(".png")) else { continue };
            let (w, h, published) = grey_png(&last_order.join(reference));
            let request = Request { camera: View::Named(&camera.name), look: Look::Grey, width: w, height: h, time: 0.0 };
            let frame = renderer.render(scene, &request).unwrap();
            let ours: Vec<u8> = frame.colour.chunks(3).map(|px| px[0]).collect();
            let (differ, away) = differences(&published, &ours, w as usize, scene.looks.edges.grey);
            eprintln!("{builder} {}: {:.3}% differ, {:.4}% away from edges", camera.name, differ * 100.0, away * 100.0);
            assert!(differ <= 0.005, "{name} {}: {:.3}% of pixels differ", camera.name, differ * 100.0);
            assert!(away <= 0.0002, "{name} {}: {:.4}% differ away from edges", camera.name, away * 100.0);
            checked += 1;
        }
    }
    assert_eq!(checked, 8, "the contract names 8 published grey renders");
}

#[test]
fn every_group_the_builders_records_list_is_in_the_object_pass() {
    let Some(last_order) = last_order() else { return };
    let scenes = scenes(&last_order);
    let renderer = Renderer::software().unwrap();
    let mut records = 0;
    // The agent's files, `agent/`, once `working/` (spumcake-benchmarks 5ef168d, 2026-11-17).
    for entry in std::fs::read_dir(last_order.join("agent")).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with("-grey.record.json") {
            continue;
        }
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let file = record["file"].as_str().unwrap();
        // The records of the renders the published ones are: the 4195103 builder's views, and the door-close
        // builder's. The later "v4" records are of renders made again after the cabinet was removed.
        let builder = if file.contains("/v3/views/") {
            "4195103"
        } else if file.contains("/door-close/") {
            "2026-11-10-08-05-door-close"
        } else {
            continue;
        };
        let (_, scene) = &scenes[builder];
        let camera = record["camera"].as_str().unwrap();
        let (w, h) = (1664usize, 928usize);
        let request = Request { camera: View::Named(camera), look: Look::Grey, width: w as u32, height: h as u32, time: 0.0 };
        let frame = renderer.render(scene, &request).unwrap();
        let mut boxes: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for (k, &o) in frame.objects.iter().enumerate() {
            if o == Frame::NONE {
                continue;
            }
            let (x, y) = (k % w, k / w);
            let b = boxes.entry(group(&scene.objects[o as usize].name)).or_insert([x, y, x, y]);
            *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        for content in record["contents"].as_array().unwrap() {
            let name = content["name"].as_str().unwrap();
            let b = boxes.get(name).unwrap_or_else(|| panic!("{camera}: no {name} in the object pass"));
            let reference: Vec<f64> = content["frame_box"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let expected = [reference[0] * w as f64, reference[1] * h as f64, reference[2] * w as f64, reference[3] * h as f64];
            if builder == "4195103" && camera == "window-wall" && name == "booth" {
                // This record's booth box stops at row 747, but the published render, which the 4195103 builder
                // makes again to the pixel, shows the booth's seats down to row 832, and so do we. The record
                // was written later than the render and does not match it; Last order's records are not
                // changed for the port. So this one group's box is checked against the published render: its
                // bottom row is where the render's booth ends, and every other side is the record's.
                assert_eq!(b[3], 832, "{camera}: the booth's bottom row");
                for i in 0..3 {
                    assert!((b[i] as f64 - expected[i]).abs() <= 2.0, "{camera}: {name}'s box {b:?} against {expected:?}");
                }
                continue;
            }
            for i in 0..4 {
                assert!((b[i] as f64 - expected[i]).abs() <= 2.0, "{camera}: {name}'s box {b:?} against {expected:?}");
            }
        }
        records += 1;
    }
    assert_eq!(records, 7, "the records of the 4195103 views and the door-close renders");
}

#[test]
fn the_door_swings_as_the_published_videos_show() {
    let Some(last_order) = last_order() else { return };
    let scenes = scenes(&last_order);
    let renderer = Renderer::software().unwrap();
    // The two published swings: the 19-13 builder's in grey, and the 19-56 builder's in the simple look, the
    // motion reference.
    for (builder, look, video) in [("2026-11-14-19-13", Look::Grey, "-door-swing.mp4"), ("2026-11-14-19-56", Look::Simple, "-door-swing-simple.mp4")] {
        let (_, scene) = &scenes[builder];
        let camera = scene.cameras.iter().find(|c| c.reference_file.as_deref().is_some_and(|r| r.ends_with(video))).unwrap();
        let published = last_order.join(camera.reference_file.as_ref().unwrap());
        let frames = scene.swing.as_ref().unwrap().times.len();
        let means = swing_differences(&renderer, scene, &camera.name, look, &published, frames);
        let worst = means.iter().cloned().fold(0.0, f64::max);
        eprintln!("{builder} {look:?}: {frames} frames, at most {worst:.4} grey levels from the published video");
        for (i, mean) in means.iter().enumerate() {
            assert!(*mean <= 0.05, "{builder} frame {i}: a mean difference of {mean:.4} grey levels from the published video");
        }
    }
}

/// Renders the swing at 24 frames a second, encodes it as the builders encoded theirs, and gives each frame's
/// mean difference from the published video, in grey levels.
fn swing_differences(renderer: &Renderer, scene: &scene::Scene, camera: &str, look: Look, published: &Path, frames: usize) -> Vec<f64> {
    let (w, h) = (1664u32, 928u32);
    // The frames go straight into ffmpeg, encoded as the builder encoded its own, to a file on disk.
    let ours = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("door-swing-{look:?}.mp4"));
    let mut encoder = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{w}x{h}"), "-framerate", "24", "-i", "-"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16"])
        .arg(&ours)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("the door swing's check needs ffmpeg, as the builder did");
    {
        use std::io::Write;
        let stdin = encoder.stdin.as_mut().unwrap();
        for i in 0..frames {
            let request = Request { camera: View::Named(camera), look, width: w, height: h, time: i as f64 / 24.0 };
            stdin.write_all(&renderer.render(scene, &request).unwrap().colour).unwrap();
        }
    }
    drop(encoder.stdin.take());
    assert!(encoder.wait().unwrap().success());
    let decode = |path: &Path| -> Vec<u8> {
        let output = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    };
    let (a, b) = (decode(published), decode(&ours));
    let size = (w * h) as usize;
    assert_eq!((a.len() / size, b.len() / size), (frames, frames));
    (0..frames)
        .map(|i| {
            let (fa, fb) = (&a[i * size..(i + 1) * size], &b[i * size..(i + 1) * size]);
            fa.iter().zip(fb).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / size as f64
        })
        .collect()
}

/// A PNG's samples, as written: 8-bit bytes, or 16-bit values.
fn png_samples(path: &Path) -> (png::ColorType, png::BitDepth, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.color_type, info.bit_depth, buf)
}

#[test]
fn the_command_line_writes_what_the_library_draws() {
    let Some(last_order) = last_order() else { return };
    let scenes = scenes(&last_order);
    let (name, scene) = &scenes["4195103"];
    let file = last_order.join("prompts/resources").join(name);
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("command-line");
    std::fs::create_dir_all(&out).unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_spumcake-render"))
        .env("SPUMCAKE_RENDER_ADAPTER", "software")
        .arg("render")
        .arg(&file)
        .args(["--camera", "location", "--look", "coded", "--size", "1664x928"])
        .arg("--out")
        .arg(out.join("location.png"))
        .arg("--depth")
        .arg(out.join("location-depth.png"))
        .arg("--objects")
        .arg(out.join("location-objects.png"))
        .status()
        .unwrap();
    assert!(status.success());

    let request = Request { camera: View::Named("location"), look: Look::Coded, width: 1664, height: 928, time: 0.0 };
    let frame = Renderer::software().unwrap().render(scene, &request).unwrap();

    let (colour, depth, rgb) = png_samples(&out.join("location.png"));
    assert_eq!((colour, depth), (png::ColorType::Rgb, png::BitDepth::Eight));
    assert!(rgb == frame.colour, "the picture differs from the library's");

    let (colour, depth, bytes) = png_samples(&out.join("location-depth.png"));
    assert_eq!((colour, depth), (png::ColorType::Grayscale, png::BitDepth::Sixteen));
    for (k, z) in frame.depth.iter().enumerate() {
        let mm = u16::from_be_bytes([bytes[2 * k], bytes[2 * k + 1]]);
        if z.is_finite() {
            assert!((mm as f64 - *z as f64 * 1000.0).abs() <= 0.5, "pixel {k}: {mm} mm for {z} m");
        } else {
            assert_eq!(mm, 0, "pixel {k}: nothing drawn");
        }
    }

    let (_, _, bytes) = png_samples(&out.join("location-objects.png"));
    let names: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("location-objects.png.json")).unwrap()).unwrap();
    assert!(names["0"].is_null());
    for (k, &o) in frame.objects.iter().enumerate() {
        let number = u16::from_be_bytes([bytes[2 * k], bytes[2 * k + 1]]);
        if o == Frame::NONE {
            assert_eq!(number, 0);
        } else {
            assert_eq!(number as u32, o + 1);
            assert_eq!(names[number.to_string()].as_str(), Some(scene.objects[o as usize].name.as_str()));
        }
    }

    let version = std::process::Command::new(env!("CARGO_BIN_EXE_spumcake-render")).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8(version.stdout).unwrap(), format!("spumcake-render {}\n", env!("CARGO_PKG_VERSION")));
}
