//! cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline
//! Uses the production canvas callback, offscreen. Wall timings include CPU
//! staging, upload, submission and waiting; GPU timestamps measure draw only.
//! No native window, event-loop scheduling, compositor/vsync or display latency.
#[allow(dead_code)]
#[path = "../src/canvas_renderer.rs"]
mod canvas_renderer;

use canvas_renderer::{CanvasCallback, CanvasRenderer, PendingUpload, Uniforms};
use eframe::{
    egui,
    egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor},
    wgpu,
};
use rspaint_core::{Color, Layer, LayerNode, PixelBuffer};
use rspaint_editor::{Editor, PointerButton};
use std::{
    error::Error,
    time::{Duration, Instant},
};

struct Queries {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

const CASES: [&str; 5] = [
    "partial-1",
    "partial-upper-32",
    "full-upload-1",
    "opacity-8",
    "detailed-opacity-8",
];

fn editor_for_case(width: u32, height: u32, case: &str) -> Editor {
    let mut e = Editor::new();
    e.load_buffer(PixelBuffer::new(width, height, Color::WHITE), None);
    let count = match case {
        "partial-upper-32" => 32,
        "opacity-8" | "detailed-opacity-8" => 8,
        _ => 1,
    };
    if count > 1 {
        // Share uniform source storage to bound fixture RAM. This still blends
        // every layer and does not model independently allocated layer memory.
        let mut pixels = PixelBuffer::new(width, height, Color::rgba(60, 130, 190, 140));
        if case == "detailed-opacity-8" {
            let mut seed = 0x12345678u32;
            for p in pixels.data_mut().chunks_exact_mut(4) {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                p.copy_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 140]);
            }
        }
        e.document.layers.splice(
            0..0,
            (100..100 + count - 1)
                .map(|id| LayerNode::Layer(Layer::new(id, "Static", pixels.clone()))),
        );
        e.document.next_id = 100 + count;
    }
    if matches!(case, "opacity-8" | "detailed-opacity-8") {
        e.select_layer(100, false, false);
    }
    let mut ids = std::collections::HashSet::new();
    for node in &e.document.layers {
        assert!(ids.insert(node.id()), "duplicate benchmark layer ID");
    }
    assert_eq!(ids.len(), count as usize);
    assert!(ids.contains(&e.document.active));
    assert!(ids.iter().all(|&id| id < e.document.next_id));
    e
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    resources: &mut CallbackResources,
    target: &wgpu::TextureView,
    callback: &CanvasCallback,
    queries: Option<&Queries>,
) -> Result<(f64, f64, Option<f64>, Instant), Box<dyn Error>> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let start = Instant::now();
    let mut commands = callback.prepare(
        device,
        queue,
        &ScreenDescriptor {
            size_in_pixels: [1024, 768],
            pixels_per_point: 1.0,
        },
        &mut encoder,
        resources,
    );
    let prepare = start.elapsed().as_secs_f64() * 1000.0;
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas benchmark"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: queries.map(|q| wgpu::RenderPassTimestampWrites {
                    query_set: &q.set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            })
            .forget_lifetime();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 768.0));
        callback.paint(
            egui::epaint::PaintCallbackInfo {
                viewport: rect,
                clip_rect: rect,
                pixels_per_point: 1.0,
                screen_size_px: [1024, 768],
            },
            &mut pass,
            resources,
        );
    }
    if let Some(q) = queries {
        encoder.resolve_query_set(&q.set, 0..2, &q.resolve, 0);
        encoder.copy_buffer_to_buffer(&q.resolve, 0, &q.read, 0, 16);
    }
    commands.push(encoder.finish());
    let start = Instant::now();
    let submitted = queue.submit(commands);
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submitted),
        timeout: Some(Duration::from_secs(30)),
    })?;
    let finished = Instant::now();
    let submit_done = start.elapsed().as_secs_f64() * 1000.0;
    let gpu = if let Some(q) = queries {
        let (tx, rx) = std::sync::mpsc::channel();
        q.read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        rx.recv_timeout(Duration::from_secs(30))??;
        let bytes = q.read.slice(..).get_mapped_range();
        let stamps: &[u64] = bytemuck::cast_slice(&bytes);
        let ms = stamps[1].saturating_sub(stamps[0]) as f64 * queue.get_timestamp_period() as f64
            / 1_000_000.0;
        drop(bytes);
        q.read.unmap();
        Some(ms)
    } else {
        None
    };
    Ok((prepare, submit_done, gpu, finished))
}

