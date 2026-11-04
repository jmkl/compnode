//! The egui application: graph editor, canvas viewer, menus and overlays.
use crate::Args;
use crate::backend::{Backend, Output};
use crate::paint::{self, Xf};
use compnode::Rect as GRect;
use compnode::geom::vec2;
use compnode_image::imageio::ImageCrateLoader;
use compnode_image::loader::{AsyncLoader, ExportFormat, ImageLoader, StoreLoader, export};
use compnode_image::thumbs::{ThumbCache, ThumbState};
use compnode_ui::canvas::CanvasView;
use compnode_ui::draw::DrawList;
use compnode_ui::editor::focus::AccessRole;
use compnode_ui::editor::{DrawKey, Editor, Interaction, MenuConnect};
use compnode_ui::input::{InputEvent, Key, Modifiers, PointerButton};
use compnode_ui::minimap::Minimap;
use compnode_ui::{EditorAction, ImageHandle, Theme, Value, project};
use eframe::egui;
use egui::{Color32, TextureId, pos2};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Draw-list handle of the canvas result (shown by Output node previews).
const CANVAS_HANDLE: u64 = u64::MAX;
const THUMB_SIZE: u32 = 192;

/// Nerd Font (private-use) glyphs for the GUI icons. Codepoints are from the
/// Material Design Icons set (`nf-md-*`) that 3270 Nerd Font carries.
const ICON_SELECT: &str = "\u{f0481}"; // nf-md-cursor_default
const ICON_ADD: &str = "\u{f0415}"; // nf-md-plus
const ICON_DUPLICATE: &str = "\u{f018f}"; // nf-md-content_copy
const ICON_FRAME: &str = "\u{f0f9c}"; // nf-md-arrow_expand
const ICON_VIEW_ALL: &str = "\u{f0ac9}"; // nf-md-fit_to_page
const ICON_MUTE: &str = "\u{f058f}"; // nf-md-eye_off
const ICON_CUT: &str = "\u{f0d79}"; // nf-md-content_cut
const ICON_UNDO: &str = "\u{f054c}"; // nf-md-undo
const ICON_REDO: &str = "\u{f044e}"; // nf-md-redo
const ICON_DELETE: &str = "\u{f0a79}"; // nf-md-trash_can

/// Font for a GUI icon: the bundled Nerd Font, by family name.
fn icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
}

struct ShapeCache {
    key: (DrawKey, u32, u32, u32),
    origin: compnode::Vec2,
    shapes: Vec<egui::Shape>,
}

struct TextEditUi {
    buf: String,
    first: bool,
}

struct MenuUi {
    screen: egui::Pos2,
    graph_pos: compnode::Vec2,
    connect: Option<MenuConnect>,
    query: String,
    first: bool,
}

#[allow(dead_code)]
struct ContextUi {
    screen: egui::Pos2,
    graph_pos: compnode::Vec2,
    context: Option<u64>,
    first: bool,
    /// Currently highlighted menu row (keyboard navigation / hover).
    highlight: usize,
}

/// An action offered by the right-click context menu.
#[derive(Clone)]
enum ContextAction {
    Add { schema: String, pos: compnode::Vec2 },
    AddHere { pos: compnode::Vec2 },
    Duplicate,
    Delete,
    Mute,
    Frame,
}

struct Bench {
    target: usize,
    samples: Vec<f32>,
    nodes: usize,
}

#[allow(dead_code)]
enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Export(ExportFormat),
    LoadTheme,
    SaveTheme,
    Dark(bool),
    FitCanvas,
    OneToOne,
    ViewAll,
    Undo,
    Redo,
    Canvas(u32),
    UseGpu(bool),
}

pub struct App {
    ed: Editor,
    backend: Backend,
    store: StoreLoader,
    loads: AsyncLoader,
    load_ids: HashMap<u64, String>,
    requested: HashSet<String>,
    next_load: u64,
    thumbs: ThumbCache,
    canvas_px: u32,
    canvas: CanvasView,
    canvas_dims: (u32, u32),
    canvas_tex: Option<egui::TextureHandle>,
    textures: HashMap<u64, egui::TextureHandle>,
    path_handles: HashMap<String, u64>,
    next_handle: u64,
    preview_set: HashMap<u64, (u64, u32)>,
    #[cfg(feature = "gpu")]
    wgpu_state: Option<eframe::egui_wgpu::RenderState>,
    #[cfg(feature = "gpu")]
    gpu_display: Option<compnode_image::gpu::GpuImage>,
    #[cfg(feature = "gpu")]
    gpu_tex: Option<TextureId>,
    cache: Option<ShapeCache>,
    graph_rect: egui::Rect,
    show_minimap: bool,
    minimap_drag: bool,
    text_edit: Option<TextEditUi>,
    menu: Option<MenuUi>,
    context_menu: Option<ContextUi>,
    eval_dirty: bool,
    preview_selected: bool,
    frame_ms: Vec<f32>,
    eval_ms: f32,
    status: String,
    project_path: Option<PathBuf>,
    bench: Option<Bench>,
    dark: bool,
    profile: bool,
}

fn map_key(k: egui::Key) -> Option<Key> {
    use egui::Key as E;
    Some(match k {
        E::A => Key::A,
        E::C => Key::C,
        E::D => Key::D,
        E::F => Key::F,
        E::H => Key::H,
        E::I => Key::I,
        E::M => Key::M,
        E::R => Key::R,
        E::V => Key::V,
        E::X => Key::X,
        E::Y => Key::Y,
        E::Z => Key::Z,
        E::Delete => Key::Delete,
        E::Backspace => Key::Backspace,
        E::Escape => Key::Escape,
        E::Enter => Key::Enter,
        E::Space => Key::Space,
        E::Tab => Key::Tab,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::Home => Key::Home,
        E::End => Key::End,
        E::Period => Key::Period,
        _ => return None,
    })
}

fn map_mods(m: &egui::Modifiers) -> Modifiers {
    Modifiers { shift: m.shift, ctrl: m.ctrl || m.command, alt: m.alt }
}

fn map_button(b: egui::PointerButton) -> Option<PointerButton> {
    match b {
        egui::PointerButton::Primary => Some(PointerButton::Primary),
        egui::PointerButton::Secondary => Some(PointerButton::Secondary),
        egui::PointerButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

fn color_image(img: &compnode_image::ImageBuf) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_srgb8_rgba())
}

