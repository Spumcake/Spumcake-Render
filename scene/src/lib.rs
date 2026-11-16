//! A place's scene file: a glTF 2.0 binary that Spumcake-Skills' export makes
//! from a pre-vis builder, read as the scene contract
//! (spumcake-team `.project/documents/contracts/scene.md`) describes it.
//!
//! The reader keeps the builder's own frame: z up, in metres, so every number
//! it gives is the number the builder, its floor plan, and its canon use. It
//! renders nothing, and knows no window, no host, and no workspace.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

/// One scene file, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// The builder it was exported from, as the export names it: a path in
    /// the project, or `git:<commit>:<path>`.
    pub builder: String,
    pub builder_sha256: String,
    /// The size the builder renders at, such as 1664 × 928.
    pub image_size: [u32; 2],
    /// What the export says about the door, such as why it does not move.
    pub door_note: Option<String>,
    pub looks: Looks,
    pub objects: Vec<Object>,
    pub cameras: Vec<Camera>,
    /// The door's swing, for a builder that has one.
    pub swing: Option<Swing>,
}

/// The builder's looks, as data, so a render can match its published pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Looks {
    /// The grey of each kind in the grey look.
    pub grey: BTreeMap<String, u8>,
    /// The colour of each kind in the colour-coded look.
    pub coded: BTreeMap<String, [u8; 3]>,
    pub shading: Shading,
    pub edges: Edges,
    /// The grey behind everything, which the contract says is 255. None while
    /// a scene file does not carry it as data.
    pub background: Option<u8>,
    /// The simple look, for the one builder that has it.
    pub simple: Option<Simple>,
}

/// Each face is shaded `ambient + diffuse · |n · light|`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shading {
    pub ambient: f64,
    pub diffuse: f64,
    /// Already normalised, in the builder's frame.
    pub light: [f64; 3],
}

/// The grey of the one-pixel edge drawn where the object under the pixel
/// changes, in each look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub grey: u8,
    pub coded: u8,
    pub simple: Option<u8>,
}

/// The simple look: white faces and thin grey edges, keeping only the
/// objects the builder's own rule keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct Simple {
    /// The names of the objects it keeps.
    pub kept: Vec<String>,
    pub face: u8,
    pub night: u8,
    pub edge: u8,
}

/// One of the builder's objects, such as "stool 3 seat".
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// The name the builder gives it.
    pub name: String,
    pub kind: String,
    /// Its label group's key, such as "stacked", which canon's contents use.
    pub label: String,
    /// The words the builder's labelled renders show for its group, such as
    /// "plates and mugs".
    pub label_text: String,
    /// Its place in the builder's list, which is the order it draws in: the
    /// builder's depth test keeps the first-drawn face on ties.
    pub builder_id: u32,
    /// Its corners as the file stores them, three per triangle, in the file's
    /// order, in its own node's frame. `Scene::object_transform` places them
    /// in the builder's frame at a time.
    pub positions: Vec<[f32; 3]>,
    /// One normal per corner, as the file stores it, in the same frame.
    pub normals: Vec<[f32; 3]>,
    /// Where its node stands in the builder's frame, at rest.
    pub rest: Transform,
    /// Where its node stands under the swing's hinge, for an object the
    /// hinge turns.
    pub under_hinge: Option<Transform>,
}

/// A turn, then a move, in the builder's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// A unit quaternion, x, y, z, w.
    pub rotation: [f64; 4],
    pub translation: [f64; 3],
}

impl Transform {
    pub const IDENTITY: Transform = Transform { rotation: [0.0, 0.0, 0.0, 1.0], translation: [0.0; 3] };

    /// A turn about the builder's z, counter-clockwise seen from above.
    pub fn about_z(degrees: f64, at: [f64; 3]) -> Transform {
        let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
        Transform { rotation: [0.0, 0.0, sin, cos], translation: at }
    }

    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let r = rotate(self.rotation, p);
        [r[0] + self.translation[0], r[1] + self.translation[1], r[2] + self.translation[2]]
    }

    /// This transform after `local`: `local` places a point in this one's
    /// frame, and this one places that in its parent's.
    pub fn then(&self, local: Transform) -> Transform {
        Transform { rotation: multiply(self.rotation, local.rotation), translation: self.apply(local.translation) }
    }

    /// The same transform as a 4 × 4 matrix, row by row, for a column vector.
    pub fn matrix(&self) -> [[f64; 4]; 4] {
        let [x, y, z, w] = self.rotation;
        let t = self.translation;
        [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), t[0]],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), t[1]],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), t[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}

