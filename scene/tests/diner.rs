//! The reader on Last order's own diner scene files, where Last order is on
//! disk: set `LAST_ORDER` to its folder. Without it, the test says so and
//! checks nothing.

use std::path::PathBuf;

fn diner() -> Option<Vec<(String, scene::Scene)>> {
    let Some(folder) = std::env::var_os("LAST_ORDER") else {
        eprintln!("LAST_ORDER is not set, so the diner's scene files are not checked");
        return None;
    };
    let resources = PathBuf::from(folder).join("prompts/resources");
    let mut scenes = Vec::new();
    for entry in std::fs::read_dir(&resources).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.contains("-diner-scene-") && name.ends_with(".glb") {
            scenes.push((name, scene::read(&std::fs::read(&path).unwrap()).unwrap()));
        }
    }
    // A builder exported again keeps only its newest export, by the time
    // its file's name starts with.
    scenes.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = std::collections::HashSet::new();
    scenes.retain(|(_, scene)| seen.insert(scene.builder.clone()));
    scenes.sort_by(|a, b| a.0.cmp(&b.0));
    Some(scenes)
}

#[test]
fn every_diner_scene_reads_as_its_builder_made_it() {
    let Some(scenes) = diner() else { return };
    assert!(scenes.len() >= 6, "found {} diner scene files", scenes.len());
    for (name, scene) in &scenes {
        let builder = &scene.builder;
        let (objects, cameras) = if builder.starts_with("git:4195103") {
            (106, 6)
        } else if builder.contains("2026-11-14_19-") {
            (111, 7)
        } else if builder.ends_with("2026-11-10_08-05-build-previs.py") {
            (105, 6)
        } else {
            (105, 7)
        };
        assert_eq!(scene.objects.len(), objects, "{name}");
        assert_eq!(scene.cameras.len(), cameras, "{name}");
        assert!(scene.objects.windows(2).all(|w| w[0].builder_id < w[1].builder_id), "{name}: not in the builder's order");
        assert!(scene.objects.iter().all(|o| o.positions.len() % 3 == 0 && !o.positions.is_empty()), "{name}");
        assert!(scene.objects.iter().all(|o| scene.looks.grey.contains_key(&o.kind)), "{name}: a kind with no grey");
        assert_eq!(scene.image_size, [1664, 928], "{name}");
        assert_eq!(scene.looks.background, Some(255), "{name}");
    }
}

#[test]
fn the_swing_matches_the_builders_easing_on_every_frame() {
    let Some(scenes) = diner() else { return };
    let swinging: Vec<_> = scenes.iter().filter(|(_, s)| s.swing.is_some()).collect();
    assert_eq!(swinging.len(), 2);
    for (name, scene) in swinging {
        let swing = scene.swing.as_ref().unwrap();
        assert_eq!(swing.times.len(), 192, "{name}");
        for frame in 0..192 {
            let t = frame as f64 / 24.0;
            let u = ((t - 3.0) / 0.8).clamp(0.0, 1.0);
            let builder = (75.0 * (1.0 - (1.0 - u).powi(3)) * 10.0).round() / 10.0;
            assert!((swing.angle_at(t) - builder).abs() < 1e-3, "{name}: frame {frame}");
        }
        let hinged: Vec<_> = scene.objects.iter().filter(|o| o.under_hinge.is_some()).map(|o| o.name.as_str()).collect();
        assert_eq!(hinged, ["front door", "front door glass", "front door push bar"], "{name}");
    }
}

#[test]
fn renders_are_linked_only_from_the_scene_that_made_them() {
    let Some(scenes) = diner() else { return };
    let linked: Vec<(String, String)> = scenes
        .iter()
        .flat_map(|(_, s)| s.cameras.iter().filter_map(|c| c.reference_file.clone().map(|r| (c.name.clone(), r))))
        .collect();
    assert_eq!(linked.iter().filter(|(_, r)| r.contains("2026-11-10_05-21-previs-")).count(), 6);
    assert!(linked.iter().any(|(c, r)| c == "door-close" && r.contains("2026-11-12_21-57")));
    assert!(linked.iter().any(|(c, r)| c == "far-end-wall" && r.contains("2026-11-11_17-20")));
}

#[test]
fn the_newest_exports_say_which_groups_are_named_and_each_renders_look() {
    let Some(scenes) = diner() else { return };
    let newest = scenes.iter().map(|(name, _)| &name[..16]).max().unwrap().to_string();
    if !scenes.iter().all(|(name, _)| name.starts_with(&newest)) {
        eprintln!("the diner's newest exports are not all from one time, so named and reference_look are not checked");
        return;
    }
    for (name, scene) in &scenes {
        let unnamed: Vec<&str> = scene.objects.iter().filter(|o| !o.named).map(|o| o.name.as_str()).collect();
        assert_eq!(unnamed, ["floor", "ceiling", "back wall", "far end wall", "near end wall"], "{name}");
        for camera in &scene.cameras {
            match (&camera.reference_file, camera.reference_look.as_deref()) {
                (Some(file), Some(look)) => {
                    let expected = if file.ends_with("-simple.mp4") { "simple" } else { "grey" };
                    assert_eq!(look, expected, "{name}: {}", camera.name);
                }
                (Some(file), None) => panic!("{name}: {} names {file} with no look", camera.name),
                (None, _) => {}
            }
        }
    }
}
