use rand::seq::SliceRandom;
use std::{num::NonZeroU64, process::Child, sync::Arc};
use strum::VariantArray;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
use game_loop::game_loop;
use indicatif::{ProgressIterator, ProgressStyle};
use tracing::{info, instrument};
use wgpu::{
    include_wgsl,
    rwh::{HasDisplayHandle, HasWindowHandle},
    util::DeviceExt,
    Origin3d,
};
use winit::{
    event::{Event, WindowEvent},
    window::Fullscreen,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub enum PixelState {
    Rock,
    Paper,
    Scissors,
}

impl PixelState {
    /// Returns the color of a state in RGBA
    #[inline]
    pub fn color(&self) -> [u8; 4] {
        match self {
            PixelState::Rock => [0xff, 0x00, 0x00, 255],
            PixelState::Paper => [0x00, 0xff, 0x00, 255],
            PixelState::Scissors => [0x00, 0x00, 0xff, 255],
            // PixelState::Gun =>      [0xff, 0xff, 0xf, 255],
        }
    }
    pub fn index(&self) -> u8 {
        match self {
            PixelState::Rock => 0,
            PixelState::Paper => 1,
            PixelState::Scissors => 2,
            //PixelState::Gun => 3,
        }
    }
    pub fn colors() -> [[u8; 4]; 3] {
        [
            PixelState::Rock.color(),
            PixelState::Paper.color(),
            PixelState::Scissors.color(),
            //PixelState::Gun.color(),
        ]
    }
    #[inline]
    pub fn rand() -> Self {
        *PixelState::VARIANTS
            .choose(&mut rand::thread_rng())
            .expect("has variants")
    }
    pub fn interaction(self, neighbors: &[Option<Self>]) -> Self {
        // Count the number of each type of neighbor
        let mut counts = vec![0; PixelState::VARIANTS.len()];
        for neighbor in neighbors.iter() {
            match neighbor {
                Some(PixelState::Rock) => counts[0] += 1,
                Some(PixelState::Paper) => counts[1] += 1,
                Some(PixelState::Scissors) => counts[2] += 1,
                None => {}
            }
        }
        // check if we are beaten (and if the beater is > 2)
        let our_foe_count = match self {
            PixelState::Rock => counts[1],  // paper beats rock
            PixelState::Paper => counts[2], // gun beats paper
            PixelState::Scissors => counts[0], // rock beats scissors
                                             //PixelState::Gun => counts[2], // scissors beat gun
        };
        if our_foe_count > 2 {
            match self {
                PixelState::Paper => PixelState::Scissors, // paper becomes scissors
                PixelState::Rock => PixelState::Paper,     // rock becomes paper
                PixelState::Scissors => PixelState::Rock,  // scissors becomes rock
            }
        } else {
            self
        }
    }
}

const FPS: usize = 60;

pub async fn run() {
    render_to_window().await
}

struct WinitGame<'a> {
    game: Game,
    surface: wgpu::Surface<'a>,
}

async fn render_to_window() {
    let game = Game::new(false).await;

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = winit::window::WindowBuilder::new()
        .with_title("ow my eyes")
        .with_inner_size(winit::dpi::PhysicalSize::new(WIDTH, HEIGHT))
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .with_resizable(false)
        .build(&event_loop)
        .unwrap();
    let window = Arc::new(window);
    // SAFETY: We are using the raw handle to create the surface
    // this handle lives for the lifetime of the window
    // the window will outlive the surface
    let surface = unsafe {
        game.instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: window.display_handle().unwrap().as_raw(),
                raw_window_handle: window.window_handle().unwrap().as_raw(),
            })
            .unwrap()
    };
    // let surface_caps = surface.get_capabilities(&game.adapter);
    // Shader code in this tutorial assumes an sRGB surface texture. Using a different
    // one will result in all the colors coming out darker. If you want to support non
    // sRGB surfaces, you'll need to account for that when drawing to the frame.
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: WIDTH,
        height: HEIGHT,
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
    };
    surface.configure(&game.device, &config);

    let winit_game = WinitGame { game, surface };

    game_loop(
        event_loop,
        Arc::clone(&window),
        winit_game,
        60,
        0.1,
        |u| {
            u.game.game.tick();
        },
        |r| {
            r.game.game.render(0);
            // gotta copy the texture to the surface
            let mut command_encoder =
                r.game
                    .game
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Copy Texture to Surface"),
                    });
            if r.window_occluded {
                return;
            }
            let st = r.game.surface.get_current_texture().unwrap();
            command_encoder.copy_texture_to_texture(
                wgpu::ImageCopyTexture {
                    texture: &r.game.game.texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTexture {
                    texture: &st.texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
            );
            let id = r.game.game.queue.submit(Some(command_encoder.finish()));
            r.game
                .game
                .device
                .poll(wgpu::Maintain::WaitForSubmissionIndex(id));
            st.present();
        },
        |h, e| {
            if let Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } = e
            {
                h.exit();
            }
        },
    )
    .unwrap();
    info!("Window closed");
    drop(window); // winit game should be dropped by this point i hope
}
#[allow(dead_code)]
async fn render_to_file() {
    let mut game = Game::new(true).await;
    const RENDER_TIME: usize = 60; // in seconds
    const FRAMES: usize = FPS * RENDER_TIME;
    info!(
        "Doing {} ({} at {}fps) renders, may god have mercy",
        FRAMES,
        humantime::format_duration(std::time::Duration::from_secs(FRAMES as u64 / FPS as u64)),
        FPS
    );
    let now = std::time::Instant::now();
    for i in (0..FRAMES).progress_with_style(ProgressStyle::with_template("[{elapsed}/{eta}] {bar:40.cyan/blue} {pos:>7}/{len:7} ({percent}%) {per_sec}tps ({msg})").unwrap()).with_message("Rendering") {
        game.render(i);
        game.tick();
    }
    let elapsed = now.elapsed();
    info!(
        "Render Compelete in {}, god save us all",
        humantime::format_duration(elapsed)
    )
}

