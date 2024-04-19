#![forbid(unsafe_code)]

use anyhow::Context;
use rand::Rng;
use std::{num::NonZeroU64, sync::Arc};
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
use game_loop::{game_loop, TimeTrait};
use tracing::{error, info, instrument};
use wgpu::{
    include_wgsl,
    util::DeviceExt,
    Instance, Surface,
};
use winit::{
    event::{Event, WindowEvent}, keyboard::NamedKey, window::Fullscreen
};

fn create_initial_state() -> Vec<u32> {
    let mut rng = rand::thread_rng();
    (0..WIDTH * HEIGHT)
        .map(|_| rng.gen_range(0..=2u32))
        .collect()
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;


//const FPS: usize = 60;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub async fn run() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt, Layer};
        let stupid_amd_filter = filter::filter_fn(|meta| {
            // see https://github.com/gfx-rs/wgpu/issues/4247
            // basically, amd does some weird stuff on vulkan that causes error spam
            // even though it's not really an error
            meta.target() != "wgpu_hal::auxil::dxgi::exception"
        });
        let stupid_amd_layer = tracing_subscriber::fmt::layer();
        let min_level_filter = tracing_subscriber::filter::LevelFilter::INFO;
        tracing_subscriber::registry()
            .with(stupid_amd_layer.with_filter(stupid_amd_filter))
            .with(min_level_filter)
            .init();
    }
    #[cfg(target_arch = "wasm32")]
    {
        tracing_wasm::set_as_global_default();
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    }
    if let Err(e) = render_to_window().await {
        error!("Error: {:?}", e);
    }
}

async fn render_to_window() -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoop::new().context("Failed to create event loop")?;
    info!("Creating window");
    let window = winit::window::WindowBuilder::new()
        .with_title("ow my eyes")
        .with_inner_size(winit::dpi::PhysicalSize::new(WIDTH, HEIGHT))
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .with_resizable(false)
        .build(&event_loop)
        .context("Failed to create window")?;
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let dst = doc.get_element_by_id("wasm")?;
                let canvas = web_sys::Element::from(window.canvas()?);
                dst.append_child(&canvas).ok()?;
                Some(())
            })
            .context("Couldn't append canvas to document body.")?;
    }

    let window = Arc::new(window);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    info!("Creating surface");
    let surface = 
        instance
            .create_surface(window.clone())
            .context("Failed to create surface!")?;
    // let surface_caps = surface.get_capabilities(&game.adapter);
    info!("Creating game");
    let game = Game::new(surface, instance).await?;
    //let winit_game = WinitGame { game, surface };
    info!("Starting game loop");
    game_loop(
        event_loop,
        Arc::clone(&window),
        game,
        60,
        0.1,
        |u| {
            u.game.tick();
            let between = u.current_instant().sub(&u.previous_instant());
            // use between to calculate fps
            if u.number_of_renders() % 60 == 0 {
                // round to no decimal places
                let fps = (1.0 / between).round() as u64;
                info!("FPS: {}", fps);
            }
        },
        |r| {
            #[cfg(not(target_arch = "wasm32"))]
            if r.window_occluded {
                //info!("Window occluded");
                return;
            }
            if let Err(e) = r.game.render() {
                error!("Render failed: {:?}", e);
                r.exit();
            }
        },
        |h, e| {
            if let Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } = e
            {
                h.exit();
            }
            if let Event::WindowEvent { event: WindowEvent::KeyboardInput { event, .. }, .. } = e {
                if event.state.is_pressed() {
                    if event.logical_key == "p" {
                        h.game.color_manager.toggle_blender();
                    } else if cfg!(not(target_arch = "wasm32")) && event.logical_key == NamedKey::Escape {
                        h.exit();
                    } else if event.logical_key == NamedKey::ArrowRight {
                        h.game.color_manager.next();
                    } else if event.logical_key == NamedKey::ArrowLeft {
                        h.game.color_manager.prev();
                    } else if event.logical_key == "r" {
                        h.game.queue
                            .write_buffer(&h.game.data_buffer, 0, 
                            bytemuck::cast_slice(&create_initial_state()));
                    }
                }
                
            }
        },
    )
    .context("Game loop failed")?;
    info!("Window closed");
    drop(window); // game should be dropped by this point so its okay to drop the window (the surface is already gone)
    Ok(())
}