fn format_for(path: &std::path::Path) -> Option<ExportFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some(ExportFormat::Png),
        "jpg" | "jpeg" => Some(ExportFormat::Jpeg),
        "webp" => Some(ExportFormat::WebP),
        "ppm" => Some(ExportFormat::Ppm),
        _ => None,
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: &Args) -> App {
        let store = StoreLoader::new();
        #[cfg(feature = "gpu")]
        let wgpu_state = if args.no_gpu { None } else { cc.wgpu_render_state.clone() };
        #[cfg(feature = "gpu")]
        let use_gpu = wgpu_state.is_some();
        #[cfg(not(feature = "gpu"))]
        let use_gpu = false;
        let canvas_px = args.canvas.unwrap_or(if use_gpu { 2048 } else { 512 });

        #[cfg(feature = "gpu")]
        if args.selftest_gpu {
            std::process::exit(selftest_gpu(wgpu_state.as_ref()));
        }

        #[cfg(feature = "gpu")]
        let backend = match &wgpu_state {
            Some(s) => Backend::gpu(s, canvas_px, &store),
            None => Backend::cpu(canvas_px, &store),
        };
        #[cfg(not(feature = "gpu"))]
        let backend = Backend::cpu(canvas_px, &store);

        let loader: Arc<dyn ImageLoader> = Arc::new(ImageCrateLoader);
        let mut ed = Editor::new(compnode_image::registry());
        let mut project_path = None;
        let mut status = format!("{} backend, {canvas_px}x{canvas_px} canvas", backend.name());
        if let Some(p) = &args.project {
            match std::fs::read_to_string(p)
                .map_err(|e| e.to_string())
                .and_then(|t| project::load(&t, compnode_image::registry()))
            {
                Ok(doc) => {
                    ed.replace_document(doc);
                    project_path = Some(p.clone());
                }
                Err(e) => status = format!("could not open {}: {e}", p.display()),
            }
        }
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        setup_fonts(&cc.egui_ctx);
        let bench = args.bench.map(|n| Bench { target: 240, samples: Vec::new(), nodes: n });
        let mut app = App {
            ed,
            backend,
            store,
            loads: AsyncLoader::new(Arc::clone(&loader), 2, Some((canvas_px, canvas_px))),
            load_ids: HashMap::new(),
            requested: HashSet::new(),
            next_load: 1,
            thumbs: ThumbCache::new(loader, 2, THUMB_SIZE),
            canvas_px,
            canvas: CanvasView::new(vec2(500.0, 400.0)),
            canvas_dims: (canvas_px, canvas_px),
            canvas_tex: None,
            textures: HashMap::new(),
            path_handles: HashMap::new(),
            next_handle: 1,
            preview_set: HashMap::new(),
            #[cfg(feature = "gpu")]
            wgpu_state,
            #[cfg(feature = "gpu")]
            gpu_display: None,
            #[cfg(feature = "gpu")]
            gpu_tex: None,
            cache: None,
            graph_rect: egui::Rect::NOTHING,
            show_minimap: true,
            minimap_drag: false,
            text_edit: None,
            menu: None,
            context_menu: None,
            eval_dirty: true,
            preview_selected: false,
            frame_ms: Vec::new(),
            eval_ms: 0.0,
            status,
            project_path,
            bench,
            dark: true,
            profile: false,
        };
        app.canvas.set_canvas(canvas_px, canvas_px);
        if let Some(n) = app.bench.as_ref().map(|b| b.nodes) {
            app.build_bench_graph(n);
        }
        let all = app.ed.doc.uids();
        app.backend.apply_changes(&app.ed.doc, &[compnode_ui::Change::Structure { uids: all }]);
        app
    }

    // ───────────── helpers ─────────────

    fn to_screen(&self, p: compnode::Vec2) -> egui::Pos2 {
        let s = self.ed.view.to_screen(p);
        pos2(self.graph_rect.min.x + s.x, self.graph_rect.min.y + s.y)
    }

    fn output_target(&self) -> Option<u64> {
        if self.preview_selected
            && let Some(u) = self.ed.selection.active.filter(|u| self.ed.doc.contains(*u))
        {
            return Some(u);
        }
        self.ed.doc.uids().into_iter().find(|u| self.ed.doc.data(*u).is_some_and(|d| d.schema == "image.output"))
    }

    fn texture_ids(&self) -> HashMap<u64, TextureId> {
        let mut m: HashMap<u64, TextureId> = self.textures.iter().map(|(k, t)| (*k, t.id())).collect();
        #[cfg(feature = "gpu")]
        if let Some(id) = self.gpu_tex {
            m.insert(CANVAS_HANDLE, id);
        }
        if let Some(t) = &self.canvas_tex {
            m.entry(CANVAS_HANDLE).or_insert_with(|| t.id());
        }
        m
    }

    fn build_bench_graph(&mut self, n: usize) {
        let kinds = ["image.invert", "image.blur", "image.hue_sat", "image.brightness_contrast"];
        let cols = (n as f32).sqrt().ceil() as usize;
        let mut prev: Option<u64> = None;
        for i in 0..n {
            let (col, row) = (i / 10, i % 10);
            let pos = vec2(col as f32 * 260.0, row as f32 * 140.0);
            let uid = if row == 0 {
                self.ed.add_node("image.solid", pos)
            } else {
                self.ed.add_node(kinds[i % kinds.len()], pos)
            };
            if let (Some(a), Some(b)) = (prev.filter(|_| row != 0), uid) {
                self.ed.connect((a, 0), (b, 0));
            }
            prev = uid;
        }
        let _ = cols;
        self.ed.selection.clear();
        self.ed.view_all();
    }

    // ───────────── loading and thumbnails ─────────────

    fn image_file_paths(&self) -> Vec<(u64, String)> {
        self.ed
            .doc
            .uids()
            .into_iter()
            .filter_map(|u| {
                let d = self.ed.doc.data(u)?;
                if d.schema != "image.file" {
                    return None;
                }
                match d.values.first() {
                    Some(Value::String(s)) if !s.trim().is_empty() => Some((u, s.trim().to_string())),
                    _ => None,
                }
            })
            .collect()
    }

    fn ensure_loads(&mut self) {
        for (_, path) in self.image_file_paths() {
            if !self.store.contains(&path) && self.requested.insert(path.clone()) {
                let id = self.next_load;
                self.next_load += 1;
                self.load_ids.insert(id, path.clone());
                self.loads.submit(id, &path);
            }
        }
    }

    fn poll_loads(&mut self) {
        for r in self.loads.poll() {
            let path = self.load_ids.remove(&r.id).unwrap_or(r.path);
            match r.result {
                Ok(img) => self.store.put(&path, img),
                Err(e) => self.store.fail(&path, &e),
            }
            self.backend.invalidate_file(&self.ed.doc, &path);
            self.eval_dirty = true;
        }
    }

    fn pump_thumbs(&mut self, ctx: &egui::Context) {
        for uid in self.ed.visible_uids() {
            let Some(d) = self.ed.doc.data(uid) else { continue };
            if d.schema != "image.file" {
                continue;
            }
            let Some(Value::String(path)) = d.values.first() else { continue };
            let path = path.trim().to_string();
            if path.is_empty() {
                continue;
            }
            self.thumbs.request(&path);
            if let Some(ThumbState::Ready(img)) = self.thumbs.get(&path) {
                let handle = *self.path_handles.entry(path.clone()).or_insert_with(|| {
                    self.next_handle += 1;
                    self.next_handle
                });
                let aspect = img.height as f32 / img.width.max(1) as f32;
                self.textures.entry(handle).or_insert_with(|| {
                    ctx.load_texture(format!("thumb-{handle}"), color_image(img), egui::TextureOptions::LINEAR)
                });
                let marker = (handle, aspect.to_bits());
                if self.preview_set.get(&uid) != Some(&marker) {
                    self.ed.set_preview(uid, 1, Some(ImageHandle(handle)), aspect);
                    self.preview_set.insert(uid, marker);
                }
            }
        }
        if !self.thumbs.poll().is_empty() {
            ctx.request_repaint();
        }
        if self.thumbs.pending() > 0 || !self.load_ids.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    // ───────────── evaluation ─────────────

    fn run_eval(&mut self, ctx: &egui::Context) {
        self.eval_dirty = false;
        self.ensure_loads();
        let Some(target) = self.output_target() else {
            self.canvas_tex = None;
            return;
        };
        let t0 = Instant::now();
        let res = self.backend.evaluate(&self.ed.doc, target);
        self.eval_ms = t0.elapsed().as_secs_f32() * 1000.0;
        for uid in self.ed.doc.uids() {
            let err = self.backend.error(uid);
            if err.as_deref() != self.ed.errors.get(&uid).map(String::as_str) {
                self.ed.set_node_error(uid, err);
            }
        }
        match res {
            Ok(out) => self.show_output(ctx, out),
            Err(e) => self.status = e,
        }
        ctx.request_repaint();
    }

    fn show_output(&mut self, ctx: &egui::Context, out: Output) {
        let dims = out.size();
        match out {
            Output::Cpu(img) => {
                let ci = color_image(&img);
                match &mut self.canvas_tex {
                    Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                    None => self.canvas_tex = Some(ctx.load_texture("canvas", ci, egui::TextureOptions::LINEAR)),
                }
            }
            #[cfg(feature = "gpu")]
            Output::Gpu(img) => {
                let (Backend::Gpu(e), Some(state)) = (&mut self.backend, &self.wgpu_state) else { return };
                let disp = e.ctx.to_display(&img);
                if let Some(old) = self.gpu_display.take() {
                    e.ctx.release(old);
                }
                let mut r = state.renderer.write();
                match self.gpu_tex {
                    Some(id) => r.update_egui_texture_from_wgpu_texture(
                        &state.device,
                        &disp.view,
                        eframe::wgpu::FilterMode::Linear,
                        id,
                    ),
                    None => {
                        self.gpu_tex =
                            Some(r.register_native_texture(&state.device, &disp.view, eframe::wgpu::FilterMode::Linear))
                    }
                }
                drop(r);
                self.gpu_display = Some(disp);
            }
        }
        if dims != self.canvas_dims {
            self.canvas_dims = dims;
            self.canvas.set_canvas(dims.0, dims.1);
            self.canvas.fit();
        }
        let aspect = dims.1 as f32 / dims.0.max(1) as f32;
        let outputs: Vec<u64> = self
            .ed
            .doc
            .uids()
            .into_iter()
            .filter(|u| self.ed.doc.data(*u).is_some_and(|d| d.schema == "image.output"))
            .collect();
        for uid in outputs {
            let marker = (CANVAS_HANDLE, aspect.to_bits());
            if self.preview_set.get(&uid) != Some(&marker) {
                self.ed.set_preview(uid, 1, Some(ImageHandle(CANVAS_HANDLE)), aspect);
                self.preview_set.insert(uid, marker);
            }
        }
    }

    fn change_canvas(&mut self, px: u32) {
        self.canvas_px = px;
        self.backend.set_canvas(px);
        self.store.clear();
        self.requested.clear();
        self.load_ids.clear();
        let loader: Arc<dyn ImageLoader> = Arc::new(ImageCrateLoader);
        self.loads = AsyncLoader::new(loader, 2, Some((px, px)));
        self.eval_dirty = true;
        self.status = format!("canvas {px}x{px}");
    }

    // ───────────── file operations ─────────────

    fn new_project(&mut self) {
        self.ed.replace_document(compnode_ui::Document::new(compnode_image::registry()));
        self.after_document_replaced();
        self.project_path = None;
    }

    fn after_document_replaced(&mut self) {
        self.backend.set_canvas(self.canvas_px);
        self.preview_set.clear();
        self.eval_dirty = true;
        self.cache = None;
    }

    fn open_project(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| project::load(&t, compnode_image::registry()))
        {
            Ok(doc) => {
                self.ed.replace_document(doc);
                self.after_document_replaced();
                self.status = format!("opened {}", path.display());
                self.project_path = Some(path);
            }
            Err(e) => self.status = format!("could not open {}: {e}", path.display()),
        }
    }

    fn save_project(&mut self, as_new: bool) {
        let path = match (&self.project_path, as_new) {
            (Some(p), false) => Some(p.clone()),
            _ => {
                rfd::FileDialog::new().add_filter("compnode project", &["cnp"]).set_file_name("project.cnp").save_file()
            }
        };
        let Some(path) = path else { return };
        match std::fs::write(&path, project::save(&self.ed.doc)) {
            Ok(()) => {
                self.status = format!("saved {}", path.display());
                self.project_path = Some(path);
            }
            Err(e) => self.status = format!("could not save: {e}"),
        }
    }

    fn export_result(&mut self, format: ExportFormat) {
        let Some(target) = self.output_target() else {
            self.status = "nothing to export: add an Output node".into();
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("image", &[format.extension()])
            .set_file_name(format!("result.{}", format.extension()))
            .save_file()
        else {
            return;
        };
        let result =
            self.backend.evaluate(&self.ed.doc, target).and_then(|out| self.backend.read_cpu(&out)).and_then(|img| {
                let format = format_for(&path).unwrap_or(format);
                export(&img, &path.to_string_lossy(), format)
            });
        self.status = match result {
            Ok(()) => format!("exported {}", path.display()),
            Err(e) => format!("export failed: {e}"),
        };
    }

    fn load_theme(&mut self, ctx: &egui::Context) {
        let Some(p) = rfd::FileDialog::new().add_filter("theme", &["txt", "theme"]).pick_file() else { return };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Theme::from_text(&t)) {
            Ok(t) => {
                self.ed.set_theme(t);
                self.cache = None;
                ctx.request_repaint();
                self.status = format!("theme {}", p.display());
            }
            Err(e) => self.status = format!("theme error: {e}"),
        }
    }

    fn save_theme(&mut self) {
        let Some(p) = rfd::FileDialog::new().set_file_name("theme.txt").save_file() else { return };
        self.status = match std::fs::write(&p, self.ed.theme.to_text()) {
            Ok(()) => format!("saved theme {}", p.display()),
            Err(e) => format!("could not save theme: {e}"),
        };
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::New => self.new_project(),
            Cmd::Open => {
                if let Some(p) = rfd::FileDialog::new().add_filter("compnode project", &["cnp"]).pick_file() {
                    self.open_project(p);
                }
            }
            Cmd::Save => self.save_project(false),
            Cmd::SaveAs => self.save_project(true),
            Cmd::Export(f) => self.export_result(f),
            Cmd::LoadTheme => self.load_theme(ctx),
            Cmd::SaveTheme => self.save_theme(),
            Cmd::Dark(d) => {
                self.dark = d;
                ctx.set_visuals(if d { egui::Visuals::dark() } else { egui::Visuals::light() });
                self.ed.set_theme(if d { Theme::blender_dark() } else { Theme::light() });
                self.cache = None;
            }
            Cmd::FitCanvas => self.canvas.fit(),
            Cmd::OneToOne => self.canvas.one_to_one(),
            Cmd::ViewAll => self.ed.view_all(),
            Cmd::Undo => {
                self.ed.undo();
            }
            Cmd::Redo => {
                self.ed.redo();
            }
            Cmd::Canvas(px) => self.change_canvas(px),
            Cmd::UseGpu(on) => self.switch_backend(on),
        }
    }

    fn switch_backend(&mut self, on: bool) {
        #[cfg(feature = "gpu")]
        {
            self.backend = match (&self.wgpu_state, on) {
                (Some(s), true) => Backend::gpu(s, self.canvas_px, &self.store),
                _ => Backend::cpu(self.canvas_px, &self.store),
            };
            self.gpu_display = None;
            self.preview_set.clear();
            let all = self.ed.doc.uids();
            self.backend.apply_changes(&self.ed.doc, &[compnode_ui::Change::Structure { uids: all }]);
            self.backend.set_profile(self.profile);
            self.eval_dirty = true;
            self.status = format!("{} backend", self.backend.name());
        }
        #[cfg(not(feature = "gpu"))]
        let _ = on;
    }

    // ───────────── input ─────────────

    fn feed_graph_input(&mut self, ctx: &egui::Context, rect: egui::Rect, layer: egui::LayerId) {
        let events = ctx.input(|i| i.events.clone());
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let typing = ctx.egui_wants_keyboard_input();
        let over_graph = |p: egui::Pos2| rect.contains(p) && ctx.layer_id_at(p) == Some(layer);
        let local = |p: egui::Pos2| vec2(p.x - rect.min.x, p.y - rect.min.y);
        let mm = self.minimap_rect();
        for ev in events {
            let busy = *self.ed.interaction() != Interaction::Idle;
            match ev {
                egui::Event::PointerMoved(p) => {
                    if self.minimap_drag {
                        let mut m = Minimap::new(mm);
                        m.update(&self.ed.doc, &self.ed.view);
                        self.ed.minimap_click(&m, local(p));
                    } else if busy || over_graph(p) {
                        self.ed.handle_event(&InputEvent::PointerMove { pos: local(p), mods: Modifiers::NONE });
                    }
                }
                egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                    let Some(button) = map_button(button) else { continue };
                    let mods = map_mods(&modifiers);
                    if pressed {
                        if self.show_minimap && over_graph(pos) && mm_contains(mm, local(pos)) && !busy {
                            self.minimap_drag = true;
                            let mut m = Minimap::new(mm);
                            m.update(&self.ed.doc, &self.ed.view);
                            self.ed.minimap_click(&m, local(pos));
                        } else if over_graph(pos) {
                            self.ed.handle_event(&InputEvent::PointerDown { pos: local(pos), button, mods });
                        }
                    } else if self.minimap_drag {
                        self.minimap_drag = false;
                    } else if busy {
                        self.ed.handle_event(&InputEvent::PointerUp { pos: local(pos), button, mods });
                    }
                }
                egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                    if let Some(p) = hover.filter(|p| over_graph(*p)) {
                        let lines = match unit {
                            egui::MouseWheelUnit::Point => delta.y / 50.0,
                            egui::MouseWheelUnit::Line => delta.y,
                            egui::MouseWheelUnit::Page => delta.y * 10.0,
                        };
                        self.ed.handle_event(&InputEvent::Scroll {
                            pos: local(p),
                            delta: vec2(0.0, lines),
                            mods: map_mods(&modifiers),
                        });
                    }
                }
                egui::Event::Zoom(f) => {
                    if let Some(p) = hover.filter(|p| over_graph(*p)) {
                        let lines = f.ln() / 1.1_f32.ln();
                        self.ed.handle_event(&InputEvent::Scroll {
                            pos: local(p),
                            delta: vec2(0.0, lines),
                            mods: Modifiers::NONE,
                        });
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if !typing && (busy || hover.is_some_and(over_graph)) =>
                {
                    if let Some(k) = map_key(key) {
                        self.ed.handle_event(&InputEvent::Key { key: k, mods: map_mods(&modifiers) });
                    }
                }
                // egui turns Ctrl+C/X/V into these events instead of key presses
                egui::Event::Copy if !typing => {
                    self.ed.handle_event(&InputEvent::Key { key: Key::C, mods: Modifiers::CTRL })
                }
                egui::Event::Cut if !typing => {
                    self.ed.handle_event(&InputEvent::Key { key: Key::C, mods: Modifiers::CTRL });
                    self.ed.handle_event(&InputEvent::Key { key: Key::Delete, mods: Modifiers::NONE });
                }
                egui::Event::Paste(_) if !typing => {
                    self.ed.handle_event(&InputEvent::Key { key: Key::V, mods: Modifiers::CTRL })
                }
                _ => {}
            }
        }
    }

    fn minimap_rect(&self) -> GRect {
        let size = vec2(200.0, 140.0);
        let br = vec2(self.graph_rect.width() - 12.0, self.graph_rect.height() - 12.0);
        GRect::from_min_size(br - size, size)
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let at = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.graph_rect.contains(*p));
        let mut gp = match at {
            Some(p) => self.ed.view.to_graph(vec2(p.x - self.graph_rect.min.x, p.y - self.graph_rect.min.y)),
            None => self.ed.view.to_graph(self.ed.view.size * 0.5),
        };
        for f in dropped {
            let path = f.path().to_path_buf();
            if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cnp")) {
                self.open_project(path);
                return;
            }
            if let Some(uid) = self.ed.add_node("image.file", gp) {
                self.ed.set_value(uid, 0, Value::String(path.to_string_lossy().into_owned()));
                gp += vec2(0.0, 80.0);
            }
        }
    }

    // ───────────── frame ─────────────

    fn pump_actions(&mut self, ctx: &egui::Context) {
        for a in self.ed.drain_actions() {
            match a {
                EditorAction::OpenAddMenu { graph_pos, connect } => {
                    let s = self.to_screen(graph_pos);
                    self.menu = Some(MenuUi { screen: s, graph_pos, connect, query: String::new(), first: true });
                }
                EditorAction::OpenContextMenu { graph_pos, context } => {
                    let s = self.to_screen(graph_pos);
                    self.context_menu = Some(ContextUi { screen: s, graph_pos, context, first: true, highlight: 0 });
                }
                EditorAction::ButtonPressed { uid, input } => {
                    let is_browse = self.ed.doc.schema_of(uid).is_some_and(|s| s.id == "image.file" && input == 1);
                    if is_browse
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("images", &["png", "jpg", "jpeg", "webp", "ppm"])
                            .pick_file()
                    {
                        self.ed.set_value(uid, 0, Value::String(p.to_string_lossy().into_owned()));
                    }
                }
                EditorAction::TextEdit { .. } => {}
            }
        }
        let changes = self.ed.doc.take_changes();
        if !changes.is_empty() {
            self.backend.apply_changes(&self.ed.doc, &changes);
            self.eval_dirty = true;
        }
        self.poll_loads();
        if self.eval_dirty {
            self.run_eval(ctx);
        }
        self.pump_thumbs(ctx);
    }

    fn top_bar(&mut self, root: &mut egui::Ui) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let mut item = |ui: &mut egui::Ui, label: &str, c: Cmd| {
                        if ui.button(label).clicked() {
                            cmds.push(c);
                            ui.close();
                        }
                    };
                    item(ui, "New", Cmd::New);
                    item(ui, "Open project...", Cmd::Open);
                    item(ui, "Save", Cmd::Save);
                    item(ui, "Save as...", Cmd::SaveAs);
                    ui.separator();
                    item(ui, "Export PNG...", Cmd::Export(ExportFormat::Png));
                    item(ui, "Export JPEG...", Cmd::Export(ExportFormat::Jpeg));
                    item(ui, "Export WebP...", Cmd::Export(ExportFormat::WebP));
                    ui.separator();
                    item(ui, "Load theme...", Cmd::LoadTheme);
                    item(ui, "Save theme...", Cmd::SaveTheme);
                });
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(self.ed.can_undo(), egui::Button::new("Undo")).clicked() {
                        cmds.push(Cmd::Undo);
                        ui.close();
                    }
                    if ui.add_enabled(self.ed.can_redo(), egui::Button::new("Redo")).clicked() {
                        cmds.push(Cmd::Redo);
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Show all nodes").clicked() {
                        cmds.push(Cmd::ViewAll);
                        ui.close();
                    }
                    if ui.button("Fit canvas").clicked() {
                        cmds.push(Cmd::FitCanvas);
                        ui.close();
                    }
                    if ui.button("Canvas 1:1").clicked() {
                        cmds.push(Cmd::OneToOne);
                        ui.close();
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.show_minimap, "Minimap").changed() {
                        self.cache = None;
                    }
                    if ui.checkbox(&mut self.preview_selected, "Preview selected node").changed() {
                        self.eval_dirty = true;
                    }
                    let mut snap = self.ed.snap.grid.is_some();
                    if ui.checkbox(&mut snap, "Snap to grid").changed() {
                        self.ed.snap.grid = snap.then_some(20.0);
                    }
                    ui.separator();
                    if ui.radio(self.dark, "Dark theme").clicked() {
                        cmds.push(Cmd::Dark(true));
                    }
                    if ui.radio(!self.dark, "Light theme").clicked() {
                        cmds.push(Cmd::Dark(false));
                    }
                });
                ui.menu_button("Canvas", |ui| {
                    for px in [512u32, 1024, 2048] {
                        if ui.radio(self.canvas_px == px, format!("{px} x {px}")).clicked() {
                            cmds.push(Cmd::Canvas(px));
                            ui.close();
                        }
                    }
                    self.backend_menu(ui, &mut cmds);
                });
            });
        });
        cmds
    }

    #[cfg(feature = "gpu")]
    fn backend_menu(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        if self.wgpu_state.is_none() {
            return;
        }
        ui.separator();
        let gpu = self.backend.name() == "GPU";
        if ui.radio(gpu, "GPU").clicked() {
            cmds.push(Cmd::UseGpu(true));
            ui.close();
        }
        if ui.radio(!gpu, "CPU").clicked() {
            cmds.push(Cmd::UseGpu(false));
            ui.close();
        }
        if ui.checkbox(&mut self.profile, "Profile nodes (waits for the GPU)").changed() {
            self.backend.set_profile(self.profile);
            self.eval_dirty = true;
        }
    }

    #[cfg(not(feature = "gpu"))]
    fn backend_menu(&mut self, _ui: &mut egui::Ui, _cmds: &mut Vec<Cmd>) {}

    fn status_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("status").show(root, |ui| {
            ui.horizontal(|ui| {
                let n = self.frame_ms.len().max(1) as f32;
                let avg = self.frame_ms.iter().sum::<f32>() / n;
                let max = self.frame_ms.iter().copied().fold(0.0_f32, f32::max);
                ui.label(format!(
                    "{} nodes, {} links | {} | eval {:.1} ms ({} nodes) | cache {:.1} MB | frame {:.2} ms avg, {:.2} ms max | zoom {:.0}% ({} detail)",
                    self.ed.doc.node_count(),
                    self.ed.doc.all_links().len(),
                    self.backend.name(),
                    self.eval_ms,
                    self.backend.evaluated_count(),
                    self.backend.cached_bytes() as f32 / 1.0e6,
                    avg,
                    max,
                    self.ed.view.zoom * 100.0,
                    match self.ed.view.lod() {
                        compnode::Lod::Full => "full",
                        compnode::Lod::Compact => "compact",
                        compnode::Lod::Dot => "dot",
                    }
                ));
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("{ICON_ADD}  Add node")).clicked() {
                        self.open_add_menu_at_cursor();
                    }
                    if ui.button(format!("{ICON_VIEW_ALL}  Graph")).clicked() {
                        self.ed.view_all();
                    }
                    if self.ed.can_undo() && ui.button(format!("{ICON_UNDO}  Undo")).clicked() {
                        self.ed.undo();
                    }
                    if self.ed.can_redo() && ui.button(format!("{ICON_REDO}  Redo")).clicked() {
                        self.ed.redo();
                    }
                });
            });
        });
    }

    /// Left Blender-style tool rail (select / add / duplicate / frame / view / mute).
    fn toolbar(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("toolbar").resizable(false).default_size(44.0).show(root, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                let icon = |c: &str| egui::RichText::new(c).font(icon_font(18.0));
                let btn = |ui: &mut egui::Ui, c: &str, tip: &str| -> bool {
                    ui.add(egui::Button::new(icon(c))).on_hover_text(tip).clicked()
                };
                // T stays the active tool (no tool switching yet); the others
                // map directly onto existing editor actions.
                let _ = btn(ui, ICON_SELECT, "Select tool (T)");
                if btn(ui, ICON_ADD, "Add node (Shift+A)") {
                    self.open_add_menu_at_cursor();
                }
                if btn(ui, ICON_DUPLICATE, "Duplicate selection (Ctrl+D)") {
                    self.ed.duplicate();
                }
                if btn(ui, ICON_FRAME, "Frame selection (F)") {
                    self.ed.frame_selected();
                }
                if btn(ui, ICON_VIEW_ALL, "View all (Home)") {
                    self.ed.view_all();
                }
                if btn(ui, ICON_MUTE, "Mute selected (M)") {
                    self.ed.toggle_mute();
                }
                if btn(ui, ICON_CUT, "Cut links (Ctrl + right-drag)") {
                    self.status = "Cut links: Ctrl + right-drag across wires".to_string();
                }
            });
        });
    }

    /// Open the add-node menu at the current pointer (or the view center).
    fn open_add_menu_at_cursor(&mut self) {
        let gp = self.ed.view.to_graph(self.ed.view.size * 0.5 - vec2(self.ed.view.size.x * 0.25, 0.0));
        let s = self.to_screen(gp);
        self.menu = Some(MenuUi { screen: s, graph_pos: gp, connect: None, query: String::new(), first: true });
    }

    fn canvas_panel(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        egui::Panel::right("canvas").resizable(true).default_size(520.0).show(root, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Fit").clicked() {
                    self.canvas.fit();
                }
                if ui.button("1:1").clicked() {
                    self.canvas.one_to_one();
                }
                ui.label(format!(
                    "{}x{} at {:.0}%",
                    self.canvas_dims.0,
                    self.canvas_dims.1,
                    self.canvas.view.zoom * 100.0
                ));
            });
            let avail = ui.available_size();
            let size = egui::vec2(avail.x, (avail.y * 0.65).max(160.0));
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            self.canvas.set_screen_size(vec2(rect.width(), rect.height()));
            if resp.dragged() {
                let d = resp.drag_delta();
                self.canvas.pan(vec2(d.x, d.y));
            }
            if resp.double_clicked() {
                self.canvas.fit();
            }
            if resp.hovered() {
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                let zoom = ctx.input(|i| i.zoom_delta());
                if let Some(p) = resp.hover_pos() {
                    let local = vec2(p.x - rect.min.x, p.y - rect.min.y);
                    if scroll != 0.0 {
                        self.canvas.zoom_at(local, 1.002_f32.powf(scroll));
                    }
                    if zoom != 1.0 {
                        self.canvas.zoom_at(local, zoom);
                    }
                }
            }
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(24));
            let sr = self.canvas.screen_rect();
            let img_rect = egui::Rect::from_min_max(
                pos2(rect.min.x + sr.min.x, rect.min.y + sr.min.y),
                pos2(rect.min.x + sr.max.x, rect.min.y + sr.max.y),
            );
            let visible = img_rect.intersect(rect);
            if visible.width() > 0.0 && visible.height() > 0.0 && self.output_target().is_some() {
                let th = &self.ed.theme;
                painter.add(paint::checker_shape(
                    visible,
                    12.0,
                    th.c(compnode_ui::theme::Col::CheckerA),
                    th.c(compnode_ui::theme::Col::CheckerB),
                ));
                if let Some(id) = self.texture_ids().get(&CANVAS_HANDLE) {
                    painter.image(
                        *id,
                        img_rect,
                        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Add an Output node and connect it",
                    egui::FontId::proportional(14.0),
                    Color32::GRAY,
                );
            }
            ui.separator();
            self.inspector(ui);
        });
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let Some(uid) = self.ed.selection.active.filter(|u| self.ed.doc.contains(*u)) else {
                ui.label("No node selected");
                return;
            };
            let title = self.ed.doc.schema_of(uid).map(|s| s.title.clone()).unwrap_or_default();
            ui.heading(title);
            if let Some(e) = self.backend.error(uid) {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            if let Some(ms) = self.backend.timing_ms(uid) {
                ui.label(format!("last evaluation {ms:.2} ms"));
            }
            if let Some(d) = self.ed.doc.data(uid) {
                ui.label(format!("muted: {}  collapsed: {}", d.muted, d.collapsed));
            }
        });
    }

    fn text_overlay(&mut self, ctx: &egui::Context) {
        let Some(p) = self.ed.pending_text_edit().cloned() else {
            self.text_edit = None;
            return;
        };
        let mut st = self.text_edit.take().unwrap_or(TextEditUi { buf: p.request.text.clone(), first: true });
        let rect = self.ed.widget_rect(p.uid, p.slot, p.output);
        let at = rect.map_or(self.graph_rect.center(), |r| self.to_screen(r.min));
        let width = rect.map_or(160.0, |r| (r.width() * self.ed.view.zoom).max(120.0));
        let mut commit = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("compnode-text-edit")).fixed_pos(at).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let resp = if p.request.multiline {
                        ui.add(egui::TextEdit::multiline(&mut st.buf).desired_width(width).desired_rows(4))
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut st.buf).desired_width(width))
                    };
                    if st.first {
                        resp.request_focus();
                        st.first = false;
                    } else if resp.lost_focus() {
                        commit = true;
                    }
                    let (enter, esc) = ui.input(|i| {
                        let multi_ok = !p.request.multiline || i.modifiers.ctrl || i.modifiers.command;
                        (i.key_pressed(egui::Key::Enter) && multi_ok, i.key_pressed(egui::Key::Escape))
                    });
                    if esc {
                        cancel = true;
                        commit = false;
                    } else if enter {
                        commit = true;
                    }
                });
            },
        );
        if cancel {
            self.ed.cancel_text();
            self.text_edit = None;
        } else if commit {
            self.ed.commit_text(&st.buf);
            self.text_edit = None;
        } else {
            self.text_edit = Some(st);
        }
        ctx.request_repaint();
    }

    fn menu_overlay(&mut self, ctx: &egui::Context) {
        let Some(mut m) = self.menu.take() else { return };
        let mut pick: Option<String> = None;
        let mut close = false;
        let items: Vec<(String, String)> = self
            .ed
            .doc
            .registry
            .search(&m.query)
            .into_iter()
            .map(|s| (s.id.clone(), format!("{}  ({})", s.title, s.category.name())))
            .collect();
        let area = egui::Area::new(egui::Id::new("compnode-add-menu"))
            .fixed_pos(m.screen)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(240.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut m.query).hint_text("Search nodes").desired_width(f32::INFINITY),
                    );
                    if m.first {
                        resp.request_focus();
                        m.first = false;
                    }
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (id, label) in &items {
                            if ui.selectable_label(false, label).clicked() {
                                pick = Some(id.clone());
                            }
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        pick = items.first().map(|(id, _)| id.clone());
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
        let clicked_away = ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !m.first;
        if let Some(id) = pick {
            self.ed.add_from_menu(&id, m.graph_pos, m.connect);
        } else if !close && !clicked_away {
            self.menu = Some(m);
        }
        ctx.request_repaint();
    }

    fn context_overlay(&mut self, ctx: &egui::Context) {
        let Some(mut c) = self.context_menu.take() else { return };
        let mut close = false;
        let uid = c.context;
        let graph_pos = c.graph_pos;
        let screen = c.screen;
        let response = egui::Area::new(egui::Id::new("compnode-context-menu"))
            .fixed_pos(screen)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(220.0);
                    let has_ctx = uid.is_some_and(|u| self.ed.doc.contains(u));
                    let mut items: Vec<(ContextAction, &'static str, &'static str, &'static str)> = Vec::new();
                    if has_ctx {
                        let Some(uid) = uid else { return ui.response() };
                        let schema = self.ed.doc.data(uid).map(|d| d.schema.clone()).unwrap_or_default();
                        items.push((
                            ContextAction::Add { schema, pos: graph_pos + vec2(80.0, -70.0) },
                            ICON_ADD,
                            "Add node",
                            "Add",
                        ));
                        items.push((ContextAction::Duplicate, ICON_DUPLICATE, "Duplicate", "Ctrl+D"));
                        items.push((ContextAction::Delete, ICON_DELETE, "Delete", "Del"));
                        items.push((ContextAction::Mute, ICON_MUTE, "Mute", "M"));
                        items.push((ContextAction::Frame, ICON_FRAME, "Frame", "Frame"));
                    } else {
                        items.push((ContextAction::AddHere { pos: graph_pos }, ICON_ADD, "Add node", "Shift+A"));
                    }
                    let count = items.len();
                    let mut idx = c.highlight.min(count.saturating_sub(1));
                    for (i, (op, icon, label, shortcut)) in items.iter().enumerate() {
                        let selected = i == idx;
                        let text = egui::RichText::new(format!("{icon}  {label}      {shortcut}"));
                        let row = ui.selectable_label(selected, text);
                        if row.clicked() {
                            self.context_action(op.clone());
                            close = true;
                        }
                    }
                    // Blender-style keyboard navigation: Up/Down move the
                    // highlight, Enter activates, Escape dismisses.
                    let (down, up, enter, esc) = ctx.input(|i| {
                        (
                            i.key_pressed(egui::Key::ArrowDown),
                            i.key_pressed(egui::Key::ArrowUp),
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if down && idx + 1 < count {
                        idx += 1;
                        c.highlight = idx;
                    }
                    if up && idx > 0 {
                        idx -= 1;
                        c.highlight = idx;
                    }
                    if enter && !items.is_empty() && idx < items.len() {
                        self.context_action(items[idx].0.clone());
                        close = true;
                    }
                    if esc {
                        close = true;
                    }
                    ui.response()
                })
            });
        let clicked_away = ctx.input(|i| i.pointer.any_pressed())
            && !response.response.rect.contains(ctx.input(|i| i.pointer.hover_pos()).unwrap_or(screen));
        if !close && !clicked_away {
            self.context_menu = Some(c);
        }
        ctx.request_repaint();
    }

    /// Run a context-menu action (Blender-style: Add / Duplicate / Delete / ...).
    fn context_action(&mut self, op: ContextAction) {
        match op {
            ContextAction::Add { schema, pos } => {
                self.ed.add_node(&schema, pos);
            }
            ContextAction::AddHere { pos } => {
                self.ed.open_add_menu_at(pos, None);
            }
            ContextAction::Duplicate => {
                self.ed.duplicate();
            }
            ContextAction::Delete => {
                self.ed.delete_selection();
            }
            ContextAction::Mute => {
                self.ed.toggle_mute();
            }
            ContextAction::Frame => {
                self.ed.frame_selected();
            }
        }
    }

    fn accessibility(&self, ui: &egui::Ui) {
        for item in self.ed.accessibility() {
            let r = egui::Rect::from_min_max(self.to_screen(item.rect.min), self.to_screen(item.rect.max));
            let id = egui::Id::new(("compnode-a11y", item.uid, item.widget));
            let resp = ui.interact(r, id, egui::Sense::focusable_noninteractive());
            let wt = match item.role {
                AccessRole::Slider => egui::WidgetType::Slider,
                AccessRole::SpinButton => egui::WidgetType::SpinButton,
                AccessRole::CheckBox => egui::WidgetType::CheckBox,
                AccessRole::Button => egui::WidgetType::Button,
                AccessRole::ComboBox => egui::WidgetType::ComboBox,
                AccessRole::TextInput => egui::WidgetType::TextInput,
                AccessRole::ColorWell => egui::WidgetType::ColorWell,
                AccessRole::Image => egui::WidgetType::Image,
                AccessRole::Node | AccessRole::Frame | AccessRole::Label => egui::WidgetType::Label,
            };
            let label =
                if item.value.is_empty() { item.label.clone() } else { format!("{}: {}", item.label, item.value) };
            resp.widget_info(move || egui::WidgetInfo::labeled(wt, true, label.clone()));
        }
    }

    fn graph_panel(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(root, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.allocate_rect(rect, egui::Sense::hover());
            self.graph_rect = rect;
            self.ed.set_viewport_size(vec2(rect.width(), rect.height()));
            self.feed_graph_input(&ctx, rect, ui.layer_id());
            self.ed.sync();
            self.paint_graph(ui, rect);
            self.accessibility(ui);
        });
    }

    fn paint_graph(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, paint::c32(self.ed.theme.c(compnode_ui::theme::Col::Background)));
        let zoom = self.ed.view.zoom;
        let key = (self.ed.draw_key(), zoom.to_bits(), rect.width().to_bits(), rect.height().to_bits());
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            let mut dl = DrawList::new();
            self.ed.draw(&mut dl);
            let origin = self.ed.view.origin;
            let shapes = paint::build(&painter, &dl, Xf { origin, zoom }, &self.texture_ids());
            self.cache = Some(ShapeCache { key, origin, shapes });
        }
        if let Some(c) = &self.cache {
            let d = (c.origin - self.ed.view.origin) * zoom;
            let delta = egui::vec2(rect.min.x + d.x, rect.min.y + d.y);
            for s in &c.shapes {
                let mut s = s.clone();
                s.translate(delta);
                painter.add(s);
            }
        }
        if self.show_minimap {
            let mm_rect = self.minimap_rect();
            let mut mm = Minimap::new(mm_rect);
            mm.update(&self.ed.doc, &self.ed.view);
            let mut dl = DrawList::new();
            mm.draw(&self.ed.doc, &self.ed.view, &self.ed.theme, &mut dl);
            let shapes = paint::build(&painter, &dl, Xf::IDENTITY, &HashMap::new());
            for mut s in shapes {
                s.translate(egui::vec2(rect.min.x, rect.min.y));
                painter.add(s);
            }
        }
    }

    fn run_bench_step(&mut self, ctx: &egui::Context, frame_ms: f32) {
        let Some(b) = &mut self.bench else { return };
        b.samples.push(frame_ms);
        let n = b.samples.len();
        let t = n as f32 * 0.05;
        let center = self.ed.view.size * 0.5;
        self.ed.view.zoom_at(center, 1.0 + 0.012 * t.sin());
        self.ed.view.pan(vec2(3.0, 2.0));
        self.ed
            .handle_event(&InputEvent::PointerMove { pos: vec2(100.0 + t.cos() * 80.0, 100.0), mods: Modifiers::NONE });
        if n >= b.target {
            let mut s: Vec<f32> = b.samples.iter().copied().skip(20).collect();
            s.sort_by(|a, b| a.total_cmp(b));
            let avg = s.iter().sum::<f32>() / s.len().max(1) as f32;
            let p95 = s.get(s.len() * 95 / 100).copied().unwrap_or(0.0);
            println!(
                "bench: {} nodes, {} backend, {} frames: avg {avg:.2} ms, p95 {p95:.2} ms, max {:.2} ms",
                b.nodes,
                self.backend.name(),
                s.len(),
                s.last().copied().unwrap_or(0.0)
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }
}

fn mm_contains(r: GRect, p: compnode::Vec2) -> bool {
    r.contains(p)
}

#[cfg(feature = "gpu")]
fn selftest_gpu(state: Option<&eframe::egui_wgpu::RenderState>) -> i32 {
    let Some(s) = state else {
        println!("selftest-gpu: no wgpu device (run without --no-gpu)");
        return 2;
    };
    let reports = compnode_image::gpu::compare_with_cpu(Arc::new(s.device.clone()), Arc::new(s.queue.clone()), 5e-3);
    let mut failed = 0;
    for r in &reports {
        let status = if r.ok { "ok  " } else { "FAIL" };
        println!("{status} {:<24} max diff {:.5} {}", r.name, r.max_diff, r.message);
        if !r.ok {
            failed += 1;
        }
    }
    println!("selftest-gpu: {} cases, {failed} failed", reports.len());
    i32::from(failed > 0)
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        let t0 = Instant::now();
        let cmds = self.top_bar(root);
        for c in cmds {
            self.run(ctx, c);
        }
        self.handle_drops(ctx);
        self.toolbar(root);
        self.status_bar(root);
        self.canvas_panel(root);
        self.graph_panel(root);
        self.pump_actions(ctx);
        self.text_overlay(ctx);
        self.menu_overlay(ctx);
        self.context_overlay(ctx);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        if self.frame_ms.len() >= 120 {
            self.frame_ms.remove(0);
        }
        self.frame_ms.push(ms);
        self.run_bench_step(ctx, ms);
    }
}

