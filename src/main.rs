#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod scan;
mod theme;
mod treemap;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect,
    RichText, Sense, Stroke, StrokeKind,
};

use scan::Tree;
use theme as th;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_maximized(true)
            .with_title("DiskScope"),
        ..Default::default()
    };
    eframe::run_native(
        "DiskScope",
        options,
        Box::new(|cc| Ok(Box::new(DiskScope::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Explorer,
    Largest,
    Apps,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Size,
    Name,
    Items,
}

#[derive(Clone)]
struct Drive {
    letter: String,
    label: String,
    root: PathBuf,
    total: u64,
    free: u64,
}

impl Drive {
    fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    fn name(&self) -> String {
        if self.label.is_empty() {
            format!("Local Disk ({})", self.letter)
        } else {
            format!("{} ({})", self.label, self.letter)
        }
    }
}

struct ScanJob {
    progress: Arc<scan::Progress>,
    rx: Receiver<Option<Tree>>,
    root: PathBuf,
    pending: Option<PathBuf>,
}

struct Row {
    id: Option<usize>,
    path: PathBuf,
    name: String,
    size: u64,
    items: u64,
    is_dir: bool,
}

struct DiskScope {
    view: View,
    drives: Vec<Drive>,
    tree: Option<Arc<Tree>>,
    job: Option<ScanJob>,
    current: usize,
    selected: Option<PathBuf>,
    pending_nav: Option<usize>,
    sort: SortKey,
    asc: bool,
    filter: String,
    apps: Option<Vec<apps::AppEntry>>,
    apps_rx: Option<Receiver<Vec<apps::AppEntry>>>,
    apps_filter: String,
    apps_sort_size: bool,
    confirm: Option<PathBuf>,
    toast_msg: Option<(String, Instant)>,
    last_drive_refresh: Instant,
}

impl DiskScope {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        th::apply(&cc.egui_ctx);
        let mut app = Self {
            view: View::Explorer,
            drives: list_drives(),
            tree: None,
            job: None,
            current: 0,
            selected: None,
            pending_nav: None,
            sort: SortKey::Size,
            asc: false,
            filter: String::new(),
            apps: None,
            apps_rx: None,
            apps_filter: String::new(),
            apps_sort_size: true,
            confirm: None,
            toast_msg: None,
            last_drive_refresh: Instant::now(),
        };
        if let Some(arg) = std::env::args().nth(1) {
            let arg = arg.trim().trim_matches('"').to_owned();
            let path = PathBuf::from(arg);
            if path.is_dir() {
                app.start_scan(path, None);
            } else if path.is_file() {
                if let Some(parent) = path.parent() {
                    app.start_scan(parent.to_path_buf(), None);
                }
            }
        }
        if let Ok(view) = std::env::var("DISKSCOPE_VIEW") {
            app.view = match view.to_lowercase().as_str() {
                "apps" => View::Apps,
                "largest" => View::Largest,
                _ => View::Explorer,
            };
        }
        app
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast_msg = Some((message.into(), Instant::now()));
    }

    fn refresh_drives(&mut self) {
        self.drives = list_drives();
        self.last_drive_refresh = Instant::now();
    }

    fn start_scan(&mut self, root: PathBuf, pending: Option<PathBuf>) {
        if self.job.is_some() {
            return;
        }
        let root = normalize_root(root);
        self.tree = None;
        self.selected = None;
        self.pending_nav = None;
        self.filter.clear();
        let (progress, rx) = scan::spawn_scan(root.clone());
        self.job = Some(ScanJob {
            progress,
            rx,
            root,
            pending,
        });
        self.view = View::Explorer;
    }

    fn navigate_or_scan(&mut self, path: &Path) {
        if let Some(tree) = self.tree.clone() {
            if let Some(id) = tree.find(path) {
                self.navigate(id);
                return;
            }
        }
        self.start_scan(path.to_path_buf(), None);
    }

    fn navigate(&mut self, id: usize) {
        self.current = id;
        self.selected = None;
        self.filter.clear();
    }

    fn up(&mut self) {
        if let Some(tree) = self.tree.clone() {
            if let Some(parent) = tree.node(self.current).parent {
                self.navigate(parent);
            }
        }
    }

    fn scan_target(&self) -> PathBuf {
        if let Some(tree) = &self.tree {
            return tree.root_node().path.clone();
        }
        if let Some(drive) = self.drives.iter().find(|d| {
            std::env::var("SystemDrive")
                .map(|s| s.eq_ignore_ascii_case(&d.letter))
                .unwrap_or(false)
        }) {
            return drive.root.clone();
        }
        self.drives
            .first()
            .map(|d| d.root.clone())
            .unwrap_or_else(|| PathBuf::from("C:\\"))
    }

    fn poll_job(&mut self) {
        let mut outcome: Option<Option<Tree>> = None;
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Ok(tree) => outcome = Some(tree),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => outcome = Some(None),
            }
        }
        let Some(outcome) = outcome else { return };
        let job = self.job.take();
        match outcome {
            Some(tree) => {
                let mut summary = format!(
                    "Scanned {} files in {} folders · {}",
                    th::fmt_num(tree.total_files),
                    th::fmt_num(tree.total_dirs),
                    th::fmt_secs(tree.elapsed)
                );
                if tree.errors > 0 {
                    summary.push_str(&format!(
                        " · {} skipped",
                        th::fmt_num(tree.errors)
                    ));
                }
                let tree = Arc::new(tree);
                let pending = job.and_then(|j| j.pending);
                self.current = pending
                    .as_deref()
                    .and_then(|p| tree.find(p))
                    .unwrap_or(tree.root);
                self.tree = Some(tree);
                self.selected = None;
                self.filter.clear();
                // Drop any app list computed without a scan tree so the Apps
                // tab re-resolves sizes from the exact directory sizes.
                self.apps = None;
                self.apps_rx = None;
                self.refresh_drives();
                self.toast(summary);
            }
            None => {
                self.toast("Scan cancelled");
            }
        }
    }

    fn poll_apps(&mut self) {
        let mut result = None;
        if let Some(rx) = &self.apps_rx {
            match rx.try_recv() {
                Ok(list) => result = Some(list),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => result = Some(Vec::new()),
            }
        }
        if let Some(list) = result {
            self.apps = Some(list);
            self.apps_rx = None;
        }
    }

    // ---------------------------------------------------------------- panels

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("topbar")
            .exact_size(58.0)
            .frame(Frame::new().fill(th::TOPBAR).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let (logo_rect, _) =
                        ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    paint_logo(ui.painter(), logo_rect, th::TOPBAR);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("DiskScope")
                            .family(th::semibold_family())
                            .size(16.5)
                            .color(th::TEXT),
                    );
                    ui.add_space(18.0);
                    tabs(ui, &mut self.view);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.job.is_some() {
                            ui.label(RichText::new("Scanning…").size(12.0).color(th::MUTED));
                        } else {
                            let label = if self.tree.is_some() { "Rescan" } else { "Scan" };
                            if ui.add(th::accent_button(label)).clicked() {
                                let target = self.scan_target();
                                self.start_scan(target, None);
                            }
                        }
                        if let Some(tree) = self.tree.clone() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} scanned",
                                    th::fmt_bytes(tree.root_node().size)
                                ))
                                .size(12.0)
                                .color(th::MUTED),
                            );
                        }
                    });
                });
            });
    }

    fn side_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .exact_size(244.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(th::SIDEBAR)
                    .inner_margin(Margin::symmetric(12, 12)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        th::section_label(ui, "Scan");
                        let w = ui.available_width();
                        if ui
                            .add_sized([w, 34.0], th::accent_button("Scan this PC"))
                            .clicked()
                        {
                            let target = self.scan_target();
                            self.start_scan(target, None);
                        }
                        if ui
                            .add_sized([w, 32.0], th::ghost_button("Choose folder…"))
                            .clicked()
                        {
                            if let Some(dir) = rfd::FileDialog::new()
                                .set_title("Choose a folder to scan")
                                .pick_folder()
                            {
                                self.start_scan(dir, None);
                            }
                        }

                        ui.add_space(14.0);
                        th::section_label(ui, "Drives");
                        let drives = self.drives.clone();
                        for drive in &drives {
                            if drive_card(ui, drive).clicked() {
                                self.navigate_or_scan(&drive.root);
                            }
                        }

                        ui.add_space(14.0);
                        th::section_label(ui, "Quick access");
                        let quick = quick_access();
                        for (label, path) in quick {
                            let active = self
                                .tree
                                .as_ref()
                                .map(|t| same_path(&t.node(self.current).path, &path))
                                .unwrap_or(false);
                            if nav_row(ui, &label, active).clicked() {
                                self.navigate_or_scan(&path);
                            }
                        }

                        ui.add_space(18.0);
                        ui.label(
                            RichText::new("Read-only scanning.\nNothing changes unless you ask.")
                                .size(10.5)
                                .color(th::DIM),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new("DiskScope 1.0").size(10.5).color(th::DIM));
                    });
            });
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(th::BG)
                    .inner_margin(Margin::symmetric(18, 14)),
            )
            .show(ui, |ui| match self.view {
                View::Explorer => {
                    if self.job.is_some() {
                        self.scanning_state(ui);
                    } else if self.tree.is_none() {
                        self.empty_state(ui);
                    } else {
                        self.explorer_view(ui);
                    }
                }
                View::Largest => {
                    if self.job.is_some() {
                        self.scanning_state(ui);
                    } else if self.tree.is_none() {
                        self.empty_state(ui);
                    } else {
                        self.largest_view(ui);
                    }
                }
                View::Apps => self.apps_view(ui),
            });
    }

    // ----------------------------------------------------------- empty/scan

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let h = ui.available_height();
        ui.add_space((h * 0.14).max(8.0));
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(88.0, 88.0), Sense::hover());
            paint_logo(ui.painter(), rect, th::BG);
            ui.add_space(16.0);
            ui.label(
                RichText::new("See what's taking up your space")
                    .family(th::semibold_family())
                    .size(26.0)
                    .color(th::TEXT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Pick a drive or folder. Scanning is read-only, parallel and stays on this PC.",
                )
                .size(13.0)
                .color(th::MUTED),
            );
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                let target = self.scan_target();
                if ui
                    .add_sized([170.0, 38.0], th::accent_button("Scan this PC"))
                    .clicked()
                {
                    self.start_scan(target, None);
                }
                if ui
                    .add_sized([170.0, 38.0], th::ghost_button("Choose folder…"))
                    .clicked()
                {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("Choose a folder to scan")
                        .pick_folder()
                    {
                        self.start_scan(dir, None);
                    }
                }
            });
            ui.add_space(26.0);
            let drives = self.drives.clone();
            let total_w = drives.len() as f32 * 210.0
                + drives.len().saturating_sub(1) as f32 * 8.0;
            ui.horizontal_wrapped(|ui| {
                ui.add_space(((ui.available_width() - total_w) * 0.5).max(0.0));
                for drive in &drives {
                    if drive_chip(ui, drive).clicked() {
                        self.start_scan(drive.root.clone(), None);
                    }
                }
            });
        });
    }

    fn scanning_state(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.job else { return };
        let root = job.root.clone();
        let (files, dirs, bytes, errors, current) = job.progress.snapshot();
        let progress = job.progress.clone();

        ui.add_space((ui.available_height() * 0.18).max(10.0));
        ui.vertical_centered(|ui| {
            Frame::new()
                .fill(th::PANEL)
                .stroke(Stroke::new(1.0, th::BORDER_SOFT))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::same(26))
                .show(ui, |ui| {
                    ui.set_width(470.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("Scanning {}", root.display()))
                                .family(th::semibold_family())
                                .size(15.0)
                                .color(th::TEXT),
                        );
                    });
                    ui.add_space(14.0);
                    let (bar, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(bar, CornerRadius::same(3), th::WIDGET);
                    let t = ui.input(|i| i.time) as f32;
                    let phase = (t * 0.42).fract();
                    let seg_w = bar.width() * 0.28;
                    let x = bar.left() + (bar.width() + seg_w) * phase - seg_w;
                    let seg = Rect::from_min_max(
                        pos2(x.max(bar.left()), bar.top()),
                        pos2((x + seg_w).min(bar.right()), bar.bottom()),
                    );
                    if seg.width() > 1.0 {
                        painter.rect_filled(seg, CornerRadius::same(3), th::ACCENT);
                    }
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        stat_block(ui, "Files", &th::fmt_num(files));
                        ui.add_space(26.0);
                        stat_block(ui, "Data", &th::fmt_bytes(bytes));
                        ui.add_space(26.0);
                        stat_block(ui, "Folders", &th::fmt_num(dirs));
                    });
                    ui.add_space(14.0);
                    let shown = if current.is_empty() {
                        "reading directory tree…".to_owned()
                    } else {
                        th::truncate(&current, 430.0, 11.0)
                    };
                    ui.label(RichText::new(shown).size(11.0).color(th::MUTED));
                    if errors > 0 {
                        ui.label(
                            RichText::new(format!(
                                "{} items skipped (permissions)",
                                th::fmt_num(errors)
                            ))
                            .size(10.5)
                            .color(th::DIM),
                        );
                    }
                    ui.add_space(16.0);
                    if ui.add(th::ghost_button("Cancel scan")).clicked() {
                        progress
                            .cancelled
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
        });
    }

    // -------------------------------------------------------------- explorer

    fn explorer_view(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = self.tree.clone() else { return };
        let node = tree.node(self.current);
        let is_root = node.parent.is_none();

        ui.horizontal(|ui| {
            let back = egui::Button::new(
                RichText::new("←")
                    .size(15.0)
                    .color(if is_root { th::DIM } else { th::TEXT }),
            )
            .fill(th::WIDGET)
            .stroke(Stroke::new(1.0, th::BORDER_SOFT))
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(34.0, 30.0));
            if ui.add_enabled(!is_root, back).clicked() {
                self.up();
            }
            ui.add_space(2.0);
            if let Some(target) = breadcrumbs(ui, &tree, self.current) {
                self.navigate(target);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(th::ghost_button("Open folder")).clicked() {
                    open_path(&node.path);
                }
                if let Some(sel) = self.selected.clone() {
                    if ui.add(th::danger_button("Recycle")).clicked() {
                        self.confirm = Some(sel.clone());
                    }
                    if ui.add(th::ghost_button("Copy path")).clicked() {
                        ui.ctx().copy_text(sel.display().to_string());
                        self.toast("Path copied");
                    }
                }
            });
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&node.name)
                        .family(th::semibold_family())
                        .size(23.0)
                        .color(th::TEXT),
                );
                ui.label(
                    RichText::new(node.path.display().to_string())
                        .size(11.0)
                        .color(th::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                chip(ui, &format!("{} folders", th::fmt_num(node.children.len() as u64)));
                chip(ui, &format!("{} files", th::fmt_num(node.files)));
                chip(ui, &th::fmt_bytes(node.size));
            });
        });

        ui.add_space(10.0);
        self.treemap_card(ui, &tree);
        ui.add_space(10.0);
        self.item_list(ui, &tree);

        if let Some(id) = self.pending_nav.take() {
            self.navigate(id);
        }
    }

    fn treemap_card(&mut self, ui: &mut egui::Ui, tree: &Tree) {
        let node = tree.node(self.current);
        let height = (ui.available_height() * 0.44).clamp(150.0, 420.0);
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        let painter = ui.painter().clone();
        painter.rect_filled(rect, CornerRadius::same(12), th::PANEL);
        painter.rect_stroke(
            rect,
            CornerRadius::same(12),
            Stroke::new(1.0, th::BORDER_SOFT),
            StrokeKind::Inside,
        );

        let inner = rect.shrink2(vec2(8.0, 8.0));
        if node.children.is_empty() {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "No subfolders here",
                FontId::proportional(12.5),
                th::DIM,
            );
            return;
        }

        const MAX_BLOCKS: usize = 48;
        struct Block {
            child: Option<usize>,
            file: Option<PathBuf>,
            size: u64,
            label: String,
        }
        let mut blocks: Vec<Block> = Vec::new();
        if node.children.is_empty() {
            for (name, size) in node.top_files.iter().take(MAX_BLOCKS) {
                blocks.push(Block {
                    child: None,
                    file: Some(node.path.join(name)),
                    size: *size,
                    label: name.clone(),
                });
            }
            let listed: u64 = blocks.iter().map(|b| b.size).sum();
            let rest = node.own.saturating_sub(listed);
            if rest > 0 {
                let hidden = node.files.saturating_sub(blocks.len() as u64);
                blocks.push(Block {
                    child: None,
                    file: None,
                    size: rest,
                    label: format!("{} smaller files", th::fmt_num(hidden)),
                });
            }
        } else {
            for &child in node.children.iter().take(MAX_BLOCKS) {
                let n = tree.node(child);
                blocks.push(Block {
                    child: Some(child),
                    file: None,
                    size: n.size,
                    label: n.name.clone(),
                });
            }
            if node.children.len() > MAX_BLOCKS {
                let rest: u64 = node.children[MAX_BLOCKS..]
                    .iter()
                    .map(|&c| tree.node(c).size)
                    .sum();
                if rest > 0 {
                    blocks.push(Block {
                        child: None,
                        file: None,
                        size: rest,
                        label: format!("{} more folders", node.children.len() - MAX_BLOCKS),
                    });
                }
            }
        }
        blocks.retain(|b| b.size > 0);
        if blocks.is_empty() {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "This folder is empty",
                FontId::proportional(12.5),
                th::DIM,
            );
            return;
        }

        let weights: Vec<f64> = blocks.iter().map(|b| b.size as f64).collect();
        let rects = treemap::squarify(&weights, inner);

        let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
        let mut hovered: Option<usize> = None;

        for (i, block) in blocks.iter().enumerate() {
            let mut r = rects[i];
            if !r.is_positive() {
                continue;
            }
            r = r.shrink(1.5);
            if r.width() < 3.0 || r.height() < 3.0 {
                continue;
            }
            let clickable = block.child.is_some() || block.file.is_some();
            let base = if clickable {
                treemap::color(i)
            } else {
                Color32::from_rgb(46, 56, 70)
            };
            let is_selected = match (&block.child, &block.file) {
                (Some(id), _) => self
                    .selected
                    .as_deref()
                    .map(|p| same_path(p, &tree.node(*id).path))
                    .unwrap_or(false),
                (_, Some(path)) => self
                    .selected
                    .as_deref()
                    .map(|p| same_path(p, path))
                    .unwrap_or(false),
                _ => false,
            };
            let is_hovered = pointer.map(|p| r.contains(p)).unwrap_or(false);
            if is_hovered {
                hovered = Some(i);
            }
            let fill = if is_hovered {
                base.lerp_to_gamma(Color32::WHITE, 0.16)
            } else {
                base
            };
            painter.rect_filled(r, CornerRadius::same(4), fill);
            if is_selected {
                painter.rect_stroke(
                    r,
                    CornerRadius::same(4),
                    Stroke::new(2.0, Color32::WHITE),
                    StrokeKind::Inside,
                );
            }

            let clip = painter.with_clip_rect(r.shrink(2.0));
            if r.width() > 54.0 && r.height() > 22.0 {
                let label = th::truncate(&block.label, r.width() - 12.0, 12.0);
                clip.text(
                    pos2(r.left() + 6.0, r.top() + 5.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(12.0),
                    Color32::from_rgb(240, 245, 252),
                );
            }
            if r.width() > 76.0 && r.height() > 40.0 {
                clip.text(
                    pos2(r.left() + 6.0, r.top() + 21.0),
                    Align2::LEFT_TOP,
                    th::fmt_bytes(block.size),
                    FontId::proportional(10.5),
                    Color32::from_rgb(226, 234, 245).gamma_multiply(0.85),
                );
            }
        }

        if response.clicked() {
            if let Some(i) = hovered {
                let block = &blocks[i];
                if let Some(path) = &block.file {
                    self.selected = Some(path.clone());
                } else if let Some(id) = block.child {
                    self.selected = Some(tree.node(id).path.clone());
                }
            }
        }
        if response.double_clicked() {
            if let Some(i) = hovered {
                let block = &blocks[i];
                if let Some(id) = block.child {
                    self.pending_nav = Some(id);
                } else if let Some(path) = &block.file {
                    reveal_path(path);
                }
            }
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Tooltip painted by hand so it always matches the dark theme.
        if let (Some(i), Some(pointer)) = (hovered, pointer) {
            let block = &blocks[i];
            let pct = th::pct(block.size, node.size.max(1));
            let detail = if let Some(id) = block.child {
                let n = tree.node(id);
                format!(
                    "{} · {} files · {} folders",
                    pct,
                    th::fmt_num(n.files),
                    th::fmt_num(n.children.len() as u64)
                )
            } else if block.file.is_some() {
                format!("{pct} · file")
            } else {
                pct.to_string()
            };
            draw_tooltip(
                &painter,
                ui.ctx().viewport_rect(),
                pointer,
                &block.label,
                &format!("{} · {}", th::fmt_bytes(block.size), detail),
            );
        }
    }

    fn item_list(&mut self, ui: &mut egui::Ui, tree: &Tree) {
        let node = tree.node(self.current);
        let total = node.size.max(1);

        let mut rows: Vec<Row> = Vec::with_capacity(node.children.len() + node.top_files.len());
        for &child in &node.children {
            let n = tree.node(child);
            rows.push(Row {
                id: Some(child),
                path: n.path.clone(),
                name: n.name.clone(),
                size: n.size,
                items: n.children.len() as u64 + n.files,
                is_dir: true,
            });
        }
        let listed: u64 = node.top_files.iter().map(|f| f.1).sum();
        for (name, size) in &node.top_files {
            rows.push(Row {
                id: None,
                path: node.path.join(name),
                name: name.clone(),
                size: *size,
                items: 0,
                is_dir: false,
            });
        }
        let needle = self.filter.trim().to_lowercase();
        if !needle.is_empty() {
            rows.retain(|r| r.name.to_lowercase().contains(&needle));
        }
        match self.sort {
            SortKey::Size => rows.sort_by(|a, b| b.size.cmp(&a.size)),
            SortKey::Name => rows.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then(b.size.cmp(&a.size))
            }),
            SortKey::Items => rows.sort_by(|a, b| b.items.cmp(&a.items).then(b.size.cmp(&a.size))),
        }
        if self.asc {
            rows.reverse();
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Contents")
                    .family(th::semibold_family())
                    .size(13.0)
                    .color(th::TEXT),
            );
            ui.label(
                RichText::new(format!(
                    "{} items{}",
                    th::fmt_num(rows.len() as u64),
                    if needle.is_empty() {
                        String::new()
                    } else {
                        format!(" matching “{}”", self.filter.trim())
                    }
                ))
                .size(11.5)
                .color(th::MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter…")
                        .desired_width(170.0),
                );
            });
        });
        ui.add_space(4.0);

        let header_h = 24.0;
        let (header_rect, header_resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), header_h), Sense::click());
        paint_header(ui.painter(), header_rect, self.sort, self.asc);
        if header_resp.clicked() {
            if let Some(p) = header_resp.interact_pointer_pos() {
                let c = columns(header_rect);
                let key = if p.x < c.name_right {
                    SortKey::Name
                } else if p.x < c.pct_right {
                    SortKey::Size
                } else if p.x < c.items_right {
                    SortKey::Items
                } else {
                    SortKey::Size
                };
                if key == self.sort {
                    self.asc = !self.asc;
                } else {
                    self.sort = key;
                    self.asc = key == SortKey::Name;
                }
            }
        }

        let remaining_h = ui.available_height().max(60.0);
        let row_h = 32.0;
        let body_h = (remaining_h - 24.0).max(40.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body_h),
            Layout::top_down(Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("items")
                    .auto_shrink([false, false])
                    .show_rows(ui, row_h, rows.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for i in range {
                            let row = &rows[i];
                            let (rect, resp) = ui
                                .allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                            let selected = self
                                .selected
                                .as_deref()
                                .map(|p| same_path(p, &row.path))
                                .unwrap_or(false);
                            if selected {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::SELECT,
                                );
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::PANEL_HI,
                                );
                            }
                            paint_row(ui.painter(), rect, row, total);
                            if resp.clicked() {
                                self.selected = Some(row.path.clone());
                            }
                            if resp.double_clicked() {
                                if let Some(id) = row.id {
                                    self.pending_nav = Some(id);
                                } else {
                                    reveal_path(&row.path);
                                }
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    });
            },
        );

        let hidden_files = node.files.saturating_sub(node.top_files.len() as u64);
        let hidden_bytes = node.own.saturating_sub(listed);
        if hidden_files > 0 {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "plus {} smaller files · {}",
                    th::fmt_num(hidden_files),
                    th::fmt_bytes(hidden_bytes)
                ))
                .size(10.5)
                .color(th::DIM),
            );
        }
    }

    // ---------------------------------------------------------------- largest

    fn largest_view(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = self.tree.clone() else { return };
        let total: u64 = tree.top_files.iter().map(|f| f.size).sum();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Largest files")
                        .family(th::semibold_family())
                        .size(23.0)
                        .color(th::TEXT),
                );
                ui.label(
                    RichText::new(format!(
                        "{} files over 8 MB  ·  {} total",
                        th::fmt_num(tree.top_files.len() as u64),
                        th::fmt_bytes(total)
                    ))
                    .size(11.0)
                    .color(th::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter by name or path…")
                        .desired_width(240.0),
                );
            });
        });
        ui.add_space(8.0);

        let needle = self.filter.trim().to_lowercase();
        let hits: Vec<usize> = tree
            .top_files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                needle.is_empty()
                    || f.path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();

        let (header_rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            pos2(header_rect.left() + 12.0, header_rect.center().y),
            Align2::LEFT_CENTER,
            "File",
            FontId::proportional(11.0),
            th::DIM,
        );
        painter.text(
            pos2(header_rect.right() - 14.0, header_rect.center().y),
            Align2::RIGHT_CENTER,
            "Size",
            FontId::proportional(11.0),
            th::DIM,
        );

        let body_h = ui.available_height().max(80.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body_h),
            Layout::top_down(Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("largest")
                    .auto_shrink([false, false])
                    .show_rows(ui, 42.0, hits.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row_index in range {
                            let file = &tree.top_files[hits[row_index]];
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 42.0),
                                Sense::click(),
                            );
                            let selected = self
                                .selected
                                .as_deref()
                                .map(|p| same_path(p, &file.path))
                                .unwrap_or(false);
                            if selected {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::SELECT,
                                );
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::PANEL_HI,
                                );
                            }
                            let p = ui.painter();
                            let name = file
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| file.path.display().to_string());
                            let folder = file
                                .path
                                .parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default();
                            let clip = p.with_clip_rect(rect.shrink2(vec2(12.0, 2.0)));
                            clip.text(
                                pos2(rect.left() + 12.0, rect.top() + 7.0),
                                Align2::LEFT_TOP,
                                th::truncate(&name, rect.width() - 130.0, 13.0),
                                FontId::proportional(13.0),
                                th::TEXT,
                            );
                            clip.text(
                                pos2(rect.left() + 12.0, rect.top() + 24.0),
                                Align2::LEFT_TOP,
                                th::truncate(&folder, rect.width() - 130.0, 10.5),
                                FontId::proportional(10.5),
                                th::DIM,
                            );
                            p.text(
                                pos2(rect.right() - 14.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                th::fmt_bytes(file.size),
                                FontId::proportional(12.5),
                                th::TEXT,
                            );
                            if resp.clicked() {
                                self.selected = Some(file.path.clone());
                            }
                            if resp.double_clicked() {
                                reveal_path(&file.path);
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    });
            },
        );
    }

    // ------------------------------------------------------------------- apps

    fn apps_view(&mut self, ui: &mut egui::Ui) {
        if self.apps.is_none() && self.apps_rx.is_none() {
            self.apps_rx = Some(apps::spawn_apps_scan(self.tree.clone()));
        }
        let loading = self.apps_rx.is_some();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Installed apps")
                        .family(th::semibold_family())
                        .size(23.0)
                        .color(th::TEXT),
                );
                let summary = match &self.apps {
                    Some(list) => {
                        let total: u64 = list.iter().map(|a| a.size).sum();
                        let hint = if self.tree.is_some() {
                            ""
                        } else {
                            "  ·  scan a drive for exact sizes"
                        };
                        format!(
                            "{} apps  ·  {} across install folders{}",
                            th::fmt_num(list.len() as u64),
                            th::fmt_bytes(total),
                            hint
                        )
                    }
                    None => "Reading the uninstall registry…".to_owned(),
                };
                ui.label(RichText::new(summary).size(11.0).color(th::MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(th::ghost_button("Refresh")).clicked() {
                    self.apps = None;
                    self.apps_rx = Some(apps::spawn_apps_scan(self.tree.clone()));
                }
                let label = if self.apps_sort_size { "Size ↓" } else { "Name ↓" };
                if ui.add(th::ghost_button(label)).clicked() {
                    self.apps_sort_size = !self.apps_sort_size;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.apps_filter)
                        .hint_text("Filter apps…")
                        .desired_width(190.0),
                );
            });
        });
        ui.add_space(8.0);

        if loading && self.apps.is_none() {
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Measuring install folders…").size(12.5).color(th::MUTED));
            });
            return;
        }

        let needle = self.apps_filter.trim().to_lowercase();
        let mut list: Vec<&apps::AppEntry> = self
            .apps
            .as_ref()
            .map(|l| {
                l.iter()
                    .filter(|a| {
                        needle.is_empty()
                            || a.name.to_lowercase().contains(&needle)
                            || a.publisher.to_lowercase().contains(&needle)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self.apps_sort_size {
            list.sort_by(|a, b| b.size.cmp(&a.size));
        } else {
            list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }

        let (header_rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            pos2(header_rect.left() + 12.0, header_rect.center().y),
            Align2::LEFT_CENTER,
            "App",
            FontId::proportional(11.0),
            th::DIM,
        );
        painter.text(
            pos2(header_rect.right() - 14.0, header_rect.center().y),
            Align2::RIGHT_CENTER,
            "Folder size",
            FontId::proportional(11.0),
            th::DIM,
        );

        let body_h = ui.available_height().max(80.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body_h),
            Layout::top_down(Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("apps")
                    .auto_shrink([false, false])
                    .show_rows(ui, 46.0, list.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for i in range {
                            let app = list[i];
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 46.0),
                                Sense::click(),
                            );
                            let selected = app
                                .location
                                .as_ref()
                                .map(|p| {
                                    self.selected
                                        .as_deref()
                                        .map(|s| same_path(s, p))
                                        .unwrap_or(false)
                                })
                                .unwrap_or(false);
                            if selected {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::SELECT,
                                );
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(vec2(0.0, 1.0)),
                                    CornerRadius::same(7),
                                    th::PANEL_HI,
                                );
                            }
                            let p = ui.painter();
                            let sub = [app.publisher.as_str(), app.version.as_str()]
                                .iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("  ·  ");
                            let clip = p.with_clip_rect(rect.shrink2(vec2(12.0, 2.0)));
                            clip.text(
                                pos2(rect.left() + 12.0, rect.top() + 8.0),
                                Align2::LEFT_TOP,
                                th::truncate(&app.name, rect.width() - 150.0, 13.0),
                                FontId::proportional(13.0),
                                th::TEXT,
                            );
                            let location = app
                                .location
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| "—".to_owned());
                            let line = if sub.is_empty() {
                                location
                            } else {
                                format!("{sub}  ·  {location}")
                            };
                            clip.text(
                                pos2(rect.left() + 12.0, rect.top() + 26.0),
                                Align2::LEFT_TOP,
                                th::truncate(&line, rect.width() - 150.0, 10.5),
                                FontId::proportional(10.5),
                                th::DIM,
                            );
                            p.text(
                                pos2(rect.right() - 14.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                format!(
                                    "{}{}",
                                    th::fmt_bytes(app.size),
                                    if app.estimated { " est." } else { "" }
                                ),
                                FontId::proportional(12.5),
                                th::TEXT,
                            );
                            if resp.clicked() {
                                self.selected = app.location.clone();
                            }
                            if resp.double_clicked() {
                                if let Some(loc) = &app.location {
                                    open_path(loc);
                                }
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    });
            },
        );
    }

    // --------------------------------------------------------------- dialogs

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = self.confirm.clone() else { return };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut delete = false;
        let mut cancel = false;
        egui::Window::new("Move to Recycle Bin?")
            .id(egui::Id::unique("confirm_delete"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(
                    RichText::new(&name)
                        .family(th::semibold_family())
                        .size(15.0)
                        .color(th::TEXT),
                );
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(10.5)
                        .color(th::MUTED),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "It goes to the Recycle Bin, so you can restore it. DiskScope will rescan afterwards.",
                    )
                    .size(12.0)
                    .color(th::MUTED),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.add(th::danger_button("Move to Recycle Bin")).clicked() {
                        delete = true;
                    }
                    if ui.add(th::ghost_button("Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.confirm = None;
        }
        if delete {
            self.confirm = None;
            match trash::delete(&path) {
                Ok(_) => {
                    self.toast(format!("Moved “{name}” to the Recycle Bin"));
                    let root = self
                        .tree
                        .as_ref()
                        .map(|t| t.root_node().path.clone())
                        .unwrap_or_else(|| PathBuf::from("C:\\"));
                    let parent = path.parent().map(|p| p.to_path_buf());
                    self.start_scan(root, parent);
                }
                Err(err) => self.toast(format!("Couldn't delete: {err}")),
            }
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((message, at)) = &self.toast_msg else {
            return;
        };
        if at.elapsed() > Duration::from_secs(4) {
            self.toast_msg = None;
            return;
        }
        let message = message.clone();
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -18.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(th::PANEL_HI)
                    .stroke(Stroke::new(1.0, th::BORDER))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(message).size(12.5).color(th::TEXT));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl eframe::App for DiskScope {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_job();
        self.poll_apps();

        if self.last_drive_refresh.elapsed() > Duration::from_secs(15) && self.job.is_none() {
            self.refresh_drives();
        }

        if self.job.is_none()
            && ctx.memory(|m| m.focused().is_none())
            && ctx.input(|i| i.key_pressed(egui::Key::Backspace))
        {
            self.up();
        }

        self.top_bar(ui);
        self.side_bar(ui);
        self.central(ui);
        self.confirm_dialog(&ctx);
        self.draw_toast(&ctx);

        if self.job.is_some() || self.toast_msg.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}

// ------------------------------------------------------------------ helpers

fn tabs(ui: &mut egui::Ui, view: &mut View) {
    Frame::new()
        .fill(Color32::from_rgb(15, 20, 28))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (value, label) in [
                    (View::Explorer, "Explorer"),
                    (View::Largest, "Largest files"),
                    (View::Apps, "Apps"),
                ] {
                    let selected = *view == value;
                    let button = egui::Button::new(
                        RichText::new(label)
                            .size(12.5)
                            .color(if selected { th::TEXT } else { th::MUTED }),
                    )
                    .fill(if selected {
                        th::WIDGET_ACTIVE
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(7))
                    .min_size(vec2(0.0, 26.0));
                    if ui.add(button).clicked() {
                        *view = value;
                    }
                }
            });
        });
}

