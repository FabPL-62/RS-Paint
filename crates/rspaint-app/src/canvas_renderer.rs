//! GPU rendering of the canvas via a custom wgpu render pass.
//!
//! The document is uploaded to a `wgpu` texture (full or partial updates) and
//! drawn with a single shader that also composites the transparency
//! checkerboard, the pixel grid and the tool cursor. Because all of this runs
//! on the GPU, interaction feedback stays perfectly in sync with the pointer.

use eframe::egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;

/// The canvas texture format. `Rgba8Unorm` is gamma-space, matching how egui
/// itself stores and outputs its textures.
const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const SHADER: &str = r#"
struct Uniforms {
    // canvas_w, canvas_h, magnification (physical px per canvas px), checker size (physical px)
    canvas_mag: vec4<f32>,
    // cursor origin_x, origin_y, size, shape (0 = square, 1 = circle)
    cursor: vec4<f32>,
    // transparent, grid_enabled, show_cursor, srgb_output
    flags: vec4<f32>,
    // uv min_x, min_y, size_x, size_y of the visible sub-rectangle
    uv_rect: vec4<f32>,
    // grid_opacity, (reserved)
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

// Transparency checkerboard shades and their representative luma (used by the
// pixel grid so its color stays consistent across the checker cells).
const CHECKER_LIGHT: f32 = 0.45;
const CHECKER_DARK: f32 = 0.30;
const CHECKER_LUMA: f32 = 0.375;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Whether local pixel `p` (relative to the cursor origin) is part of the brush
// footprint. Mirrors the CPU brush stamping so the cursor matches exactly.
fn cursor_contains(p: vec2<f32>, size: f32, shape: f32) -> bool {
    if p.x < 0.0 || p.y < 0.0 || p.x >= size || p.y >= size {
        return false;
    }
    if shape < 0.5 {
        return true; // square
    }
    if shape < 1.5 {
        // circle: radius size/2, size 3 special-cased to a plus.
        let c = (size - 1.0) * 0.5;
        let d = p - vec2<f32>(c);
        let r = select(size * 0.5, 1.0, size == 3.0);
        return dot(d, d) <= r * r;
    }
    // diagonal lines; `p` is integer-valued (floor of the pixel coordinate).
    if shape < 2.5 {
        return p.x == p.y;
    }
    return p.x + p.y == size - 1.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let canvas_size = u.canvas_mag.xy;
    let magnification = u.canvas_mag.z;
    let checker_px = u.canvas_mag.w;

    // The callback only covers the visible sub-rectangle of the canvas, so map
    // the local [0,1] uv into the full canvas uv before sampling.
    let uv = u.uv_rect.xy + in.uv * u.uv_rect.zw;
    let canvas_px = uv * canvas_size;

    let texel = textureSample(tex, samp, uv);
    var color = texel.rgb;

    // Transparency checkerboard, composited per-pixel from the alpha so any
    // transparent pixel reads as transparency (regardless of the document's
    // "transparent" flag). Opaque pixels are unaffected.
    let cell = floor(in.pos.xy / checker_px);
    let parity = fract((cell.x + cell.y) * 0.5) * 2.0;
    // Dark enough that the pixel grid (a light line over dark) is visible.
    let checker = mix(CHECKER_LIGHT, CHECKER_DARK, parity);
    color = mix(vec3<f32>(checker), texel.rgb, texel.a);

    // Pixel grid, drawn once a canvas pixel is at least 8 screen px wide.
    // The line contrasts with whatever is underneath: the pixel color, or the
    // transparency checkerboard's representative shade where transparent (a
    // single value, so the line does not flip between checker cells).
    if u.flags.y > 0.5 {
        let f = fract(canvas_px);
        let d = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y)) * magnification;
        let g = 1.0 - smoothstep(0.0, 1.0, d);

        let pixel_luma = dot(texel.rgb, vec3<f32>(0.299, 0.587, 0.114));
        let luma = mix(CHECKER_LUMA, pixel_luma, texel.a);

        // light background -> gray line, mid-gray -> dark line, dark -> light line
        var line = vec3<f32>(0.9);
        if luma > 0.75 {
            line = vec3<f32>(0.5);
        } else if luma > 0.3 {
            line = vec3<f32>(0.1);
        }
        color = mix(color, line, g * u.params.x);
    }

    // Tool cursor: the exact brush footprint (pixel-aligned, matching the
    // stamp), drawn as a contrasting silhouette outline.
    if u.flags.z > 0.5 {
        let origin = u.cursor.xy;
        let size = u.cursor.z;
        let shape = u.cursor.w;
        let local = floor(canvas_px) - origin;
        if cursor_contains(local, size, shape) {
            let f = fract(canvas_px);
            let t = 1.0 / magnification; // one physical pixel in pixel fractions
            var edge = 0.0;
            if !cursor_contains(local - vec2<f32>(1.0, 0.0), size, shape) && f.x < t {
                edge = 1.0;
            }
            if !cursor_contains(local + vec2<f32>(1.0, 0.0), size, shape) && f.x > 1.0 - t {
                edge = 1.0;
            }
            if !cursor_contains(local - vec2<f32>(0.0, 1.0), size, shape) && f.y < t {
                edge = 1.0;
            }
            if !cursor_contains(local + vec2<f32>(0.0, 1.0), size, shape) && f.y > 1.0 - t {
                edge = 1.0;
            }
            if edge > 0.5 {
                let luma = dot(color, vec3<f32>(0.299, 0.587, 0.114));
                let line = select(vec3<f32>(1.0), vec3<f32>(0.0), luma > 0.5);
                color = mix(color, line, 0.85);
            }
        }
    }

    if u.flags.w > 0.5 {
        return vec4<f32>(srgb_to_linear(color), 1.0);
    }
    return vec4<f32>(color, 1.0);
}
"#;