/// One of the builder's cameras, with the builder's own numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// The builder's name for it, such as "far-end-wall".
    pub name: String,
    pub position: [f64; 3],
    pub target: [f64; 3],
    /// The horizontal field of view, in degrees.
    pub hfov: f64,
    pub image_size: [u32; 2],
    /// The canon artifact its render was published as, if any.
    pub canon: Option<String>,
    /// The published render this scene's own builder made from it, as a path
    /// in the project, if any.
    pub reference_file: Option<String>,
}

/// The door's swing: the hinge's turn about the room's z, held at each key
/// until the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Swing {
    pub name: String,
    /// Where the hinge stands, in the builder's frame.
    pub hinge: [f64; 3],
    /// Each key's time, in seconds.
    pub times: Vec<f64>,
    /// Each key's angle, in degrees, counter-clockwise seen from above.
    pub angles: Vec<f64>,
}

impl Swing {
    /// The angle shown at a time: the last key at or before it, or the first
    /// key before the swing starts. Key times are stored as 32-bit floats, so
    /// frame 73 at 24 fps is stored a little after 73/24 s; a key within a
    /// hundred-thousandth of a second counts as reached, far less than a
    /// frame.
    pub fn angle_at(&self, time: f64) -> f64 {
        let held = self.times.iter().rposition(|t| *t <= time + 1e-5).unwrap_or(0);
        self.angles[held]
    }

    /// The length of the swing, in seconds: its last key.
    pub fn length(&self) -> f64 {
        self.times.last().copied().unwrap_or(0.0)
    }
}

impl Scene {
    /// The camera the builder calls by this name.
    pub fn camera(&self, name: &str) -> Option<&Camera> {
        self.cameras.iter().find(|camera| camera.name == name)
    }

    /// Where an object's stored corners go in the builder's frame at a time
    /// in the swing. The swing holds each key until the next, so a time
    /// between keys gives the earlier key's angle. An object no hinge turns
    /// stays at rest at every time.
    pub fn object_transform(&self, object: &Object, time: f64) -> Transform {
        match (&self.swing, object.under_hinge) {
            (Some(swing), Some(local)) => Transform::about_z(swing.angle_at(time), swing.hinge).then(local),
            _ => object.rest,
        }
    }

    /// An object's triangles at a time, in the builder's frame.
    pub fn triangles_at(&self, object: &Object, time: f64) -> Vec<[[f64; 3]; 3]> {
        let place = self.object_transform(object, time);
        object
            .positions
            .chunks(3)
            .map(|t| [place.apply(t[0].map(f64::from)), place.apply(t[1].map(f64::from)), place.apply(t[2].map(f64::from))])
            .collect()
    }
}

/// Why a scene file could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Not a glTF binary, or one the glTF reader refuses.
    Glb(String),
    /// The file holds no scene.
    NoScene,
    /// The scene's root is not the one node that turns z up to y up.
    Root(String),
    /// The scene's own record, its `extras`, is missing something.
    Record(String),
    /// A node of the builder's objects is missing something.
    Object { node: String, what: String },
    Camera { node: String, what: String },
    Swing(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Glb(why) => write!(f, "not a readable glTF binary: {why}"),
            Error::NoScene => write!(f, "the file holds no scene"),
            Error::Root(why) => write!(f, "the scene's root {why}"),
            Error::Record(why) => write!(f, "the scene's record {why}"),
            Error::Object { node, what } => write!(f, "the object \"{node}\" {what}"),
            Error::Camera { node, what } => write!(f, "the camera \"{node}\" {what}"),
            Error::Swing(why) => write!(f, "the door's swing {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads a scene file's bytes.
pub fn read(bytes: &[u8]) -> Result<Scene, Error> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| Error::Glb(e.to_string()))?;
    let blob = gltf.blob.as_deref();
    let document = &gltf.document;
    let scene = document.default_scene().or_else(|| document.scenes().next()).ok_or(Error::NoScene)?;
    let record = extras(scene.extras()).ok_or_else(|| Error::Record("is missing".into()))?;

    let builder = text(&record, "builder").ok_or_else(|| Error::Record("names no builder".into()))?;
    let builder_sha256 = text(&record, "builder_sha256").ok_or_else(|| Error::Record("has no builder_sha256".into()))?;
    let image_size = size(record.get("image_size")).ok_or_else(|| Error::Record("has no image_size".into()))?;
    let door_note = text(&record, "door");
    let looks = looks(record.get("looks").ok_or_else(|| Error::Record("has no looks".into()))?)?;

    let mut roots = scene.nodes();
    let root = roots.next().ok_or_else(|| Error::Root("is missing".into()))?;
    if roots.next().is_some() {
        return Err(Error::Root("is not the scene's only node".into()));
    }
    check_root(&root)?;

    // The node the swing turns, if any, so the objects under it are marked.
    let swing_node = document
        .animations()
        .find(|a| a.name() == Some("door swing"))
        .and_then(|a| a.channels().next())
        .map(|c| c.target().node().index());

    let mut found = Found { objects: Vec::new(), cameras: Vec::new(), hinge: None };
    for child in root.children() {
        walk(&child, Transform::IDENTITY, None, swing_node, blob, &mut found)?;
    }
    found.objects.sort_by_key(|o| o.builder_id);

    let swing = match document.animations().find(|a| a.name() == Some("door swing")) {
        Some(animation) => Some(swing(&animation, blob, found.hinge)?),
        None => None,
    };
    Ok(Scene { builder, builder_sha256, image_size, door_note, looks, objects: found.objects, cameras: found.cameras, swing })
}