fn paint_logo(painter: &egui::Painter, rect: Rect, hole: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    painter.circle_filled(c, r, th::ACCENT);
    painter.circle_filled(c, r * 0.54, hole);
    painter.circle_filled(c + vec2(r * 0.52, -r * 0.52), r * 0.2, th::GREEN);
}

fn drive_card(ui: &mut egui::Ui, drive: &Drive) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 66.0), Sense::click());
    let painter = ui.painter();
    let bg = if resp.hovered() { th::PANEL_HI } else { th::PANEL };
    painter.rect_filled(rect, CornerRadius::same(10), bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, th::BORDER_SOFT),
        StrokeKind::Inside,
    );
    let badge = Rect::from_min_size(rect.min + vec2(10.0, 12.0), vec2(34.0, 34.0));
    painter.rect_filled(badge, CornerRadius::same(8), th::WIDGET);
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        &drive.letter,
        FontId::proportional(13.0),
        th::TEXT,
    );
    painter.text(
        pos2(rect.left() + 52.0, rect.top() + 11.0),
        Align2::LEFT_TOP,
        th::truncate(&drive.name(), rect.width() - 62.0, 12.5),
        FontId::proportional(12.5),
        th::TEXT,
    );
    painter.text(
        pos2(rect.right() - 10.0, rect.top() + 12.0),
        Align2::RIGHT_TOP,
        format!("{} free", th::fmt_bytes(drive.free)),
        FontId::proportional(10.0),
        th::MUTED,
    );
    let bar = Rect::from_min_size(
        pos2(rect.left() + 52.0, rect.top() + 33.0),
        vec2(rect.width() - 62.0, 5.0),
    );
    th::paint_usage_bar(painter, bar, drive.used(), drive.total);
    painter.text(
        pos2(rect.left() + 52.0, rect.top() + 43.0),
        Align2::LEFT_TOP,
        format!(
            "{} of {} used",
            th::fmt_bytes(drive.used()),
            th::fmt_bytes(drive.total)
        ),
        FontId::proportional(10.0),
        th::DIM,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn drive_chip(ui: &mut egui::Ui, drive: &Drive) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(210.0, 74.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(12),
        if resp.hovered() { th::PANEL_HI } else { th::PANEL },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, th::BORDER_SOFT),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.left() + 14.0, rect.top() + 12.0),
        Align2::LEFT_TOP,
        th::truncate(&drive.name(), rect.width() - 28.0, 12.5),
        FontId::proportional(12.5),
        th::TEXT,
    );
    painter.text(
        pos2(rect.left() + 14.0, rect.top() + 30.0),
        Align2::LEFT_TOP,
        format!("{} free", th::fmt_bytes(drive.free)),
        FontId::proportional(10.5),
        th::MUTED,
    );
    let bar = Rect::from_min_size(
        pos2(rect.left() + 14.0, rect.top() + 50.0),
        vec2(rect.width() - 28.0, 5.0),
    );
    th::paint_usage_bar(painter, bar, drive.used(), drive.total);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn nav_row(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), th::SELECT);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), th::PANEL_HI);
    }
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        th::truncate(label, rect.width() - 20.0, 12.5),
        FontId::proportional(12.5),
        if active { th::TEXT } else { th::MUTED },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn chip(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(th::WIDGET)
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(9, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(th::MUTED));
        });
}