/// Uniform data shared with the canvas shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug)]
pub struct Uniforms {
    pub canvas_mag: [f32; 4],
    pub cursor: [f32; 4],
    pub flags: [f32; 4],
    pub uv_rect: [f32; 4],
    pub params: [f32; 4],
}

pub use rspaint_editor::Upload;

/// Survives discarded egui layout passes until a GPU callback consumes it.
#[derive(Clone, Default)]
pub struct PendingUpload(std::sync::Arc<std::sync::Mutex<Option<Upload>>>);

impl PendingUpload {
    pub fn stage(&self, editor: &mut rspaint_editor::Editor) {
        let mut pending = self.0.lock().expect("canvas upload lock");
        if pending.is_some() {
            // An unrendered update may cover a different region or document size.
            // Replace it with the latest full snapshot rather than losing pixels.
            editor.invalidate_canvas();
        }
        if let Some(upload) = editor.take_pending_upload() {
            *pending = Some(upload);
        }
    }

    fn take(&self) -> Option<Upload> {
        self.0.lock().expect("canvas upload lock").take()
    }
}

/// GPU resources for the canvas, stored in egui-wgpu's callback resources.
///
/// The pipeline, sampler and layout are shared; the texture and uniforms are
/// kept per window (keyed by a window id) so several torn-off windows can each
/// render their own canvas in the same frame.
pub struct CanvasRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    windows: std::collections::HashMap<u64, WindowCanvas>,
}

/// Per-window canvas GPU resources.
struct WindowCanvas {
    uniform_buffer: wgpu::Buffer,
    texture: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
    size: [u32; 2],
}

impl WindowCanvas {
    fn new(device: &wgpu::Device) -> Self {
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rs-paint canvas uniforms"),
            size: std::mem::size_of::<Uniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            uniform_buffer,
            texture: None,
            bind_group: None,
            size: [0, 0],
        }
    }

    /// Creates the canvas texture and bind group if the size changed.
    fn ensure_texture(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: [u32; 2],
    ) {
        if self.texture.is_some() && self.size == size {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rs-paint canvas texture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CANVAS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rs-paint canvas bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        self.texture = Some(texture);
        self.bind_group = Some(bind_group);
        self.size = size;
    }
}

impl std::fmt::Debug for CanvasRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasRenderer")
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

impl CanvasRenderer {
    /// Creates the pipeline and reusable resources for the given target format.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rs-paint canvas shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rs-paint canvas bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rs-paint canvas pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rs-paint canvas pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rs-paint canvas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            windows: std::collections::HashMap::new(),
        }
    }
}

/// The per-frame paint callback for the canvas.
pub struct CanvasCallback {
    /// Which window this canvas belongs to (its own texture/uniforms).
    pub window_id: u64,
    pub uniforms: Uniforms,
    pub upload: PendingUpload,
}

impl CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = resources.get_mut::<CanvasRenderer>() else {
            return Vec::new();
        };
        let CanvasRenderer {
            bind_group_layout,
            sampler,
            windows,
            ..
        } = renderer;
        let window = windows
            .entry(self.window_id)
            .or_insert_with(|| WindowCanvas::new(device));

        let size = [
            self.uniforms.canvas_mag[0].max(1.0) as u32,
            self.uniforms.canvas_mag[1].max(1.0) as u32,
        ];
        window.ensure_texture(device, bind_group_layout, sampler, size);

        if let Some(upload) = self.upload.take() {
            if let Some(texture) = window.texture.as_ref() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: upload.x,
                            y: upload.y,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &upload.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(upload.width * 4),
                        rows_per_image: Some(upload.height),
                    },
                    wgpu::Extent3d {
                        width: upload.width,
                        height: upload.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }

        queue.write_buffer(
            &window.uniform_buffer,
            0,
            bytemuck::bytes_of(&self.uniforms),
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::epaint::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(renderer) = resources.get::<CanvasRenderer>() else {
            return;
        };
        let Some(window) = renderer.windows.get(&self.window_id) else {
            return;
        };
        let Some(bind_group) = &window.bind_group else {
            return;
        };
        render_pass.set_pipeline(&renderer.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspaint_core::{Color, PixelBuffer};
    use rspaint_editor::{Editor, PointerButton};

    #[test]
    fn discarded_layout_pass_preserves_initial_white_upload() {
        let mut editor = Editor::new();
        let pending = PendingUpload::default();
        pending.stage(&mut editor);
        let discarded_callback = pending.clone();
        drop(discarded_callback);
        pending.stage(&mut editor);
        let upload = pending.take().expect("initial image still needs uploading");
        assert_eq!(
            [upload.width, upload.height],
            [editor.document.width(), editor.document.height()]
        );
        assert!(upload
            .data
            .chunks_exact(4)
            .all(|pixel| pixel == [255, 255, 255, 255]));
        assert!(pending.take().is_none());
    }

    #[test]
    fn updates_before_render_replace_pending_snapshot_without_losing_regions() {
        let mut editor = Editor::new();
        editor.load_buffer(PixelBuffer::new(16, 16, Color::WHITE), None);
        let pending = PendingUpload::default();
        pending.stage(&mut editor);
        editor.begin_stroke((8, 8), PointerButton::Primary);
        editor.end_stroke(false);
        pending.stage(&mut editor);
        let upload = pending.take().unwrap();
        assert_eq!([upload.width, upload.height], [16, 16]);
        assert_eq!(upload.data.as_slice(), editor.document.buffer.data());
        // After the backend consumes the snapshot, unchanged frames send nothing.
        pending.stage(&mut editor);
        assert!(pending.take().is_none());
    }
}
