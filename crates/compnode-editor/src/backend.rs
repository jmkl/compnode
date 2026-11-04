//! The evaluation backend the app talks to: the CPU reference evaluator or
//! (feature `gpu`) the wgpu one. Both share the loader that serves
//! asynchronously loaded files.
use compnode_image::loader::{ImageLoader, StoreLoader};
use compnode_image::{Evaluator, ImageBuf};
use compnode_ui::document::{Change, Document};
use std::sync::Arc;

pub enum Output {
    Cpu(Arc<ImageBuf>),
    #[cfg(feature = "gpu")]
    Gpu(Arc<compnode_image::gpu::GpuImage>),
}

impl Output {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Output::Cpu(i) => (i.width, i.height),
            #[cfg(feature = "gpu")]
            Output::Gpu(i) => (i.width, i.height),
        }
    }
}

pub enum Backend {
    Cpu(Box<Evaluator>),
    #[cfg(feature = "gpu")]
    Gpu(Box<compnode_image::gpu::GpuEvaluator>),
}

impl Backend {
    pub fn cpu(canvas: u32, store: &StoreLoader) -> Backend {
        let loader: Arc<dyn ImageLoader> = Arc::new(store.clone());
        Backend::Cpu(Box::new(Evaluator::new((canvas, canvas)).with_loader(loader)))
    }

    #[cfg(feature = "gpu")]
    pub fn gpu(state: &eframe::egui_wgpu::RenderState, canvas: u32, store: &StoreLoader) -> Backend {
        use compnode_image::gpu::{GpuContext, GpuEvaluator};
        let ctx = GpuContext::new(Arc::new(state.device.clone()), Arc::new(state.queue.clone()), 256 * 1024 * 1024);
        let loader: Arc<dyn ImageLoader> = Arc::new(store.clone());
        Backend::Gpu(Box::new(GpuEvaluator::new(ctx, (canvas, canvas)).with_loader(loader)))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Backend::Cpu(_) => "CPU",
            #[cfg(feature = "gpu")]
            Backend::Gpu(_) => "GPU",
        }
    }

    pub fn apply_changes(&mut self, doc: &Document, changes: &[Change]) {
        match self {
            Backend::Cpu(e) => e.apply_changes(doc, changes),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.apply_changes(doc, changes),
        }
    }

    pub fn invalidate_file(&mut self, doc: &Document, path: &str) {
        match self {
            Backend::Cpu(e) => e.invalidate_file(doc, path),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.invalidate_file(doc, path),
        }
    }

    pub fn evaluate(&mut self, doc: &Document, uid: u64) -> Result<Output, String> {
        match self {
            Backend::Cpu(e) => e.evaluate(doc, uid).map(Output::Cpu).map_err(|e| e.to_string()),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.evaluate(doc, uid).map(Output::Gpu).map_err(|e| e.to_string()),
        }
    }

    /// The result as a CPU image (for export). The GPU result is read back.
    pub fn read_cpu(&self, out: &Output) -> Result<Arc<ImageBuf>, String> {
        match (self, out) {
            (_, Output::Cpu(i)) => Ok(Arc::clone(i)),
            #[cfg(feature = "gpu")]
            (Backend::Gpu(e), Output::Gpu(i)) => e.ctx.download(i).map(Arc::new),
            #[cfg(feature = "gpu")]
            (Backend::Cpu(_), Output::Gpu(_)) => Err("GPU result without a GPU backend".to_string()),
        }
    }

    pub fn error(&self, uid: u64) -> Option<String> {
        match self {
            Backend::Cpu(e) => e.error(uid).map(str::to_string),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.error(uid).map(str::to_string),
        }
    }

    pub fn evaluated_count(&self) -> usize {
        match self {
            Backend::Cpu(e) => e.last_evaluated.len(),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.last_evaluated.len(),
        }
    }

    pub fn cached_bytes(&self) -> u64 {
        match self {
            Backend::Cpu(e) => e.cached_bytes() as u64,
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.cached_bytes(),
        }
    }

    pub fn set_canvas(&mut self, canvas: u32) {
        match self {
            Backend::Cpu(e) => e.set_canvas((canvas, canvas)),
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.set_canvas((canvas, canvas)),
        }
    }

    /// Per-node evaluation time in milliseconds (GPU backend, with profiling on).
    pub fn timing_ms(&self, uid: u64) -> Option<f32> {
        match self {
            Backend::Cpu(_) => None,
            #[cfg(feature = "gpu")]
            Backend::Gpu(e) => e.timings.get(&uid).map(|d| d.as_secs_f32() * 1000.0),
        }
    }

    #[cfg(feature = "gpu")]
    pub fn set_profile(&mut self, on: bool) {
        if let Backend::Gpu(e) = self {
            e.profile = on;
        }
    }
}
