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
    event::{Event, WindowEvent},
    window::Fullscreen,
};

fn create_initial_state() -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..WIDTH * HEIGHT)
        .flat_map(|_| rng.gen_range(0..=2u32).to_ne_bytes())
        .collect()
}
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt, Layer};
//const FPS: usize = 60;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub async fn run() {
    #[cfg(not(target_arch = "wasm32"))]
    {
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
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .unwrap_or(&surface_caps.formats[0])
            .clone();
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

        info!("Creating data buffer");
        //let data_buffer_size = (WIDTH * HEIGHT) as wgpu::BufferAddress;
        let data_buffer_desc = wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE,
            label: Some("Data Buffer"),
            contents: &create_initial_state(),
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
        self.queue.submit(Some(encoder.finish()));
        //self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(id));
        output.present();
        Ok(())
    }
}