/// Install the editor's bundled fonts: Inter for UI text and 3270 Nerd Font
/// for GUI icons (its private-use glyphs carry the toolbar/menu symbols).
fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily, FontInsert, FontPriority, InsertFontFamily};

    const INTER: &[u8] = include_bytes!("../assets/fonts/Inter.ttf");
    const NERD: &[u8] = include_bytes!("../assets/fonts/3270NerdFont-Regular.ttf");

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".to_owned(), Arc::new(FontData::from_static(INTER)));
    fonts.font_data.insert("nerd".to_owned(), Arc::new(FontData::from_static(NERD)));

    // Inter is the primary UI font; the Nerd Font is the last fallback so its
    // private-use icon glyphs resolve without changing the text look.
    let proportional = fonts.families.get_mut(&FontFamily::Proportional).expect("proportional family");
    proportional.insert(0, "inter".to_owned());
    proportional.push("nerd".to_owned());
    let monospace = fonts.families.get_mut(&FontFamily::Monospace).expect("monospace family");
    monospace.push("nerd".to_owned());
    ctx.set_fonts(fonts);

    // A named "icons" family backed by the Nerd Font, so icon labels can opt in
    // explicitly with `FontId::new(size, FontFamily::Name("icons".into()))`.
    ctx.add_font(FontInsert::new(
        "icons",
        FontData::from_static(NERD),
        vec![InsertFontFamily { family: FontFamily::Name("icons".into()), priority: FontPriority::Highest }],
    ));
}