struct Game {
    instance: wgpu::Instance,
    device: wgpu::Device,
    //adapter: wgpu::Adapter,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    out_buffer: wgpu::Buffer,
    data_buffer: wgpu::Buffer,
    data_buffer_copy: wgpu::Buffer,
    render_pipeline: wgpu::RenderPipeline,
    ffmpeg: Option<Child>,
    compute_pipeline: wgpu::ComputePipeline,
    compute_bind_group_layout: wgpu::BindGroupLayout,
    render_bind_group_layout: wgpu::BindGroupLayout,
}

impl Drop for Game {
    fn drop(&mut self) {
        if let Some(ref mut ffmpeg) = self.ffmpeg {
            ffmpeg.stdin.take(); // drop stdin to kill ffmpeg
            info!("Waiting for ffmpeg to finish");
            ffmpeg.wait().unwrap();
        }
    }
}

impl Game {
    #[instrument]
    async fn new(enable_ffmpeg: bool) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        info!("Adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(&Default::default(), None)
            .await
            .unwrap();
        info!("Creating output texture");
        let texture_desc = wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: Some("Output Texture"),
            view_formats: &[],
        };
        let texture = device.create_texture(&texture_desc);
        let texture_view = texture.create_view(&Default::default());

        info!("Creating output buffer");

        let output_buffer_size =
            (std::mem::size_of::<u32>() as u32 * WIDTH * HEIGHT) as wgpu::BufferAddress;
        let output_buffer_desc = wgpu::BufferDescriptor {
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST
                // this tells wpgu that we want to read this buffer from the cpu
                | wgpu::BufferUsages::MAP_READ,
            label: Some("Output Buffer"),
            mapped_at_creation: false,
        };