struct Game {
    device: wgpu::Device,
    //adapter: wgpu::Adapter,
    queue: wgpu::Queue,
    /*texture: wgpu::Texture,
    texture_view: wgpu::TextureView,*/
    //out_buffer: wgpu::Buffer,
    data_buffer: wgpu::Buffer,
    data_buffer_copy: wgpu::Buffer,
    render_pipeline: wgpu::RenderPipeline,
    compute_pipeline: wgpu::ComputePipeline,
    compute_bind_group_layout: wgpu::BindGroupLayout,
    render_bind_group_layout: wgpu::BindGroupLayout,
    color_manager: ColorModes,
    color_buffer: wgpu::Buffer,
    surface: wgpu::Surface<'static>,
}

impl Game {
    #[instrument(skip_all)]
    async fn new(surface: Surface<'static>, instance: Instance) -> anyhow::Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("No adapter found!")?;
        info!("Adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    label: None,
                },
                None,
            )
            .await
            .context("No device found!")?;
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = *surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .unwrap_or(&surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: WIDTH,
            height: HEIGHT,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        info!("Creating color buffer");
        let color_manager = ColorModes::new();
        let colors = color_manager.colors();
        let color_buffer_desc = wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("Color Buffer"),
            contents: bytemuck::cast_slice(&colors),
        };
        let color_buffer = device.create_buffer_init(&color_buffer_desc);

        info!("Creating data buffer");
        let init_state = create_initial_state();
        //let data_buffer_size = (WIDTH * HEIGHT) as wgpu::BufferAddress;
        let data_buffer_desc = wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("Data Buffer"),
            contents: bytemuck::cast_slice(&init_state),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        count: None,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            has_dynamic_offset: false,
                            min_binding_size: None,
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
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
                    format: surface_format,
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
        Ok(Self {
            device,
            queue,
            //adapter,
            data_buffer,
            render_pipeline,
            compute_pipeline,
            data_buffer_copy,
            compute_bind_group_layout,
            render_bind_group_layout,
            surface,
            color_manager,
            color_buffer,
        })
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
        self.color_manager.tick();
        self.device
            .poll(wgpu::MaintainBase::WaitForSubmissionIndex(ind));
    }
    fn render(&mut self) -> anyhow::Result<()> {
        let output = self.surface.get_current_texture()?;
        let texture_view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            ..Default::default()
        });
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
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.color_buffer.as_entire_binding(),
            }],
        });
        {
            let render_pass_desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
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
        self.color_manager.push_to_gpu(&mut self.queue, &self.color_buffer);
        self.queue.submit(Some(encoder.finish()));
        //self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(id));
        output.present();
        Ok(())
    }
}




struct ColorModes {
    mode: usize,
    colors: Box<[[Color; 3]]>,
    blender: ColorBlender,
    blender_enabled: bool,
    should_push: bool,
}

