//! A window's live view draws, in its own render pass, the same pixels the renderer reads back. It uses Last
//! order's diner where Last order is on disk: set `LAST_ORDER` to its folder. Without it, the test says so and
//! checks nothing.

use spumcake_render::{Live, Look, Renderer, Request, View, scene, wgpu};
use std::path::PathBuf;

fn diner() -> Option<scene::Scene> {
    let Some(folder) = std::env::var_os("LAST_ORDER") else {
        eprintln!("LAST_ORDER is not set, so the live view is not checked");
        return None;
    };
    let resources = PathBuf::from(folder).join("prompts/resources");
    let mut names: Vec<String> = std::fs::read_dir(&resources)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with("-diner-scene-4195103.glb"))
        .collect();
    names.sort();
    let name = names.last().expect("no 4195103 diner scene file");
    Some(scene::read(&std::fs::read(resources.join(name)).unwrap()).unwrap())
}

/// Draws the live view into a fresh target and reads the target back as RGB.
fn paint(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    live: &mut Live,
    scene: &scene::Scene,
    request: &Request,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Vec<u8> {
    let (w, h) = (request.width, request.height);
    let target = |samples, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let resolved = target(1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let multisampled = (samples > 1).then(|| target(samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
    let resolved_view = resolved.create_view(&Default::default());
    let multisampled_view = multisampled.as_ref().map(|t| t.create_view(&Default::default()));

    let mut encoder = device.create_command_encoder(&Default::default());
    live.frame(device, queue, &mut encoder, scene, request).unwrap();
    {
        let (view, resolve_target) = match &multisampled_view {
            Some(m) => (m, Some(&resolved_view)),
            None => (&resolved_view, None),
        };
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::RED), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let mut pass = pass.forget_lifetime();
        live.paint(&mut pass);
    }
    let row = (w * 4).next_multiple_of(256);
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &resolved, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    read.map_async(wgpu::MapMode::Read, .., |_| ());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let view = read.get_mapped_range(..).unwrap();
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let k = (y * row + x * 4) as usize;
            rgb.extend_from_slice(&view[k..k + 3]);
        }
    }
    rgb
}

#[test]
fn the_live_view_draws_what_the_renderer_reads_back() {
    let Some(scene) = diner() else { return };
    let renderer = Renderer::software().unwrap();
    let (device, queue, _) = spumcake_render::device_on(true).unwrap();
    for (camera, look) in [("location", Look::Grey), ("concept-shot", Look::Coded)] {
        let request = Request { camera: View::Named(camera), look, width: 1664, height: 928, time: 0.0 };
        let frame = renderer.render(&scene, &request).unwrap();
        for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb] {
            for samples in [1, 4] {
                let mut live = Live::new(&device, &queue, format, samples);
                let rgb = paint(&device, &queue, &mut live, &scene, &request, format, samples);
                let differ = rgb.iter().zip(&frame.colour).filter(|(a, b)| a != b).count();
                assert_eq!(differ, 0, "{camera}, {format:?}, {samples} samples: {differ} channels differ");

                for at in [[0, 0], [832, 464], [100, 800], [1663, 927]] {
                    let expected = frame.objects[(at[1] * 1664 + at[0]) as usize];
                    let got = live.object_at(&device, &queue, at);
                    assert_eq!(got, (expected != spumcake_render::Frame::NONE).then_some(expected as usize), "{camera} at {at:?}");
                }
                assert!(live.objects(&device, &queue) == frame.objects, "{camera}: the object passes differ");
            }
        }
    }
}

#[test]
fn a_live_view_keeps_up_with_a_camera_and_a_size_that_change() {
    let Some(scene) = diner() else { return };
    let renderer = Renderer::software().unwrap();
    let (device, queue, _) = spumcake_render::device_on(true).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut live = Live::new(&device, &queue, format, 1);
    let free = View::Free { position: [6.0, 3.0, 1.6], target: [3.0, 5.0, 1.0], hfov: 60.0 };
    for (camera, width, height) in [(View::Named("location"), 1664, 928), (free, 800, 450), (View::Named("back-wall"), 1664, 928)] {
        let request = Request { camera, look: Look::Grey, width, height, time: 0.0 };
        let frame = renderer.render(&scene, &request).unwrap();
        let rgb = paint(&device, &queue, &mut live, &scene, &request, format, 1);
        let differ = rgb.iter().zip(&frame.colour).filter(|(a, b)| a != b).count();
        assert_eq!((rgb.len(), differ), (frame.colour.len(), 0), "{camera:?} at {width}x{height}");
        assert_eq!(live.size(), [width, height]);
    }
}

/// How long the live view takes per frame on this computer's own GPU, against the plan's 8 ms. It is a
/// measurement, not a check of pixels, so it runs only when asked: `cargo test --release -- --ignored --nocapture`.
#[test]
#[ignore]
fn how_long_a_live_frame_takes() {
    let Some(scene) = diner() else { return };
    let (device, queue, adapter) = spumcake_render::device_on(false).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut live = Live::new(&device, &queue, format, 4);
    let mut cpu = Vec::new();
    let mut whole = Vec::new();
    for i in 0..60 {
        // A camera orbiting the room, as a person dragging the view would move it.
        let a = i as f64 * 0.05;
        let camera = View::Free { position: [6.0 + 4.0 * a.cos(), 3.0 + 2.0 * a.sin(), 1.6], target: [6.0, 3.0, 1.2], hfov: 70.0 };
        let request = Request { camera, look: Look::Grey, width: 1664, height: 928, time: 0.0 };
        let start = std::time::Instant::now();
        let mut encoder = device.create_command_encoder(&Default::default());
        live.frame(&device, &queue, &mut encoder, &scene, &request).unwrap();
        let recorded = start.elapsed();
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        if i >= 10 {
            cpu.push(recorded.as_secs_f64() * 1000.0);
            whole.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let worst = |v: &Vec<f64>| v.iter().cloned().fold(0.0, f64::max);
    let (cpu_median, whole_median) = (median(&mut cpu), median(&mut whole));
    eprintln!(
        "on {adapter}: preparing {cpu_median:.2} ms (worst {:.2}), whole frame {whole_median:.2} ms (worst {:.2})",
        worst(&cpu),
        worst(&whole)
    );
}
