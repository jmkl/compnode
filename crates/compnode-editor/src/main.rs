//! compnode editor: an egui application around `compnode-ui`.
//!
//! ```text
//! cargo run --release -p compnode-editor -- [project.cnp]
//!     --no-gpu            evaluate on the CPU even if a GPU is available
//!     --canvas N          canvas size (default 2048 on the GPU, 512 on the CPU)
//!     --selftest-gpu      compare every GPU node with the CPU reference, then exit
//!     --bench [NODES]     scripted pan/zoom over a large graph, prints frame times
//! ```
#![allow(deprecated)]

mod app;
mod backend;
mod paint;

use eframe::egui;
use std::path::PathBuf;

pub struct Args {
    pub no_gpu: bool,
    pub selftest_gpu: bool,
    pub bench: Option<usize>,
    pub canvas: Option<u32>,
    pub project: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { no_gpu: false, selftest_gpu: false, bench: None, canvas: None, project: None };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--no-gpu" => a.no_gpu = true,
            "--selftest-gpu" => a.selftest_gpu = true,
            "--canvas" => a.canvas = it.next().and_then(|v| v.parse().ok()),
            "--bench" => {
                let n = it.next_if(|v| v.parse::<usize>().is_ok()).and_then(|v| v.parse().ok());
                a.bench = Some(n.unwrap_or(1000));
            }
            other if !other.starts_with("--") => a.project = Some(PathBuf::from(other)),
            other => eprintln!("ignoring unknown option {other}"),
        }
    }
    a
}

fn main() -> eframe::Result {
    let args = parse_args();
    let mut options = eframe::NativeOptions {
        // Smooth (anti-aliased) edges: wires, node bodies and widgets are
        // submitted as raw meshes, which bypass the tessellator's feathering,
        // so the final edge AA comes from multisampling of the render target.
        multisampling: 4,
        viewport: egui::ViewportBuilder::default().with_inner_size([1500.0, 900.0]).with_title("compnode"),
        ..Default::default()
    };
    #[cfg(feature = "gpu")]
    if !args.no_gpu {
        options.renderer = eframe::Renderer::Wgpu;
    }
    eframe::run_native("compnode", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, &args)))))
}