impl ColorModes {
    fn new() -> Self {
        Self {
            mode: 0,
            blender: ColorBlender::new(0.01),
            colors: vec![
                [ // standard RGB
                    [1.0, 0.0, 0.0].into(),
                    [0.0, 1.0, 0.0].into(),
                    [0.0, 0.0, 1.0].into(),
                ],
                [ // CMY
                    [0.0, 1.0, 1.0].into(),
                    [1.0, 0.0, 1.0].into(),
                    [1.0, 1.0, 0.0].into(),
                ],
                [ // darker rgb
                    [0.8, 0.0, 0.0].into(),
                    [0.0, 0.8, 0.0].into(),
                    [0.0, 0.0, 0.8].into(),
                ],
                [ // grayscale
                    [0.0, 0.0, 0.0].into(),
                    [0.5, 0.5, 0.5].into(),
                    [1.0, 1.0, 1.0].into(),
                ]
            ].into_boxed_slice(),
            blender_enabled: false,
            should_push: true
        }
    }
    fn tick(&mut self) {
        if self.blender_enabled {
            self.blender.step();
        } else {
            self.blender.reset(self.r_color(), self.p_color(), self.s_color());
            self.should_push = true;
        }
    }
    #[inline]
    fn r_color(&self) -> Color {
        self.colors[self.mode][0]
    }
    #[inline]
    fn p_color(&self) -> Color {
        self.colors[self.mode][1]
    }
    #[inline]
    fn s_color(&self) -> Color {
        self.colors[self.mode][2]
    }
    fn colors(&self) -> [f32; 9] {
        if self.blender_enabled {
            self.blender.colors()
        } else {
            let colors = &self.colors[self.mode];
            let rc = colors[0].color();
            let pc = colors[1].color();
            let sc = colors[2].color();
            [
                rc[0], rc[1], rc[2],
                pc[0], pc[1], pc[2],
                sc[0], sc[1], sc[2],
            ]
        }
    }
    fn push_to_gpu(&mut self, q: &mut wgpu::Queue, buffer: &wgpu::Buffer) {
        if self.blender_enabled { 
            self.blender.push_to_gpu(q, buffer);
        } else if self.should_push {
            self.should_push = false; // we should only push once per change
            // this probably doesn't save any meaningful amount of time but i really dont care
            q.write_buffer(buffer, 0, bytemuck::cast_slice(&self.colors()));
        }
    }

    fn next(&mut self) {
        self.mode = (self.mode + 1) % self.colors.len();
        self.should_push = true;
    }
    fn prev(&mut self) {
        self.mode = (self.mode + self.colors.len() - 1) % self.colors.len();
        self.should_push = true;
    }
    fn toggle_blender(&mut self) {
        self.blender_enabled = !self.blender_enabled;
    }
    
}


struct ColorBlender {
    rock: Color,
    paper: Color,
    scissors: Color,
    rock_target: Color,
    paper_target: Color,
    scissors_target: Color,
    dirty: bool,
    has_done_step: bool,
    pub blend_amt: f64,
}

impl ColorBlender {
    fn new(blend_amt: f64) -> Self {
        Self {
            rock:     [1.0, 0.0, 0.0].into(),
            paper:    [0.0, 1.0, 0.0].into(),
            scissors: [0.0, 0.0, 1.0].into(),
            dirty: false,
            has_done_step: false,
            rock_target: [1.0, 0.0, 0.0].into(),
            paper_target: [0.0, 1.0, 0.0].into(),
            scissors_target: [0.0, 0.0, 1.0].into(),
            blend_amt
        }
    }
    #[rustfmt::skip]
    fn colors(&self) -> [f32; 9] {
        let r=  self.rock.color();
        let p = self.paper.color();
        let s = self.scissors.color();
        [
            r[0], r[1], r[2],
            p[0], p[1], p[2],
            s[0], s[1], s[2],
        ]
    }
    fn reset(&mut self, to_r: Color, to_p: Color, to_s: Color) {
        self.rock = to_r;
        self.paper = to_p;
        self.scissors = to_s;
        self.rock_target = to_r;
        self.paper_target = to_p;
        self.scissors_target = to_s;
        if self.has_done_step {
            self.has_done_step = false;
            self.dirty = true;
        }
    }
    fn step(&mut self) {
        self.has_done_step = true;
        // if we've hit a target on any 3, reset to new random target
        if (self.rock - self.rock_target).abs().sum() < self.blend_amt {
            self.rock_target = self.rock_target.rand_within(0.5)
        }
        if (self.paper - self.paper_target).abs().sum() < self.blend_amt {
            self.paper_target = self.paper_target.rand_within(0.5)
        }
        if (self.scissors - self.scissors_target).abs().sum() < self.blend_amt {
            self.scissors_target = self.scissors_target.rand_within(0.5)
        }
        // blend towards target`
        self.rock.blend_to_target(&self.rock_target, self.blend_amt);
        self.paper.blend_to_target(&self.paper_target, self.blend_amt);
        self.scissors.blend_to_target(&self.scissors_target, self.blend_amt);
        self.dirty = true;
    }
    fn push_to_gpu(&self, q: &mut wgpu::Queue, buffer: &wgpu::Buffer) {
        if self.dirty {
            let colors = self.colors();
            q.write_buffer(buffer, 0, bytemuck::cast_slice(&colors));
        }
    }
}


