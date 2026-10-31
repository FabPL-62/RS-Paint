//! Reproducible editor sessions, not native input-to-display latency.
//! cargo run --release -p rspaint-editor --example interaction_session_bench --offline
use rspaint_core::{
    layer::{composite_region, Region},
    Color, Layer, LayerBlend, LayerGroup, LayerNode, PixelBuffer,
};
use rspaint_editor::{tools::Tool, Editor, PointerButton};
use std::time::Instant;

fn scene(w: u32, h: u32, kind: &str) -> Editor {
    let mixed = std::env::args().any(|arg| arg == "--mixed");
    let mut e = Editor::new();
    e.load_buffer(PixelBuffer::new(w, h, Color::WHITE), None);
    if kind != "single" {
        let mut source = PixelBuffer::transparent(w, h);
        let mut seed = 0x12345678u32;
        for p in source.data_mut().chunks_exact_mut(4) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            p.copy_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 140]);
        }
        e.document.layers.splice(
            0..0,
            (100..107).map(|id| {
                let mut layer = Layer::new(id, "Texture", source.clone());
                if mixed {
                    layer.blend = [
                        LayerBlend::Normal,
                        LayerBlend::Multiply,
                        LayerBlend::Screen,
                        LayerBlend::Overlay,
                    ][(id - 100) as usize % 4];
                }
                LayerNode::Layer(layer)
            }),
        );
        e.document.next_id = 108;
        let active = if kind == "below" { 106 } else { 100 };
        // Keep static sources shared to bound fixture RAM, but give the active
        // layer its own storage so first-write COW is not a fixture artifact.
        rspaint_core::layer::find_node_mut(&mut e.document.layers, active)
            .unwrap()
            .layer_mut()
            .unwrap()
            .pixels
            .data_mut();
        e.select_layer(active, false, false);
        if kind == "nested" {
            let mut inner = LayerGroup::new(108, "Inner");
            inner.opacity = 73;
            inner.children = std::mem::take(&mut e.document.layers);
            let mut outer = LayerGroup::new(109, "Outer");
            outer.opacity = 81;
            outer.children.push(LayerNode::Group(inner));
            e.document.layers.push(LayerNode::Group(outer));
            e.document.next_id = 110;
        }
        e.commit("Session fixture");
        e.invalidate_canvas();
    }
    drop(e.take_pending_upload());
    e
}

#[derive(Default)]
struct Samples {
    action: Vec<f64>,
    compose_stage: Vec<f64>,
    total: Vec<f64>,
    max_upload: usize,
}
impl Samples {
    fn event(&mut self, e: &mut Editor, action: impl FnOnce(&mut Editor)) {
        let start = Instant::now();
        action(e);
        let action_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start_stage = Instant::now();
        let upload = e.take_pending_upload();
        let stage_ms = start_stage.elapsed().as_secs_f64() * 1000.0;
        self.max_upload = self
            .max_upload
            .max(upload.as_ref().map_or(0, |u| u.data.len()));
        self.action.push(action_ms);
        self.compose_stage.push(stage_ms);
        self.total.push(action_ms + stage_ms);
        // The renderer consumes each upload before the following event.
        drop(upload);
    }
    fn report(mut self, label: &str) {
        self.action.sort_by(f64::total_cmp);
        self.compose_stage.sort_by(f64::total_cmp);
        self.total.sort_by(f64::total_cmp);
        let n = self.total.len();
        let p95 = (n * 95).div_ceil(100) - 1;
        println!("  {label}: n={n} total p50={:.3}ms p95={:.3}ms max={:.3}ms; action p95={:.3}ms compose/stage p95={:.3}ms; >16.7ms={} >50ms={}; max_upload={}B", self.total[n/2], self.total[p95], self.total[n-1], self.action[p95], self.compose_stage[p95], self.total.iter().filter(|&&v| v>16.7).count(), self.total.iter().filter(|&&v| v>50.0).count(), self.max_upload);
    }
}