fn stat_block(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(value)
                .family(th::semibold_family())
                .size(20.0)
                .color(th::TEXT),
        );
        ui.label(RichText::new(label).size(10.5).color(th::DIM));
    });
}

struct Columns {
    name_right: f32,
    size_right: f32,
    pct_right: f32,
    items_right: f32,
    type_right: f32,
}

fn columns(rect: Rect) -> Columns {
    let type_right = rect.right() - 14.0;
    let items_right = type_right - 58.0;
    let pct_right = items_right - 76.0;
    let size_right = pct_right - 78.0;
    let name_right = size_right - 98.0;
    Columns {
        name_right,
        size_right,
        pct_right,
        items_right,
        type_right,
    }
}

fn paint_header(painter: &egui::Painter, rect: Rect, sort: SortKey, asc: bool) {
    let c = columns(rect);
    let cy = rect.center().y;
    let font = FontId::proportional(11.0);
    let arrow = if asc { " ↑" } else { " ↓" };
    let label = |key: SortKey, text: &str| {
        if key == sort {
            format!("{text}{arrow}")
        } else {
            text.to_owned()
        }
    };
    painter.text(
        pos2(rect.left() + 34.0, cy),
        Align2::LEFT_CENTER,
        label(SortKey::Name, "Name"),
        font.clone(),
        th::DIM,
    );
    painter.text(
        pos2(c.size_right, cy),
        Align2::RIGHT_CENTER,
        label(SortKey::Size, "Size"),
        font.clone(),
        th::DIM,
    );
    painter.text(
        pos2(c.pct_right, cy),
        Align2::RIGHT_CENTER,
        "% of folder",
        font.clone(),
        th::DIM,
    );
    painter.text(
        pos2(c.items_right, cy),
        Align2::RIGHT_CENTER,
        label(SortKey::Items, "Items"),
        font.clone(),
        th::DIM,
    );
    painter.text(
        pos2(c.type_right, cy),
        Align2::RIGHT_CENTER,
        "Type",
        font,
        th::DIM,
    );
}