fn summary(values: &mut [f64]) -> String {
    values.sort_by(f64::total_cmp);
    format!(
        "median={:.3}ms p95={:.3}ms",
        values[values.len() / 2],
        values[((values.len() - 1) as f64 * 0.95).ceil() as usize]
    )
}

async fn run() -> Result<(), Box<dyn Error>> {
    let only_case = std::env::args().find_map(|arg| arg.strip_prefix("--case=").map(str::to_owned));
    let instance = wgpu::Instance::new(&Default::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await?;
    println!("adapter: {:?}", adapter.get_info());
    let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("canvas benchmark"),
            required_features: features,
            ..Default::default()
        })
        .await?;
    let queries = if features.contains(wgpu::Features::TIMESTAMP_QUERY) {
        Some(Queries {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
        })
    } else {
        println!("GPU draw timestamps unavailable");
        None
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen canvas"),
            size: wgpu::Extent3d {
                width: 1024,
                height: 768,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut resources = CallbackResources::default();
    resources.insert(CanvasRenderer::new(&device, format));
    for (width, height) in [(1920, 1080), (6000, 4000)] {
        for case in CASES {
            if only_case
                .as_deref()
                .is_some_and(|selected| selected != case)
            {
                continue;
            }
            let mut e = editor_for_case(width, height, case);
            let upload = PendingUpload::default();
            let callback = CanvasCallback {
                window_id: 0,
                upload: upload.clone(),
                uniforms: Uniforms {
                    canvas_mag: [width as f32, height as f32, 1024.0 / width as f32, 8.0],
                    cursor: [127.0, 127.0, 16.0, 1.0],
                    flags: [1.0, 1.0, 1.0, 0.0],
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                    params: [0.35, 0.0, 0.0, 0.0],
                },
            };
            upload.stage(&mut e);
            render(
                &device,
                &queue,
                &mut resources,
                &target,
                &callback,
                queries.as_ref(),
            )?;
            e.begin_stroke((230, 240), PointerButton::Primary);
            let mut stage_times = Vec::new();
            let mut prepare_times = Vec::new();
            let mut submit_times = Vec::new();
            let mut draw_times = Vec::new();
            let mut totals = Vec::new();
            for i in 0..9 {
                let start = Instant::now();
                e.update_stroke(
                    (
                        if i % 2 == 0 { 294 + i } else { 230 + i },
                        if i % 2 == 0 { 288 } else { 240 },
                    ),
                    false,
                );
                if case == "full-upload-1" {
                    e.invalidate_canvas();
                }
                if matches!(case, "opacity-8" | "detailed-opacity-8") {
                    e.set_layer_opacity(100, 50 + i as u8);
                }
                let staging = Instant::now();
                upload.stage(&mut e);
                let stage = staging.elapsed().as_secs_f64() * 1000.0;
                let (prepare, submit, gpu, finished) = render(
                    &device,
                    &queue,
                    &mut resources,
                    &target,
                    &callback,
                    queries.as_ref(),
                )?;
                if i >= 2 {
                    stage_times.push(stage);
                    prepare_times.push(prepare);
                    submit_times.push(submit);
                    if let Some(gpu) = gpu {
                        draw_times.push(gpu);
                    }
                    totals.push(finished.duration_since(start).as_secs_f64() * 1000.0);
                }
            }
            println!("{width}x{height} {case}: CPU compose/stage {}; prepare/upload enqueue {}; submit-to-complete {}; draw GPU {}; pipeline {} (7 samples)",
                summary(&mut stage_times),summary(&mut prepare_times),summary(&mut submit_times),
                if draw_times.is_empty(){"unavailable".to_owned()}else{summary(&mut draw_times)},summary(&mut totals));
            println!("  cache: {:?}", e.document.composition_cache_stats());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(run())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_scenes_have_unique_ids_and_edit_the_selected_opacity_layer() {
        for case in CASES {
            let e = editor_for_case(32, 24, case);
            assert_eq!((e.document.width(), e.document.height()), (32, 24));
            if matches!(case, "opacity-8" | "detailed-opacity-8") {
                assert_eq!(e.active_layer_id(), 100);
                assert!(e.layer_selection().contains(&100));
            }
        }
    }
}
