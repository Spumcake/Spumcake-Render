//! The GPU's part of a frame: two compute passes over the prepared faces and runs, then either reading the
//! result back ([`Renderer`]) or keeping it on the GPU for a window to draw ([`Live`]).

use crate::prepare::{BUCKET, Prepared, prepare};
use crate::{Error, Frame, Request};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    width: u32,
    height: u32,
    buckets_x: u32,
    background: u32,
    half_width: f32,
    half_height: f32,
    focal: f32,
    edge: u32,
}

impl Params {
    fn of(p: &Prepared) -> Params {
        Params {
            width: p.width,
            height: p.height,
            buckets_x: p.width.div_ceil(BUCKET),
            background: p.background,
            half_width: (p.width as f64 / 2.0) as f32,
            half_height: (p.height as f64 / 2.0) as f32,
            focal: p.focal as f32,
            edge: p.edge,
        }
    }
}

/// The compute pipelines, made once per device.
struct Pipelines {
    resolve: wgpu::ComputePipeline,
    edges: wgpu::ComputePipeline,
    present: wgpu::ComputePipeline,
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Pipelines {
        let pipeline = |label, source: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some(label),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Pipelines {
            resolve: pipeline("resolve", include_str!("render.wgsl")),
            edges: pipeline("edges", include_str!("edges.wgsl")),
            present: pipeline("present", include_str!("present.wgsl")),
        }
    }
}

/// A buffer that grows when a frame needs more room, and is otherwise reused.
struct Grow {
    label: &'static str,
    usage: wgpu::BufferUsages,
    buffer: wgpu::Buffer,
}

impl Grow {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Grow {
        Grow { label, usage, buffer: Grow::make(device, label, usage, 16) }
    }

    fn make(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
    }

    /// Makes room for `size` bytes, and says whether the buffer was replaced.
    fn fit(&mut self, device: &wgpu::Device, size: u64) -> bool {
        let size = size.max(16).next_multiple_of(4);
        if size <= self.buffer.size() {
            return false;
        }
        // Grow by half again, so a camera moving through a scene does not replace buffers every frame.
        self.buffer = Grow::make(device, self.label, self.usage, (size + size / 2).next_multiple_of(4));
        true
    }
}

/// The colour image as a texture, for a window to draw.
struct Picture {
    texture: wgpu::Texture,
    storage: wgpu::TextureView,
    sampled: wgpu::TextureView,
}

/// One frame's buffers on the GPU, and the bind groups that use them.
struct Work {
    params: wgpu::Buffer,
    faces: Grow,
    runs: Grow,
    starts: Grow,
    items: Grow,
    objects: Grow,
    depth: Grow,
    colour: Grow,
    picture: Option<Picture>,
    groups: Option<[wgpu::BindGroup; 3]>,
    width: u32,
    height: u32,
}

impl Work {
    fn new(device: &wgpu::Device) -> Work {
        let input = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let output = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        Work {
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("params"),
                size: size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            faces: Grow::new(device, "faces", input),
            runs: Grow::new(device, "runs", input),
            starts: Grow::new(device, "bucket starts", input),
            items: Grow::new(device, "bucket items", input),
            objects: Grow::new(device, "objects", output),
            depth: Grow::new(device, "depth", output),
            colour: Grow::new(device, "colour", output),
            picture: None,
            groups: None,
            width: 0,
            height: 0,
        }
    }