fn paint_row(painter: &egui::Painter, rect: Rect, row: &Row, total: u64) {
    let c = columns(rect);
    let cy = rect.center().y;

    // icon
    if row.is_dir {
        let base = pos2(rect.left() + 12.0, cy - 6.0);
        painter.rect_filled(
            Rect::from_min_size(base + vec2(0.0, 2.0), vec2(15.0, 11.0)),
            CornerRadius::same(2),
            th::ACCENT_SOFT,
        );
        painter.rect_filled(
            Rect::from_min_size(base, vec2(8.0, 5.0)),
            CornerRadius::same(2),
            th::ACCENT_SOFT,
        );
    } else {
        let base = pos2(rect.left() + 14.0, cy - 7.0);
        painter.rect_stroke(
            Rect::from_min_size(base, vec2(11.0, 14.0)),
            CornerRadius::same(2),
            Stroke::new(1.2, th::MUTED),
            StrokeKind::Inside,
        );
    }

    let clip = painter.with_clip_rect(Rect::from_min_max(
        pos2(rect.left() + 32.0, rect.top()),
        pos2(c.name_right, rect.bottom()),
    ));
    clip.text(
        pos2(rect.left() + 34.0, cy),
        Align2::LEFT_CENTER,
        th::truncate(&row.name, c.name_right - rect.left() - 40.0, 12.8),
        FontId::proportional(12.8),
        if row.is_dir {
            th::TEXT
        } else {
            Color32::from_rgb(205, 214, 226)
        },
    );
    painter.text(
        pos2(c.size_right, cy),
        Align2::RIGHT_CENTER,
        th::fmt_bytes(row.size),
        FontId::proportional(12.0),
        th::TEXT,
    );
    painter.text(
        pos2(c.pct_right, cy),
        Align2::RIGHT_CENTER,
        th::pct(row.size, total),
        FontId::proportional(11.5),
        th::MUTED,
    );
    painter.text(
        pos2(c.items_right, cy),
        Align2::RIGHT_CENTER,
        if row.is_dir {
            th::fmt_num(row.items)
        } else {
            "—".to_owned()
        },
        FontId::proportional(11.5),
        th::MUTED,
    );
    let kind = if row.is_dir {
        "Folder".to_owned()
    } else {
        row.name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_uppercase())
            .filter(|ext| ext.len() <= 6 && !ext.is_empty())
            .unwrap_or_else(|| "File".to_owned())
    };
    painter.text(
        pos2(c.type_right, cy),
        Align2::RIGHT_CENTER,
        kind,
        FontId::proportional(11.0),
        th::DIM,
    );
}