/// What the walk through the nodes collects.
struct Found {
    objects: Vec<Object>,
    cameras: Vec<Camera>,
    /// The swing's hinge, where it stands in the builder's frame.
    hinge: Option<[f64; 3]>,
}

/// Walks a node and its children. `parent` places the parent in the
/// builder's frame; `under_hinge`, inside the swing's hinge, places it in the
/// hinge's frame instead.
fn walk(
    node: &gltf::Node<'_>,
    parent: Transform,
    under_hinge: Option<Transform>,
    swing_node: Option<usize>,
    blob: Option<&[u8]>,
    found: &mut Found,
) -> Result<(), Error> {
    let name = node.name().unwrap_or("").to_string();
    let (translation, rotation, scale) = node.transform().decomposed();
    if scale.iter().any(|s| (*s - 1.0).abs() > 1e-6) {
        return Err(Error::Object { node: name, what: "is scaled, which no builder does".into() });
    }
    let local = Transform { rotation: rotation.map(f64::from), translation: translation.map(f64::from) };
    let rest = parent.then(local);
    let under_hinge = if swing_node == Some(node.index()) {
        // The swing replaces the hinge's turn and keeps where it stands, so
        // the hinge must stand straight under the room's root.
        if parent != Transform::IDENTITY || (rotation[3] - 1.0).abs() > 1e-6 {
            return Err(Error::Swing("turns a hinge that is not straight under the room".into()));
        }
        found.hinge = Some(rest.translation);
        Some(Transform::IDENTITY)
    } else {
        under_hinge.map(|h| h.then(local))
    };

    if node.camera().is_some() {
        found.cameras.push(camera(&name, node)?);
    }
    if let Some(mesh) = node.mesh() {
        found.objects.push(object(&name, node, &mesh, rest, under_hinge, blob)?);
    }
    for child in node.children() {
        walk(&child, rest, under_hinge, swing_node, blob, found)?;
    }
    Ok(())
}

fn object(
    name: &str,
    node: &gltf::Node<'_>,
    mesh: &gltf::Mesh<'_>,
    rest: Transform,
    under_hinge: Option<Transform>,
    blob: Option<&[u8]>,
) -> Result<Object, Error> {
    let fail = |what: &str| Error::Object { node: name.to_string(), what: what.to_string() };
    let record = extras(node.extras()).ok_or_else(|| fail("has no record"))?;
    let kind = text(&record, "kind").ok_or_else(|| fail("has no kind"))?;
    let label = text(&record, "label").ok_or_else(|| fail("has no label"))?;
    let label_text = text(&record, "label_text").unwrap_or_else(|| label.clone());
    let builder_id = record.get("builder_id").and_then(Value::as_u64).ok_or_else(|| fail("has no builder_id"))? as u32;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for primitive in mesh.primitives() {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            return Err(fail("is not made of triangles"));
        }
        let reader = primitive.reader(|b| bin(b, blob));
        let points: Vec<[f32; 3]> = reader.read_positions().ok_or_else(|| fail("has no positions"))?.collect();
        let corners: Vec<[f32; 3]> = reader.read_normals().ok_or_else(|| fail("has no normals"))?.collect();
        let order: Vec<usize> = match reader.read_indices() {
            Some(indices) => indices.into_u32().map(|i| i as usize).collect(),
            None => (0..points.len()).collect(),
        };
        if order.len() % 3 != 0 || order.iter().any(|i| *i >= points.len() || *i >= corners.len()) {
            return Err(fail("has a broken triangle list"));
        }
        for corner in order {
            positions.push(points[corner]);
            normals.push(corners[corner]);
        }
    }
    Ok(Object { name: name.to_string(), kind, label, label_text, builder_id, positions, normals, rest, under_hinge })
}

