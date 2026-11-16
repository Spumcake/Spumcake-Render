//! A frame's work on the CPU: every face the look keeps, seen from the camera, clipped, projected, given its
//! flat colour, and filled to runs of pixels by Pillow's rule. The arithmetic follows the pre-vis builders'
//! `render` function step by step, in f64, so the GPU only has to settle which face is nearest at each pixel.

use crate::fill::{Span, fill};
use crate::{Error, Look, Request, View};
use scene::Scene;

/// How wide each column of pixels is when runs are sorted into buckets for the GPU.
pub const BUCKET: u32 = 32;

/// One face as the GPU needs it: its plane in camera space, its colour, and its object.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Face {
    pub normal: [f32; 3],
    pub plane: f32,
    pub colour: u32,
    pub object: u32,
    pub pad: [u32; 2],
}

/// A run of one face's pixels on one row.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Run {
    pub x0: i32,
    pub x1: i32,
    pub face: u32,
}

/// Everything the GPU needs for one frame.
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    pub focal: f64,
    pub background: u32,
    pub edge: u32,
    pub faces: Vec<Face>,
    pub runs: Vec<Run>,
    /// For each bucket, row by row and then column by column, where its runs start in `items`, plus one end.
    pub bucket_start: Vec<u32>,
    /// Indices into `runs`, each bucket's in drawing order.
    pub items: Vec<u32>,
}

pub fn grey(v: u8) -> u32 {
    rgb([v, v, v])
}

pub fn rgb(c: [u8; 3]) -> u32 {
    c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
}

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V) -> V {
    let mut l = dot(a, a).sqrt();
    if l == 0.0 {
        l = 1.0;
    }
    [a[0] / l, a[1] / l, a[2] / l]
}

/// The near plane, in metres along the camera's forward axis, where the builders clip.
const NEAR: f64 = 0.05;

