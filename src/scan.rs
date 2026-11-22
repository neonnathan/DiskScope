//! Disk scanning: walks a folder tree in parallel and builds a compact,
//! navigation-friendly arena of directory nodes.
//!
//! The walk uses `std::fs::read_dir` rather than a generic walker on purpose:
//! on Windows the size comes free with the directory enumeration, while
//! path-based `metadata()` calls cost a full handle open per file (measured:
//! 11 s vs 0.5 s for 178k files under `C:\Windows`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use std::os::windows::fs::MetadataExt;

/// A file big enough to be worth listing globally.
#[derive(Debug, Clone)]
pub struct FileHit {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    /// Bytes of files directly inside this folder (excludes subfolders).
    pub own: u64,
    /// Number of files directly inside this folder.
    pub files: u64,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Largest files directly inside this folder: (name, size).
    pub top_files: Vec<(String, u64)>,
}

#[derive(Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
    pub total_files: u64,
    pub total_dirs: u64,
    pub errors: u64,
    pub top_files: Vec<FileHit>,
    pub elapsed: f64,
}

impl Tree {
    pub fn node(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[self.root]
    }

    /// Find a node by absolute path by walking down from the scan root.
    pub fn find(&self, path: &Path) -> Option<usize> {
        let root_path = self.nodes[self.root].path.to_string_lossy().to_lowercase();
        let target = path.to_string_lossy().to_lowercase();
        let target = target.trim_end_matches(['\\', '/']);
        let root_trimmed = root_path.trim_end_matches(['\\', '/']);
        if target.len() < root_trimmed.len() || !target.starts_with(root_trimmed) {
            return None;
        }
        let rest = &target[root_trimmed.len()..];
        if !rest.is_empty() && !rest.starts_with(['\\', '/']) {
            return None;
        }
        let mut id = self.root;
        for part in rest.split(['\\', '/']).filter(|p| !p.is_empty()) {
            let mut next = None;
            for &c in &self.nodes[id].children {
                if self.nodes[c].name.to_lowercase() == part {
                    next = Some(c);
                    break;
                }
            }
            id = next?;
        }
        Some(id)
    }
}

pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub current: Mutex<String>,
    pub cancelled: AtomicBool,
}

impl Progress {
    fn new() -> Self {
        Self {
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            current: Mutex::new(String::new()),
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> (u64, u64, u64, u64, String) {
        (
            self.files.load(Ordering::Relaxed),
            self.dirs.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
            self.current.lock().map(|s| s.clone()).unwrap_or_default(),
        )
    }
}

struct RawDir {
    path: Arc<Path>,
    parent: Option<usize>,
    size: u64,
    own: u64,
    file_count: u64,
    dir_count: u64,
    files: Vec<(String, u64)>,
}

/// What one worker found in one directory.
struct DirResult {
    path: PathBuf,
    own: u64,
    file_count: u64,
    top: Vec<(String, u64)>,
    subdirs: Vec<PathBuf>,
    big: Vec<FileHit>,
    errors: u64,
}

const PER_DIR_FILE_MIN: u64 = 512 * 1024;
const PER_DIR_FILE_CAP: usize = 300;
/// Cap before sorting, so a folder with a million mid-size files cannot blow up.
const PER_DIR_FILE_KEEP: usize = 4096;
const GLOBAL_FILE_MIN: u64 = 8 * 1024 * 1024;
const GLOBAL_FILE_CAP: usize = 3000;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

/// Start scanning `root` on a background thread.
/// The receiver yields `None` if the scan was cancelled.
pub fn spawn_scan(root: PathBuf) -> (Arc<Progress>, Receiver<Option<Tree>>) {
    let progress = Arc::new(Progress::new());
    let (tx, rx) = mpsc::channel();
    let prog = progress.clone();

    std::thread::Builder::new()
        .name("diskscope-scan".into())
        .spawn(move || {
            let flag = prog.clone();
            let tree = scan(root, prog);
            let cancelled = flag.cancelled.load(Ordering::Relaxed);
            let _ = tx.send(if cancelled { None } else { Some(tree) });
        })
        .ok();

    (progress, rx)
}

/// Shared work queue: directories to visit, plus how many are queued or in flight.
struct Pool {
    queue: Mutex<VecDeque<PathBuf>>,
    ready: Condvar,
    pending: AtomicUsize,
}

impl Pool {
    fn push_all(&self, dirs: Vec<PathBuf>) {
        if dirs.is_empty() {
            return;
        }
        self.pending.fetch_add(dirs.len(), Ordering::Relaxed);
        let mut q = self.queue.lock().unwrap();
        q.extend(dirs);
        drop(q);
        self.ready.notify_all();
    }

    fn pop(&self) -> Option<PathBuf> {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(d) = q.pop_front() {
                return Some(d);
            }
            if self.pending.load(Ordering::Relaxed) == 0 {
                return None;
            }
            q = self.ready.wait(q).unwrap();
        }
    }

    fn finish_one(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
        self.ready.notify_all();
    }

    fn abort(&self) {
        self.pending.store(0, Ordering::Relaxed);
        self.queue.lock().unwrap().clear();
        self.ready.notify_all();
    }
}

fn scan(root: PathBuf, prog: Arc<Progress>) -> Tree {
    let started = Instant::now();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 12);