fn camera(name: &str, node: &gltf::Node<'_>) -> Result<Camera, Error> {
    let fail = |what: &str| Error::Camera { node: name.to_string(), what: what.to_string() };
    let record = extras(node.extras()).ok_or_else(|| fail("has no record"))?;
    let point = |key: &str| -> Result<[f64; 3], Error> {
        let values = record.get(key).and_then(Value::as_array).ok_or_else(|| fail(&format!("has no {key}")))?;
        match values.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>() {
            Some(v) if v.len() == 3 => Ok([v[0], v[1], v[2]]),
            _ => Err(fail(&format!("has a {key} that is not three numbers"))),
        }
    };
    Ok(Camera {
        name: name.to_string(),
        position: point("position")?,
        target: point("target")?,
        hfov: record.get("hfov").and_then(Value::as_f64).ok_or_else(|| fail("has no hfov"))?,
        image_size: size(record.get("image_size")).ok_or_else(|| fail("has no image size"))?,
        canon: text(&record, "canon"),
        reference_file: text(&record, "reference_file"),
    })
}

fn swing(
    animation: &gltf::Animation<'_>,
    blob: Option<&[u8]>,
    hinge: Option<[f64; 3]>,
) -> Result<Swing, Error> {
    let fail = |why: &str| Error::Swing(why.to_string());
    let mut channels = animation.channels();
    let channel = channels.next().ok_or_else(|| fail("has no channel"))?;
    if channels.next().is_some() {
        return Err(fail("turns more than the hinge"));
    }
    if channel.target().property() != gltf::animation::Property::Rotation {
        return Err(fail("is not a turn"));
    }
    if channel.sampler().interpolation() != gltf::animation::Interpolation::Step {
        return Err(fail("is not held from key to key"));
    }
    let reader = channel.reader(|b| bin(b, blob));
    let times: Vec<f64> = reader.read_inputs().ok_or_else(|| fail("has no times"))?.map(f64::from).collect();
    let turns = match reader.read_outputs().ok_or_else(|| fail("has no angles"))? {
        gltf::animation::util::ReadOutputs::Rotations(r) => r.into_f32().collect::<Vec<[f32; 4]>>(),
        _ => return Err(fail("is not a turn")),
    };
    if turns.len() != times.len() || times.is_empty() {
        return Err(fail("has a different number of times and angles"));
    }
    let mut angles = Vec::with_capacity(turns.len());
    for q in turns {
        let q = q.map(f64::from);
        if q[0].abs() > 1e-6 || q[1].abs() > 1e-6 {
            return Err(fail("turns about something other than the room's z"));
        }
        angles.push(2.0 * q[2].atan2(q[3]).to_degrees());
    }
    let hinge = hinge.ok_or_else(|| fail("turns a node that is not in the scene"))?;
    Ok(Swing { name: animation.name().unwrap_or("door swing").to_string(), hinge, times, angles })
}

/// The root must be the one node that turns the builder's z up to glTF's
/// y up: −90° about x, with nothing else.
fn check_root(root: &gltf::Node<'_>) -> Result<(), Error> {
    let record = extras(root.extras());
    if record.as_ref().and_then(|r| text(r, "source_up")).as_deref() != Some("z") {
        return Err(Error::Root("does not say its source is z up".into()));
    }
    let (translation, rotation, scale) = root.transform().decomposed();
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let turn_ok = (rotation[0] + half).abs() < 1e-6 && rotation[1].abs() < 1e-6 && rotation[2].abs() < 1e-6
        && (rotation[3] - half).abs() < 1e-6;
    let still = translation.iter().all(|t| t.abs() < 1e-6) && scale.iter().all(|s| (s - 1.0).abs() < 1e-6);
    if !turn_ok || !still {
        return Err(Error::Root("is not exactly a turn from z up to y up".into()));
    }
    Ok(())
}

