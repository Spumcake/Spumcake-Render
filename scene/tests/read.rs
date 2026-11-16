//! The reader on small scene files each test writes itself, so the crate
//! carries no benchmark's files.

use serde_json::{Value, json};

/// A glTF binary from its JSON and its binary chunk.
fn glb(document: &Value, bin: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(document).unwrap();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let mut bin = bin.to_vec();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + text.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

const HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// One triangle on the floor, a door leaf on a hinge that swings 0° then
/// 90°, and one camera, as the export lays a scene out.
fn small(object_extras: Value) -> Vec<u8> {
    let triangle = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let up = floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let leaf = floats(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 2.0]);
    let times = floats(&[0.0, 1.0]);
    let (s, c) = (45f32.to_radians().sin(), 45f32.to_radians().cos());
    let turns = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, c]);
    let mut bin = Vec::new();
    let mut views = Vec::new();
    for part in [&triangle, &up, &leaf, &times, &turns] {
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": part.len()}));
        bin.extend_from_slice(part);
    }
    let vec3 = |view: usize, min: [f32; 3], max: [f32; 3]| json!({"bufferView": view, "componentType": 5126, "count": 3, "type": "VEC3", "min": min, "max": max});
    let document = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0], "extras": {
            "builder": "prompts/resources/build-previs.py", "builder_sha256": "ab", "image_size": [1664, 928],
            "door": "swings",
            "looks": {"grey": {"floor": 140, "door": 60}, "coded": {"floor": [150, 140, 128], "door": [60, 60, 70]},
                      "shading": {"ambient": 0.72, "diffuse": 0.28, "light": [0.0, 0.0, 1.0]},
                      "edges": {"rule": "a 1-pixel edge where the object under the pixel changes", "grey": 55, "coded": 55},
                      "simple": null}}}],
        "nodes": [
            {"name": "z up to y up", "rotation": [-HALF, 0.0, 0.0, HALF], "children": [1, 2, 4], "extras": {"source_up": "z"}},
            {"name": "floor", "mesh": 0, "extras": object_extras},
            {"name": "front door hinge", "translation": [0.0, 0.4, 0.0], "children": [3]},
            {"name": "front door", "mesh": 1, "extras": {"kind": "door", "label": "front door", "label_text": "front door", "builder_id": 1}},
            {"name": "far-end-wall", "camera": 0, "extras": {"position": [7.0, 2.6, 1.6], "target": [0.0, 2.6, 1.3], "hfov": 62,
                "image_size": [1664, 928], "canon": "the diner: pre-vis far-end-wall", "reference_file": null}}
        ],
        "cameras": [{"type": "perspective", "perspective": {"aspectRatio": 1.79, "yfov": 0.65, "znear": 0.05}}],
        "meshes": [
            {"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]},
            {"primitives": [{"attributes": {"POSITION": 2, "NORMAL": 1}}]}
        ],
        "accessors": [
            vec3(0, [0.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            vec3(1, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]),
            vec3(2, [0.0, 0.0, 0.0], [0.0, 1.0, 2.0]),
            {"bufferView": 3, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]},
            {"bufferView": 4, "componentType": 5126, "count": 2, "type": "VEC4"}
        ],
        "animations": [{"name": "door swing", "channels": [{"sampler": 0, "target": {"node": 2, "path": "rotation"}}],
                        "samplers": [{"input": 3, "output": 4, "interpolation": "STEP"}]}],
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    glb(&document, &bin)
}

fn floor_record() -> Value {
    json!({"kind": "floor", "label": "floor", "label_text": "floor", "builder_id": 0})
}

#[test]
fn reads_objects_cameras_and_looks_in_the_builders_frame() {
    let scene = scene::read(&small(floor_record())).unwrap();
    assert_eq!(scene.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["floor", "front door"]);
    let floor = &scene.objects[0];
    assert_eq!(floor.kind, "floor");
    assert_eq!(floor.positions, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    assert!(floor.under_hinge.is_none());
    let camera = scene.camera("far-end-wall").unwrap();
    assert_eq!((camera.position, camera.target, camera.hfov), ([7.0, 2.6, 1.6], [0.0, 2.6, 1.3], 62.0));
    assert_eq!(camera.canon.as_deref(), Some("the diner: pre-vis far-end-wall"));
    assert_eq!(camera.reference_file, None);
    assert_eq!(scene.looks.grey["door"], 60);
    assert_eq!(scene.looks.coded["floor"], [150, 140, 128]);
    assert_eq!(scene.looks.simple, None);
    assert_eq!(scene.image_size, [1664, 928]);
}

#[test]
fn the_swing_holds_each_key_until_the_next() {
    let scene = scene::read(&small(floor_record())).unwrap();
    let swing = scene.swing.as_ref().unwrap();
    assert!((swing.hinge[1] - 0.4).abs() < 1e-6);
    assert_eq!(swing.angle_at(-1.0), 0.0);
    assert_eq!(swing.angle_at(0.99), 0.0);
    assert!((swing.angle_at(1.0) - 90.0).abs() < 1e-4);
    assert!((swing.angle_at(5.0) - 90.0).abs() < 1e-4);

    // At rest the leaf stands on its hinge; turned 90°, its far edge, one
    // metre along y from the hinge, swings to one metre along −x.
    let door = &scene.objects[1];
    let rest = scene.triangles_at(door, 0.0);
    assert!((rest[0][1][1] - 1.4).abs() < 1e-6 && rest[0][1][0].abs() < 1e-6);
    let open = scene.triangles_at(door, 1.0);
    assert!((open[0][1][0] + 1.0).abs() < 1e-5 && (open[0][1][1] - 0.4).abs() < 1e-5);
    // An object off the hinge stays at rest at every time.
    assert_eq!(scene.object_transform(&scene.objects[0], 1.0), scene.objects[0].rest);
}

#[test]
fn names_what_is_missing() {
    let error = scene::read(&small(json!({"label": "floor", "builder_id": 0}))).unwrap_err();
    assert_eq!(error.to_string(), "the object \"floor\" has no kind");
    assert!(matches!(scene::read(b"not a scene"), Err(scene::Error::Glb(_))));
}