fn draw_tooltip(
    painter: &egui::Painter,
    screen: Rect,
    pointer: egui::Pos2,
    title: &str,
    detail: &str,
) {
    let width = 280.0;
    let height = 58.0;
    let mut pos = pointer + vec2(16.0, 18.0);
    if pos.x + width > screen.right() - 8.0 {
        pos.x = pointer.x - width - 16.0;
    }
    if pos.y + height > screen.bottom() - 8.0 {
        pos.y = pointer.y - height - 14.0;
    }
    let rect = Rect::from_min_size(pos, vec2(width, height));
    painter.rect_filled(rect, CornerRadius::same(9), Color32::from_rgb(20, 26, 36));
    painter.rect_stroke(
        rect,
        CornerRadius::same(9),
        Stroke::new(1.0, th::BORDER),
        StrokeKind::Inside,
    );
    let clip = painter.with_clip_rect(rect.shrink(8.0));
    clip.text(
        pos2(rect.left() + 12.0, rect.top() + 10.0),
        Align2::LEFT_TOP,
        th::truncate(title, width - 24.0, 12.5),
        FontId::proportional(12.5),
        th::TEXT,
    );
    clip.text(
        pos2(rect.left() + 12.0, rect.top() + 31.0),
        Align2::LEFT_TOP,
        th::truncate(detail, width - 24.0, 11.0),
        FontId::proportional(11.0),
        th::MUTED,
    );
}