fn hash(pixels: &PixelBuffer) -> u64 {
    pixels.data().iter().fold(0xcbf29ce484222325, |acc, &b| {
        (acc ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn session(w: u32, h: u32, kind: &str) {
    let mut e = scene(w, h, kind);
    println!("{w}x{h} {kind} (8 layers except single; static storage shared, active independent):");
    for (tool, size, smoothing) in [(Tool::Pencil, 1, 0), (Tool::Brush, 256, 50)] {
        e.set_tool(tool);
        e.settings.stroke_size = size;
        e.settings.brush_size = size;
        e.settings.stroke_smoothing = smoothing;
        let mut start = Samples::default();
        start.event(&mut e, |e| {
            e.begin_stroke((300, 300), PointerButton::Primary)
        });
        start.report(&format!("{tool:?} down"));
        let mut moves = Samples::default();
        for i in 1..=32 {
            moves.event(&mut e, |e| {
                e.update_stroke((300 + i * 8, 300 + i % 7 * 3), false)
            });
        }
        moves.report(&format!("{tool:?} moves size={size} smoothing={smoothing}"));
        let mut end = Samples::default();
        end.event(&mut e, |e| e.end_stroke_at((564, 330), false));
        end.report(&format!("{tool:?} release/commit"));
    }
    e.set_tool(Tool::Curve);
    e.settings.stroke_size = 3;
    let before_curve = hash(&e.document.buffer);
    e.curve_pointer_down((200, 200));
    e.curve_pointer_move((800, 500));
    e.curve_pointer_up();
    e.curve_pointer_down((400, 600));
    drop(e.take_pending_upload());
    let mut curve = Samples::default();
    for i in 0..24 {
        curve.event(&mut e, |e| e.curve_pointer_move((400 + i * 2, 600 + i % 5)));
    }
    curve.report("curve control moves");
    // Verify the displayed preview before cancellation could conceal a
    // composition error. Reference work is outside the measured events.
    let mut preview = PixelBuffer::transparent(w, h);
    composite_region(
        &e.document.layers,
        e.document.active,
        &e.document.buffer,
        Region::new(0, 0, w, h),
        &mut preview,
    );
    assert_eq!(e.document.composite, preview, "curve preview {kind}");
    drop(preview);
    let mut cancel = Samples::default();
    cancel.event(&mut e, Editor::curve_cancel);
    cancel.report("curve cancel");
    assert_eq!(
        hash(&e.document.buffer),
        before_curve,
        "curve cancellation {kind}"
    );
    let active = e.document.active;
    if kind != "single" {
        let other = if active == 100 { 106 } else { 100 };
        let mut selection = Samples::default();
        for _ in 0..3 {
            selection.event(&mut e, |e| e.select_layer(other, false, false));
            selection.event(&mut e, |e| e.select_layer(active, false, false));
        }
        selection.report("layer selection");
    }
    let mut opacity = Samples::default();
    for value in 71..=77 {
        opacity.event(&mut e, |e| e.set_layer_opacity(active, value));
    }
    opacity.report("opacity drag");
    e.commit("Opacity");
    let mut visibility = Samples::default();
    for _ in 0..6 {
        visibility.event(&mut e, |e| e.toggle_layer_visible(active));
    }
    visibility.report("visibility toggles");
    let expected_pixels = hash(&e.document.buffer);
    let expected_composite = hash(&e.document.composite);
    let mut undo = Samples::default();
    let mut redo = Samples::default();
    for _ in 0..3 {
        undo.event(&mut e, Editor::undo);
        redo.event(&mut e, Editor::redo);
        assert_eq!(
            hash(&e.document.buffer),
            expected_pixels,
            "redo pixels {kind}"
        );
        assert_eq!(
            hash(&e.document.composite),
            expected_composite,
            "redo composite {kind}"
        );
    }
    undo.report("undo");
    redo.report("redo");
    let mut expected = PixelBuffer::transparent(w, h);
    composite_region(
        &e.document.layers,
        e.document.active,
        &e.document.buffer,
        Region::new(0, 0, w, h),
        &mut expected,
    );
    assert_eq!(e.document.composite, expected, "final reference {kind}");
    println!("  checks: curve cancel, undo/redo and full reference passed");
}

fn main() {
    if std::env::args().any(|arg| arg == "--mixed") {
        println!("Mixed modes: Normal / Multiply / Screen / Overlay");
    }
    for (w, h) in [(1920, 1080), (6000, 4000)] {
        for kind in ["single", "top", "below", "nested"] {
            session(w, h, kind);
        }
    }
}