    /// Uploads a prepared frame. With `picture`, the colour is also written to a texture of that view format.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &Pipelines,
        p: &Prepared,
        picture: Option<wgpu::TextureFormat>,
    ) {
        let pixels = (p.width * p.height) as u64 * 4;
        let mut changed = self.groups.is_none();
        changed |= self.faces.fit(device, size_of_val(p.faces.as_slice()) as u64);
        changed |= self.runs.fit(device, size_of_val(p.runs.as_slice()) as u64);
        changed |= self.starts.fit(device, size_of_val(p.bucket_start.as_slice()) as u64);
        changed |= self.items.fit(device, size_of_val(p.items.as_slice()) as u64);
        changed |= self.objects.fit(device, pixels);
        changed |= self.depth.fit(device, pixels);
        changed |= self.colour.fit(device, pixels);
        if let Some(format) = picture
            && (self.picture.is_none() || self.width != p.width || self.height != p.height)
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("picture"),
                size: wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[format],
            });
            let storage = texture.create_view(&wgpu::TextureViewDescriptor::default());
            // Only sampled: an sRGB view may not be a storage binding.
            let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                ..Default::default()
            });
            self.picture = Some(Picture { texture, storage, sampled });
            changed = true;
        }
        self.width = p.width;
        self.height = p.height;

        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&Params::of(p)));
        for (grow, bytes) in [
            (&self.faces, bytemuck::cast_slice::<_, u8>(&p.faces)),
            (&self.runs, bytemuck::cast_slice(&p.runs)),
            (&self.starts, bytemuck::cast_slice(&p.bucket_start)),
            (&self.items, bytemuck::cast_slice(&p.items)),
        ] {
            if !bytes.is_empty() {
                queue.write_buffer(&grow.buffer, 0, bytes);
            }
        }

        if changed {
            let bind = |pipeline: &wgpu::ComputePipeline, entries: &[(u32, wgpu::BindingResource)]| {
                let entries: Vec<wgpu::BindGroupEntry> = entries
                    .iter()
                    .map(|(binding, resource)| wgpu::BindGroupEntry { binding: *binding, resource: resource.clone() })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                })
            };
            fn whole(b: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
                b.as_entire_binding()
            }
            let resolve = bind(
                &pipelines.resolve,
                &[
                    (0, whole(&self.params)),
                    (1, whole(&self.faces.buffer)),
                    (2, whole(&self.runs.buffer)),
                    (3, whole(&self.starts.buffer)),
                    (4, whole(&self.items.buffer)),
                    (5, whole(&self.objects.buffer)),
                    (6, whole(&self.depth.buffer)),
                    (7, whole(&self.colour.buffer)),
                ],
            );
            let edges =
                bind(&pipelines.edges, &[(0, whole(&self.params)), (1, whole(&self.objects.buffer)), (2, whole(&self.colour.buffer))]);
            let present = match &self.picture {
                Some(picture) => bind(
                    &pipelines.present,
                    &[
                        (0, whole(&self.params)),
                        (1, whole(&self.colour.buffer)),
                        (2, wgpu::BindingResource::TextureView(&picture.storage)),
                    ],
                ),
                // Never dispatched without a picture; a group keeps the array whole.
                None => edges.clone(),
            };
            self.groups = Some([resolve, edges, present]);
        }
    }

    /// Records the frame's compute passes.
    fn record(&self, encoder: &mut wgpu::CommandEncoder, pipelines: &Pipelines) {
        let Some([resolve, edges, present]) = &self.groups else { return };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("frame"), timestamp_writes: None });
        let groups = (self.width.div_ceil(8), self.height.div_ceil(8));
        pass.set_pipeline(&pipelines.resolve);
        pass.set_bind_group(0, resolve, &[]);
        pass.dispatch_workgroups(groups.0, groups.1, 1);
        pass.set_pipeline(&pipelines.edges);
        pass.set_bind_group(0, edges, &[]);
        pass.dispatch_workgroups(groups.0, groups.1, 1);
        if self.picture.is_some() {
            pass.set_pipeline(&pipelines.present);
            pass.set_bind_group(0, present, &[]);
            pass.dispatch_workgroups(groups.0, groups.1, 1);
        }
    }

    /// Reads `words` 32-bit words of `buffer` back from `offset` words in, waiting for the GPU.
    fn read(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, offset: u64, words: u64) -> Result<Vec<u32>, Error> {
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read back"),
            size: words * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("read back") });
        encoder.copy_buffer_to_buffer(buffer, offset * 4, &read, 0, words * 4);
        queue.submit([encoder.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |_| ());
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| Error::Gpu(format!("the GPU did not finish the frame: {e}")))?;
        let view = read.get_mapped_range(..).map_err(|e| Error::Gpu(format!("the frame could not be read back from the GPU: {e}")))?;
        let out = bytemuck::cast_slice::<u8, u32>(&view).to_vec();
        drop(view);
        read.unmap();
        Ok(out)
    }
}

/// The engine on a device of its own, drawing each frame and reading it back: for the command line and tests.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    adapter: String,
}

/// A GPU device of the engine's own, with its queue and the adapter's name and backend. It prefers the fastest
/// GPU; with `SPUMCAKE_RENDER_ADAPTER=software` it uses the system's software renderer, such as Mesa's lavapipe.
pub fn device() -> Result<(wgpu::Device, wgpu::Queue, String), Error> {
    device_on(std::env::var("SPUMCAKE_RENDER_ADAPTER").is_ok_and(|v| v == "software"))
}

/// A GPU device of the engine's own, on the system's software renderer when `software` is set, as the checks
/// use so their pixels do not depend on the GPU.
pub fn device_on(software: bool) -> Result<(wgpu::Device, wgpu::Queue, String), Error> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: software,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|e| Error::Gpu(format!("no GPU adapter: {e}")))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor { label: Some("spumcake-render"), ..Default::default() }),
    )
    .map_err(|e| Error::Gpu(format!("no GPU device on {}: {e}", info.name)))?;
    Ok((device, queue, format!("{} ({:?})", info.name, info.backend)))
}