// COLOR MATH AAAAAAAA
#[derive(Debug, Clone, Copy)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
}

impl Color {
    fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
    fn color(&self) -> [f32; 3] {
        [
            self.r as f32, 
            self.g as f32, 
            self.b as f32
        ]
    }
    fn abs(self) -> Self {
        Self {
            r: self.r.abs(),
            g: self.g.abs(),
            b: self.b.abs(),
        }
    }
    /*fn rand() -> Self {
        let rng = &mut rand::thread_rng();
        Self {
            r: rng.gen_range(0.0..=1.0f64),
            g: rng.gen_range(0.0..=1.0f64), 
            b: rng.gen_range(0.0..=1.0f64),
        }
    
    }*/
    fn rand_within(&self, amt: f64) -> Self {
        let r_min = (self.r - amt).min(1.0).max(0.0);
        let r_max = (self.r + amt).min(1.0).max(0.0);
        let g_min = (self.g - amt).min(1.0).max(0.0);
        let g_max = (self.g + amt).min(1.0).max(0.0);
        let b_min = (self.b - amt).min(1.0).max(0.0);
        let b_max = (self.b + amt).min(1.0).max(0.0);
        let rng = &mut rand::thread_rng();
        Self {
            r: rng.gen_range(r_min..=r_max),
            g: rng.gen_range(g_min..=g_max),
            b: rng.gen_range(b_min..=b_max),
        }
    }
    fn sum(self) -> f64 {
        self.r + self.g + self.b
    }
    fn blend_to_target(&mut self, target: &Self, amt: f64) {
        // sub/add by amt for self to get closer (unless we're < amt away)
        let diff_r = target.r - self.r;
        let diff_g = target.g - self.g;
        let diff_b = target.b - self.b;
        if diff_r.abs() < amt {
            self.r = target.r;
        } else {
            self.r += diff_r.signum() * amt;
        }
        if diff_g.abs() < amt {
            self.g = target.g;
        } else {
            self.g += diff_g.signum() * amt;
        }
        if diff_b.abs() < amt {
            self.b = target.b;
        } else {
            self.b += diff_b.signum() * amt;
        }
    }
}

impl From<[f64; 3]> for Color {
    fn from(arr: [f64; 3]) -> Self {
        Self {
            r: arr[0],
            g: arr[1],
            b: arr[2],
        }
    }
}

impl std::ops::Sub for Color {
    type Output = Color;
    fn sub(self, rhs: Self) -> Self::Output {
        Color::new(self.r - rhs.r, self.g - rhs.g, self.b - rhs.b)
    }
}
impl std::ops::Sub<&Color> for Color {
    type Output = Color;
    fn sub(self, rhs: &Self) -> Self::Output {
        Color::new(self.r - rhs.r, self.g - rhs.g, self.b - rhs.b)
    }
}

impl std::ops::Sub<&Color> for &Color {
    type Output = Color;
    fn sub(self, rhs: &Color) -> Self::Output {
        Color::new(self.r - rhs.r, self.g - rhs.g, self.b - rhs.b)
    }
}