    let pool = Arc::new(Pool {
        queue: Mutex::new(VecDeque::new()),
        ready: Condvar::new(),
        pending: AtomicUsize::new(1),
    });
    pool.queue.lock().unwrap().push_back(root.clone());

    let (tx, rx) = mpsc::channel::<DirResult>();
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let pool = pool.clone();
        let prog = prog.clone();
        let tx = tx.clone();
        handles.push(
            std::thread::Builder::new()
                .name("diskscope-walk".into())
                .spawn(move || worker(pool, prog, tx))
                .ok(),
        );
    }
    drop(tx);

    // ---------------------------------------------------------------- collect
    let root_arc: Arc<Path> = Arc::from(root.as_path());
    let mut map: HashMap<Arc<Path>, usize> = HashMap::new();
    map.insert(root_arc.clone(), 0);
    let mut dirs: Vec<RawDir> = vec![RawDir {
        path: root_arc,
        parent: None,
        size: 0,
        own: 0,
        file_count: 0,
        dir_count: 0,
        files: Vec::new(),
    }];
    let mut top_files: Vec<FileHit> = Vec::new();
    let mut files: u64 = 0;
    let mut errors: u64 = 0;

    for mut res in rx {
        files += res.file_count;
        errors += res.errors;
        top_files.append(&mut res.big);
        let Some(idx) = dir_index(&mut map, &mut dirs, &res.path) else {
            continue;
        };
        let d = &mut dirs[idx];
        d.own = d.own.saturating_add(res.own);
        d.file_count += res.file_count;
        d.files.append(&mut res.top);
    }

    for h in handles.into_iter().flatten() {
        let _ = h.join();
    }

    let cancelled = prog.cancelled.load(Ordering::Relaxed);
    prog.files.store(files, Ordering::Relaxed);
    prog.dirs.store(dirs.len() as u64, Ordering::Relaxed);
    prog.errors.store(errors, Ordering::Relaxed);

    if cancelled {
        return Tree {
            root: 0,
            total_files: files,
            total_dirs: dirs.len() as u64,
            errors,
            top_files: Vec::new(),
            elapsed: started.elapsed().as_secs_f64(),
            nodes: Vec::new(),
        };
    }

    if let Ok(mut cur) = prog.current.lock() {
        *cur = format!("Organising {} folders…", crate::theme::fmt_num(dirs.len() as u64));
    }

    // Count direct subdirectories and trim per-folder file lists.
    for i in 0..dirs.len() {
        let parent = dirs[i].parent;
        if let Some(p) = parent {
            dirs[p].dir_count += 1;
        }
        let d = &mut dirs[i];
        d.files.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        d.files.truncate(PER_DIR_FILE_CAP);
        d.size = d.own;
    }

    // Roll sizes up from the deepest folders. Depth is measured once: sorting
    // by `path.components().count()` in the comparator re-parsed every path on
    // every comparison, which is ruinous for a few hundred thousand folders.
    let depth: Vec<usize> = dirs
        .iter()
        .map(|d| d.path.components().count())
        .collect();
    let mut order: Vec<usize> = (0..dirs.len()).collect();
    order.sort_unstable_by_key(|&i| std::cmp::Reverse(depth[i]));
    for &i in &order {
        if let Some(p) = dirs[i].parent {
            dirs[p].size = dirs[p].size.saturating_add(dirs[i].size);
        }
    }

    top_files.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    top_files.truncate(GLOBAL_FILE_CAP);

    // Build the arena.
    let mut nodes: Vec<Node> = Vec::with_capacity(dirs.len());
    for d in &dirs {
        let name = if d.parent.is_none() {
            d.path.display().to_string()
        } else {
            d.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.path.display().to_string())
        };
        nodes.push(Node {
            name,
            path: d.path.to_path_buf(),
            size: d.size,
            own: d.own,
            files: d.file_count,
            parent: d.parent,
            children: Vec::new(),
            top_files: d.files.clone(),
        });
    }
    for i in 0..nodes.len() {
        let parent = nodes[i].parent;
        if let Some(p) = parent {
            nodes[p].children.push(i);
        }
    }
    let sizes: Vec<u64> = nodes.iter().map(|n| n.size).collect();
    for n in nodes.iter_mut() {
        n.children.sort_unstable_by(|a, b| sizes[*b].cmp(&sizes[*a]));
    }

    Tree {
        root: 0,
        total_files: files,
        total_dirs: dirs.len() as u64,
        errors,
        top_files,
        elapsed: started.elapsed().as_secs_f64(),
        nodes,
    }
}