fn looks(value: &Value) -> Result<Looks, Error> {
    let fail = |why: &str| Error::Record(format!("has looks {why}"));
    let grey = value
        .get("grey")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("with no grey"))?
        .iter()
        .map(|(kind, g)| byte(g).map(|g| (kind.clone(), g)).ok_or_else(|| fail(&format!("with a grey for {kind} that is not 0 to 255"))))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let coded = value
        .get("coded")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("with no coded colours"))?
        .iter()
        .map(|(kind, c)| colour(c).map(|c| (kind.clone(), c)).ok_or_else(|| fail(&format!("with a colour for {kind} that is not three bytes"))))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let shading = value.get("shading").ok_or_else(|| fail("with no shading"))?;
    let number = |key: &str| shading.get(key).and_then(Value::as_f64).ok_or_else(|| fail(&format!("with no shading {key}")));
    let light = shading
        .get("light")
        .and_then(Value::as_array)
        .and_then(|l| l.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>())
        .filter(|l| l.len() == 3)
        .ok_or_else(|| fail("with no shading light"))?;
    let shading = Shading { ambient: number("ambient")?, diffuse: number("diffuse")?, light: [light[0], light[1], light[2]] };
    let edges = value.get("edges").ok_or_else(|| fail("with no edges"))?;
    let edge = |key: &str| edges.get(key).and_then(byte);
    let edges = Edges {
        grey: edge("grey").ok_or_else(|| fail("with no grey edge"))?,
        coded: edge("coded").ok_or_else(|| fail("with no coded edge"))?,
        simple: edge("simple"),
    };
    // A builder with no simple look has `"simple": null`, or no entry at all.
    let simple = match value.get("simple") {
        None | Some(Value::Null) => None,
        Some(simple) => {
            let kept = simple
                .get("kept")
                .and_then(Value::as_array)
                .and_then(|k| k.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<Vec<String>>>())
                .ok_or_else(|| fail("with a simple look that keeps no names"))?;
            let grey = |key: &str| simple.get(key).and_then(byte).ok_or_else(|| fail(&format!("with a simple look that has no {key}")));
            Some(Simple { kept, face: grey("face")?, night: grey("night")?, edge: grey("edge")? })
        }
    };
    let background = value.get("background").and_then(byte);
    Ok(Looks { grey, coded, shading, edges, background, simple })
}

/// A buffer's bytes: only the file's own binary chunk, since a scene file
/// refers to nothing outside itself.
fn bin<'a>(buffer: gltf::Buffer<'_>, blob: Option<&'a [u8]>) -> Option<&'a [u8]> {
    match buffer.source() {
        gltf::buffer::Source::Bin => blob,
        gltf::buffer::Source::Uri(_) => None,
    }
}

fn extras(raw: &gltf::json::Extras) -> Option<Value> {
    raw.as_ref().and_then(|r| serde_json::from_str(r.get()).ok())
}

/// A text value, with JSON's null read as none.
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn byte(value: &Value) -> Option<u8> {
    value.as_u64().and_then(|v| u8::try_from(v).ok())
}

fn colour(value: &Value) -> Option<[u8; 3]> {
    let c = value.as_array()?.iter().map(byte).collect::<Option<Vec<u8>>>()?;
    (c.len() == 3).then(|| [c[0], c[1], c[2]])
}

fn size(value: Option<&Value>) -> Option<[u32; 2]> {
    let s = value?.as_array()?.iter().map(|v| v.as_u64().and_then(|v| u32::try_from(v).ok())).collect::<Option<Vec<u32>>>()?;
    (s.len() == 2).then(|| [s[0], s[1]])
}

fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let c = [y * v[2] - z * v[1], z * v[0] - x * v[2], x * v[1] - y * v[0]];
    let cc = [y * c[2] - z * c[1], z * c[0] - x * c[2], x * c[1] - y * c[0]];
    [v[0] + 2.0 * (w * c[0] + cc[0]), v[1] + 2.0 * (w * c[1] + cc[1]), v[2] + 2.0 * (w * c[2] + cc[2])]
}

fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}