pub fn prepare(scene: &Scene, request: &Request) -> Result<Prepared, Error> {
    let (width, height) = (request.width, request.height);
    if width == 0 || height == 0 {
        return Err(Error::Size(width, height));
    }
    let (position, target, hfov) = match request.camera {
        View::Named(name) => {
            let c = scene.camera(name).ok_or_else(|| Error::Camera(name.to_string()))?;
            (c.position, c.target, c.hfov)
        }
        View::Free { position, target, hfov } => (position, target, hfov),
    };
    let looks = &scene.looks;
    let simple = match request.look {
        Look::Simple => Some(looks.simple.as_ref().ok_or(Error::NoSimpleLook)?),
        _ => None,
    };
    let background = grey(looks.background.unwrap_or(255));
    let edge = match request.look {
        Look::Grey => grey(looks.edges.grey),
        Look::Coded => grey(looks.edges.coded),
        Look::Simple => grey(looks.edges.simple.unwrap_or(simple.unwrap().edge)),
    };

    let fwd = norm(sub(target, position));
    let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
    let up = cross(right, fwd);
    let (iw, ih) = (width as f64, height as f64);
    let focal = (iw / 2.0) / (hfov / 2.0).to_radians().tan();
    let view = Viewpoint { position, right, up, fwd, iw, ih, focal, width: width as i32, height: height as i32 };

    // The faces are seen and filled on several threads, each taking a run of objects, and joined in drawing
    // order, so the result is the same as on one thread.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    let triangles: usize = scene.objects.iter().map(|o| o.positions.len() / 3).sum();
    let mut ranges = Vec::new();
    let (mut first, mut count) = (0, 0);
    for (i, object) in scene.objects.iter().enumerate() {
        count += object.positions.len() / 3;
        if count * threads >= triangles * (ranges.len() + 1) {
            ranges.push(first..i + 1);
            first = i + 1;
        }
    }
    if first < scene.objects.len() {
        ranges.push(first..scene.objects.len());
    }
    let parts: Vec<Result<(Vec<Face>, Vec<Vec<Run>>), Error>> = std::thread::scope(|s| {
        let handles: Vec<_> = ranges
            .into_iter()
            .map(|range| s.spawn(|| faces_of(scene, request, simple, &view, range)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut faces = Vec::new();
    let mut runs_by_row: Vec<Vec<Run>> = vec![Vec::new(); height as usize];
    for part in parts {
        let (part_faces, part_rows) = part?;
        let offset = faces.len() as u32;
        faces.extend(part_faces);
        for (row, runs) in runs_by_row.iter_mut().zip(part_rows) {
            row.extend(runs.into_iter().map(|r| Run { face: r.face + offset, ..r }));
        }
    }

    let buckets_x = width.div_ceil(BUCKET);
    let mut runs = Vec::new();
    let mut counts = vec![0u32; (buckets_x * height) as usize + 1];
    for (y, row) in runs_by_row.iter().enumerate() {
        for r in row {
            for b in (r.x0 as u32 / BUCKET)..=(r.x1 as u32 / BUCKET) {
                counts[y * buckets_x as usize + b as usize] += 1;
            }
        }
    }
    let mut bucket_start = Vec::with_capacity(counts.len());
    let mut total = 0;
    for c in &counts {
        bucket_start.push(total);
        total += c;
    }
    let mut cursor = bucket_start.clone();
    let mut items = vec![0u32; total as usize];
    for (y, row) in runs_by_row.into_iter().enumerate() {
        for r in row {
            let index = runs.len() as u32;
            for b in (r.x0 as u32 / BUCKET)..=(r.x1 as u32 / BUCKET) {
                let k = y * buckets_x as usize + b as usize;
                items[cursor[k] as usize] = index;
                cursor[k] += 1;
            }
            runs.push(r);
        }
    }

    Ok(Prepared { width, height, focal, background, edge, faces, runs, bucket_start, items })
}

/// Where a frame is seen from, as the builders compute it.
struct Viewpoint {
    position: V,
    right: V,
    up: V,
    fwd: V,
    iw: f64,
    ih: f64,
    focal: f64,
    width: i32,
    height: i32,
}

/// The faces of the objects in `range`, in drawing order, and their runs of pixels row by row, with faces
/// numbered from 0 within the range.
fn faces_of(
    scene: &Scene,
    request: &Request,
    simple: Option<&scene::Simple>,
    view: &Viewpoint,
    range: std::ops::Range<usize>,
) -> Result<(Vec<Face>, Vec<Vec<Run>>), Error> {
    let looks = &scene.looks;
    let light = looks.shading.light;
    let (iw, ih, focal) = (view.iw, view.ih, view.focal);
    let to_cam = |p: V| {
        let v = sub(p, view.position);
        [dot(v, view.right), dot(v, view.up), dot(v, view.fwd)]
    };
    let mut faces = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut runs_by_row: Vec<Vec<Run>> = vec![Vec::new(); view.height as usize];
    for index in range {
        let object = &scene.objects[index];
        let base: [u8; 3] = match request.look {
            Look::Grey => {
                let g = *looks.grey.get(&object.kind).ok_or_else(|| Error::Kind(object.kind.clone()))?;
                [g, g, g]
            }
            Look::Coded => *looks.coded.get(&object.kind).ok_or_else(|| Error::Kind(object.kind.clone()))?,
            Look::Simple => {
                let s = simple.unwrap();
                if !s.kept.iter().any(|k| *k == object.name) {
                    continue;
                }
                let v = if object.kind == "night" { s.night } else { s.face };
                [v, v, v]
            }
        };
        for poly in scene.triangles_at(object, request.time) {
            let colour = if simple.is_some() {
                base
            } else {
                let n = norm(cross(sub(poly[1], poly[0]), sub(poly[2], poly[0])));
                let shade = looks.shading.ambient + looks.shading.diffuse * dot(n, light).abs();
                [(base[0] as f64 * shade) as u8, (base[1] as f64 * shade) as u8, (base[2] as f64 * shade) as u8]
            };
            // Clip against the near plane, as the builders do, before projecting.
            let cam = poly.map(to_cam);
            let mut clipped: Vec<V> = Vec::with_capacity(4);
            for i in 0..cam.len() {
                let (a, b) = (cam[i], cam[(i + 1) % cam.len()]);
                if a[2] > NEAR {
                    clipped.push(a);
                }
                if (a[2] > NEAR) != (b[2] > NEAR) {
                    let t = (NEAR - a[2]) / (b[2] - a[2]);
                    clipped.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]);
                }
            }
            if clipped.len() < 3 {
                continue;
            }
            let pts: Vec<[f64; 2]> =
                clipped.iter().map(|p| [iw / 2.0 + focal * p[0] / p[2], ih / 2.0 - focal * p[1] / p[2]]).collect();
            let (a, b, c) = (clipped[0], clipped[1], clipped[2]);
            let nn = cross(sub(b, a), sub(c, a));
            let dd = dot(nn, a);
            if dd.abs() < 1e-12 {
                continue;
            }
            // The builders skip a face whose box of pixels is empty.
            let min_x = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            let max_x = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
            let min_y = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
            let max_y = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
            let (x0, y0) = (0.max(min_x as i64), 0.max(min_y as i64));
            let (x1, y1) = ((view.width as i64).min(max_x as i64 + 2), (view.height as i64).min(max_y as i64 + 2));
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            spans.clear();
            fill(&pts, view.width, view.height, &mut spans);
            if spans.is_empty() {
                continue;
            }
            let face = faces.len() as u32;
            faces.push(Face {
                normal: [nn[0] as f32, nn[1] as f32, nn[2] as f32],
                plane: dd as f32,
                colour: rgb(colour),
                object: index as u32,
                pad: [0; 2],
            });
            for s in &spans {
                runs_by_row[s.y as usize].push(Run { x0: s.x0, x1: s.x1, face });
            }
        }
    }
    Ok((faces, runs_by_row))
}