fn worker(pool: Arc<Pool>, prog: Arc<Progress>, tx: Sender<DirResult>) {
    let mut visited: u64 = 0;
    loop {
        let Some(dir) = pool.pop() else { return };
        if prog.cancelled.load(Ordering::Relaxed) {
            pool.abort();
            return;
        }
        let mut res = read_one_dir(&dir);
        visited += 1;
        prog.files.fetch_add(res.file_count, Ordering::Relaxed);
        prog.bytes.fetch_add(res.own, Ordering::Relaxed);
        prog.dirs.fetch_add(1, Ordering::Relaxed);
        prog.errors.fetch_add(res.errors, Ordering::Relaxed);
        if visited % 16 == 0 {
            if let Ok(mut cur) = prog.current.lock() {
                *cur = dir.to_string_lossy().into_owned();
            }
        }
        pool.push_all(std::mem::take(&mut res.subdirs));
        let _ = tx.send(res);
        pool.finish_one();
    }
}

fn read_one_dir(dir: &Path) -> DirResult {
    let mut res = DirResult {
        path: dir.to_path_buf(),
        own: 0,
        file_count: 0,
        top: Vec::new(),
        subdirs: Vec::new(),
        big: Vec::new(),
        errors: 0,
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        res.errors = 1;
        return res;
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                res.errors += 1;
                continue;
            }
        };
        let Ok(ft) = entry.file_type() else {
            res.errors += 1;
            continue;
        };
        if ft.is_dir() {
            // Junctions and symlinked folders are not followed: they would
            // double-count and can form loops.
            if entry
                .metadata()
                .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
                .unwrap_or(false)
            {
                continue;
            }
            res.subdirs.push(entry.path());
        } else if ft.is_file() {
            // Free on Windows: the size arrived with the directory listing.
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            res.file_count += 1;
            res.own = res.own.saturating_add(size);
            if size >= PER_DIR_FILE_MIN && res.top.len() < PER_DIR_FILE_KEEP {
                let name = entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                res.top.push((name, size));
            }
            if size >= GLOBAL_FILE_MIN {
                res.big.push(FileHit {
                    path: entry.path(),
                    size,
                });
            }
        }
    }
    res
}

/// Look up a directory index, creating any missing ancestors. Workers deliver
/// results out of order, so a child can arrive before its parent is known.
fn dir_index(
    map: &mut HashMap<Arc<Path>, usize>,
    dirs: &mut Vec<RawDir>,
    path: &Path,
) -> Option<usize> {
    if let Some(&i) = map.get(path) {
        return Some(i);
    }
    let mut chain: Vec<&Path> = Vec::new();
    let mut cur = path;
    loop {
        if map.contains_key(cur) {
            break;
        }
        chain.push(cur);
        match cur.parent() {
            Some(p) => cur = p,
            None => break,
        }
    }
    for p in chain.into_iter().rev() {
        let parent_idx = p.parent().and_then(|pp| map.get(pp).copied());
        let arc: Arc<Path> = Arc::from(p);
        let idx = dirs.len();
        map.insert(arc.clone(), idx);
        dirs.push(RawDir {
            path: arc,
            parent: parent_idx,
            size: 0,
            own: 0,
            file_count: 0,
            dir_count: 0,
            files: Vec::new(),
        });
    }
    map.get(path).copied()
}

/// Quick recursive size of a single folder (used for installed apps).
/// Returns `None` if the walk ran out of budget before finishing, so callers
/// never mistake a partial walk for a real size.
pub fn dir_size(path: &Path, budget: &mut u64) -> Option<u64> {
    let mut total = 0u64;
    let rd = std::fs::read_dir(path).ok()?;
    for entry in rd.flatten() {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            total = total.saturating_add(dir_size(&entry.path(), budget)?);
        } else if let Ok(meta) = entry.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    Some(total)
}