        let output_buffer = device.create_buffer(&output_buffer_desc);
        info!("Creating data buffer");
        //let data_buffer_size = (WIDTH * HEIGHT) as wgpu::BufferAddress;
        let data_buffer_desc = wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE,
            label: Some("Data Buffer"),
            contents: &(0..WIDTH * HEIGHT)
                .enumerate()
                .flat_map(|(_, _)| (PixelState::rand() as u32).to_ne_bytes())
                .collect::<Vec<_>>(),
        };
        let data_buffer = device.create_buffer_init(&data_buffer_desc);
        // create data buffer copy
        info!("Creating data buffer copy");
        let data_buffer_desc_copy = wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            label: Some("Data Buffer Copy"),
            contents: &(0..WIDTH * HEIGHT * std::mem::size_of::<u32>() as u32)
                .map(|_| 0)
                .collect::<Vec<_>>(),
        };
        let data_buffer_copy = device.create_buffer_init(&data_buffer_desc_copy);
        info!("Compiling Shader");
        let shader = device.create_shader_module(include_wgsl!("shader.wgsl"));
        info!("Creating render pipeline");
        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    // XXX - some graphics cards do not support empty bind layout groups, so
                    // create a dummy entry.
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        count: None,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            has_dynamic_offset: false,
                            min_binding_size: Some(NonZeroU64::new(16).unwrap()),
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                        },
                    },
                ],
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&render_bind_group_layout],
                push_constant_ranges: &[],
            });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main", // 1.
                buffers: &[],           // 2.
            },
            fragment: Some(wgpu::FragmentState {
                // 3.
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    // 4.
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList, // 1.
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw, // 2.
                cull_mode: Some(wgpu::Face::Back),
                // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None, // 1.
            multisample: wgpu::MultisampleState {
                count: 1,                         // 2.
                mask: !0,                         // 3.
                alpha_to_coverage_enabled: false, // 4.
            },
            multiview: None, // 5.
        });
        info!("Creating compute pipeline");
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    // XXX - some graphics cards do not support empty bind layout groups, so
                    // create a dummy entry.
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        count: None,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            has_dynamic_offset: false,
                            min_binding_size: Some(NonZeroU64::new(16).unwrap()),
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                        },
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        count: None,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            has_dynamic_offset: false,
                            min_binding_size: Some(NonZeroU64::new(16).unwrap()),
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                        },
                    },
                ],
            });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Compute Pipeline Layout"),
            bind_group_layouts: &[&compute_bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "compute",
        });
        let ffmpeg = if enable_ffmpeg {
            info!("Creating ffmpeg process");
            Some(
                std::process::Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-f",
                        "rawvideo",
                        "-vcodec",
                        "rawvideo",
                        "-s",
                        &format!("{}x{}", WIDTH, HEIGHT),
                        "-pix_fmt",
                        "brga",
                        "-r",
                        FPS.to_string().as_str(),
                        "-i",
                        "-",
                        "-c:v",
                        "hevc_amf",
                        "-pix_fmt",
                        "yuv420p",
                        "-an",
                        "-b:v",
                        "10M",
                        "-movflags",
                        "+faststart",
                        "output.mp4",
                    ])
                    .stdin(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        } else {
            None
        };

        Self {
            instance,
            device,
            queue,
            //adapter,
            texture,
            texture_view,
            out_buffer: output_buffer,
            data_buffer,
            render_pipeline,
            ffmpeg,
            compute_pipeline,
            data_buffer_copy,
            compute_bind_group_layout,
            render_bind_group_layout,
        }
    }
    fn tick(&mut self) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Command Encoder"),
            });
        encoder.copy_buffer_to_buffer(
            &self.data_buffer,
            0,
            &self.data_buffer_copy,
            0,
            (WIDTH * HEIGHT * std::mem::size_of::<u32>() as u32) as wgpu::BufferAddress,
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.data_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.data_buffer_copy.as_entire_binding(),
                },
            ],
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute"),
                timestamp_writes: None,
            });
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.set_pipeline(&self.compute_pipeline);
            cpass.dispatch_workgroups(120, 120, 1); // 120x120x1 workgroups with 16x9x1 threads
        }

        let ind = self.queue.submit(Some(encoder.finish()));
        self.device
            .poll(wgpu::MaintainBase::WaitForSubmissionIndex(ind));
    }
    fn render(&mut self, _frame: usize) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Command Encoder"),
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.render_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.data_buffer.as_entire_binding(),
            }],
        });
        {
            let render_pass_desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            };
            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

            render_pass.set_pipeline(&self.render_pipeline);

            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        };
        if self.ffmpeg.is_some() {
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTextureBase {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBufferBase {
                    buffer: &self.out_buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(std::mem::size_of::<u32>() as u32 * WIDTH),
                        rows_per_image: Some(HEIGHT),
                    },
                },
                wgpu::Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
            );
        }
        let id = self.queue.submit(Some(encoder.finish()));
        self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(id));
        if let Some(ref mut ffmpeg) = self.ffmpeg {
            {
                let buffer_slice = self.out_buffer.slice(..);

                // NOTE: We have to create the mapping THEN device.poll() before await
                // the future. Otherwise the application will freeze.
                let (tx, rx) = futures_intrusive::channel::shared::oneshot_channel();
                buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
                    tx.send(result).unwrap();
                });
                self.device.poll(wgpu::Maintain::Wait);
                smol::block_on(rx.receive()).unwrap().unwrap();

                let data = buffer_slice.get_mapped_range();
                use std::io::Write;
                if let Some(i) = ffmpeg.stdin.as_mut() {
                    i.write_all(&data).unwrap()
                }
                //use image::{ImageBuffer, Rgba};
                //let buffer =
                //ImageBuffer::<Rgba<u8>, _>::from_raw(WIDTH, HEIGHT, data).unwrap();
                //buffer.save(format!("out/{}.png", frame)).unwrap();
            }
            self.out_buffer.unmap();
        }
    }
}