impl Renderer {
    /// A renderer on a device of its own, from [`device`].
    pub fn new() -> Result<Renderer, Error> {
        Renderer::on(device()?)
    }

    /// A renderer on the system's software renderer, such as Mesa's lavapipe, from [`device_on`].
    pub fn software() -> Result<Renderer, Error> {
        Renderer::on(device_on(true)?)
    }

    fn on((device, queue, adapter): (wgpu::Device, wgpu::Queue, String)) -> Result<Renderer, Error> {
        let pipelines = Pipelines::new(&device);
        Ok(Renderer { device, queue, pipelines, adapter })
    }

    /// The adapter's name and backend, for the record of a render.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Draws one frame of the scene and reads it back.
    pub fn render(&self, scene: &scene::Scene, request: &Request) -> Result<Frame, Error> {
        let p = prepare(scene, request)?;
        let mut work = Work::new(&self.device);
        work.upload(&self.device, &self.queue, &self.pipelines, &p, None);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        work.record(&mut encoder, &self.pipelines);
        self.queue.submit([encoder.finish()]);
        let pixels = (p.width * p.height) as u64;
        let objects = Work::read(&self.device, &self.queue, &work.objects.buffer, 0, pixels)?;
        let depth = Work::read(&self.device, &self.queue, &work.depth.buffer, 0, pixels)?
            .into_iter()
            .map(|bits| {
                let z = f32::from_bits(bits);
                if z >= 3.0e38 { f32::INFINITY } else { z }
            })
            .collect();
        let colour = Work::read(&self.device, &self.queue, &work.colour.buffer, 0, pixels)?
            .into_iter()
            .flat_map(|c| [c as u8, (c >> 8) as u8, (c >> 16) as u8])
            .collect();
        Ok(Frame { width: p.width, height: p.height, colour, depth, objects })
    }
}

/// The engine inside a window: each frame stays on the window's GPU device, and is drawn into the window's own
/// render pass, scaled to the pass's viewport. Studio keeps one for each 3D view.
pub struct Live {
    pipelines: Pipelines,
    work: Work,
    view_format: wgpu::TextureFormat,
    blit: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    blit_group: Option<wgpu::BindGroup>,
}

impl Live {
    /// A live view drawing into passes whose colour target is `format`, with `samples` samples per pixel.
    pub fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat, samples: u32) -> Live {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The picture holds sRGB values. Read through an sRGB view when the target encodes sRGB, so the values
        // reach the screen unchanged.
        let view_format =
            if format.is_srgb() { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Live { pipelines: Pipelines::new(device), work: Work::new(device), view_format, blit, sampler, blit_group: None }
    }

    /// Prepares a frame and records its compute passes into `encoder`, without submitting it.
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        scene: &scene::Scene,
        request: &Request,
    ) -> Result<(), Error> {
        let p = prepare(scene, request)?;
        let before = self.work.picture.as_ref().map(|picture| picture.texture.clone());
        self.work.upload(device, queue, &self.pipelines, &p, Some(self.view_format));
        let after = self.work.picture.as_ref().map(|picture| picture.texture.clone());
        if self.blit_group.is_none() || before != after {
            let picture = self.work.picture.as_ref().unwrap();
            self.blit_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit"),
                layout: &self.blit.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&picture.sampled) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            }));
        }
        self.work.record(encoder, &self.pipelines);
        Ok(())
    }

    /// Draws the last frame over the pass's whole viewport. Before the first frame it draws nothing.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(group) = &self.blit_group else { return };
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The last frame's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.work.width, self.work.height]
    }

    /// Which of the scene's objects the last frame shows at a pixel, as an index into `Scene::objects`. It waits
    /// for the GPU.
    pub fn object_at(&self, device: &wgpu::Device, queue: &wgpu::Queue, at: [u32; 2]) -> Option<usize> {
        if self.blit_group.is_none() || at[0] >= self.work.width || at[1] >= self.work.height {
            return None;
        }
        let k = (at[1] * self.work.width + at[0]) as u64;
        let o = *Work::read(device, queue, &self.work.objects.buffer, k, 1).ok()?.first()?;
        (o != Frame::NONE).then_some(o as usize)
    }

    /// The last frame's object pass, row by row, with [`Frame::NONE`] where nothing is drawn. It waits for the GPU,
    /// so call it when the camera settles, not every frame.
    pub fn objects(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32> {
        if self.blit_group.is_none() {
            return Vec::new();
        }
        let pixels = (self.work.width * self.work.height) as u64;
        Work::read(device, queue, &self.work.objects.buffer, 0, pixels).unwrap_or_default()
    }
}