fn breadcrumbs(ui: &mut egui::Ui, tree: &Tree, current: usize) -> Option<usize> {
    let mut chain = Vec::new();
    let mut id = Some(current);
    while let Some(i) = id {
        chain.push(i);
        id = tree.node(i).parent;
    }
    chain.reverse();

    let mut target = None;
    Frame::new()
        .fill(th::PANEL_HI)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (i, &node_id) in chain.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("›").size(12.0).color(th::DIM));
                    }
                    let last = i == chain.len() - 1;
                    let label = egui::Label::new(
                        RichText::new(&tree.node(node_id).name)
                            .size(12.0)
                            .color(if last { th::TEXT } else { th::MUTED }),
                    )
                    .sense(if last {
                        Sense::hover()
                    } else {
                        Sense::click()
                    });
                    let resp = ui.add(label);
                    if !last && resp.clicked() {
                        target = Some(node_id);
                    }
                    if !last && resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
            });
        });
    target
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn normalize_root(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy().to_string();
    if text.len() == 2 && text.ends_with(':') {
        return PathBuf::from(format!("{text}\\"));
    }
    path
}

fn open_path(path: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

fn reveal_path(path: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

fn quick_access() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let base = PathBuf::from(&profile);
        if base.is_dir() {
            out.push(("Home".to_owned(), base.clone()));
            for (label, name) in [
                ("Downloads", "Downloads"),
                ("Documents", "Documents"),
                ("Desktop", "Desktop"),
                ("Pictures", "Pictures"),
                ("Videos", "Videos"),
                ("Music", "Music"),
            ] {
                let path = base.join(name);
                if path.is_dir() {
                    out.push((label.to_owned(), path));
                }
            }
        }
    }

    for (label, path) in [
        ("Program Files", "C:\\Program Files"),
        ("Program Files (x86)", "C:\\Program Files (x86)"),
        ("AppData (local)", ""),
        ("Windows", "C:\\Windows"),
    ] {
        let path = if label == "AppData (local)" {
            std::env::var("LOCALAPPDATA").map(PathBuf::from).ok()
        } else {
            Some(PathBuf::from(path))
        };
        if let Some(path) = path {
            if path.is_dir() {
                out.push((label.to_owned(), path));
            }
        }
    }
    out
}

#[cfg(windows)]
fn list_drives() -> Vec<Drive> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetLogicalDrives, GetVolumeInformationW,
    };

    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = std::ffi::OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut name_buf = [0u16; 128];
        let mut fs_buf = [0u16; 64];
        let mut serial = 0u32;
        let mut max_len = 0u32;
        let mut flags = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                name_buf.as_mut_ptr(),
                name_buf.len() as u32,
                &mut serial,
                &mut max_len,
                &mut flags,
                fs_buf.as_mut_ptr(),
                fs_buf.len() as u32,
            )
        };
        let label = if ok != 0 {
            let end = name_buf.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&name_buf[..end])
        } else {
            String::new()
        };

        let mut free_to_caller = 0u64;
        let mut total = 0u64;
        let mut free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_to_caller, &mut total, &mut free)
        };
        if ok == 0 || total == 0 {
            continue;
        }
        out.push(Drive {
            letter: format!("{letter}:"),
            label,
            root: PathBuf::from(root),
            total,
            free: free_to_caller.min(total),
        });
    }
    out
}

#[cfg(not(windows))]
fn list_drives() -> Vec<Drive> {
    Vec::new()
}
