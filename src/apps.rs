//! Installed applications, read from the Windows uninstall registry keys.

use crate::scan::{dir_size, Tree};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub location: Option<PathBuf>,
    pub size: u64,
    /// True when the size came from the registry estimate, not a real folder walk.
    pub estimated: bool,
}

pub fn spawn_apps_scan(tree: Option<Arc<Tree>>) -> Receiver<Vec<AppEntry>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("diskscope-apps".into())
        .spawn(move || {
            let _ = tx.send(collect(tree));
        })
        .ok();
    rx
}

fn collect(tree: Option<Arc<Tree>>) -> Vec<AppEntry> {
    let mut out: Vec<AppEntry> = Vec::new();
    let mut cache: HashMap<String, Option<u64>> = HashMap::new();
    // Without a scan tree to answer from, cap how much disk walking we do.
    let mut total_budget: u64 = 6_000_000;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let mut scan = |root: &RegKey, path: &str, out: &mut Vec<AppEntry>| {
        read_uninstall(root, path, out, &tree, &mut cache, &mut total_budget);
    };
    scan(
        &hklm,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        &mut out,
    );
    scan(
        &hklm,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        &mut out,
    );
    scan(
        &hkcu,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        &mut out,
    );

    // De-duplicate (same app can be registered in more than one hive/view).
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut deduped: Vec<AppEntry> = Vec::new();
    for app in out {
        let key = (
            app.name.to_lowercase(),
            app.location
                .as_ref()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
        );
        if let Some(&i) = seen.get(&key) {
            if app.size > deduped[i].size {
                deduped[i] = app;
            }
        } else {
            seen.insert(key, deduped.len());
            deduped.push(app);
        }
    }

    deduped.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    deduped
}

fn read_uninstall(
    root: &RegKey,
    path: &str,
    out: &mut Vec<AppEntry>,
    tree: &Option<Arc<Tree>>,
    cache: &mut HashMap<String, Option<u64>>,
    total_budget: &mut u64,
) {
    let Ok(key) = root.open_subkey(path) else {
        return;
    };
    for sub in key.enum_keys().flatten() {
        let Ok(k) = key.open_subkey(&sub) else {
            continue;
        };
        let name: String = k.get_value("DisplayName").unwrap_or_default();
        let name = name.trim().to_owned();
        if name.is_empty() {
            continue;
        }
        let system: u32 = k.get_value("SystemComponent").unwrap_or(0);
        if system == 1 {
            continue;
        }
        if k.get_value::<String, _>("ParentKeyName").is_ok() {
            continue;
        }
        let release: String = k.get_value("ReleaseType").unwrap_or_default();
        if release.eq_ignore_ascii_case("update") || release.eq_ignore_ascii_case("hotfix") {
            continue;
        }
        if name.to_lowercase().starts_with("update for ") {
            continue;
        }

        let publisher: String = k.get_value("Publisher").unwrap_or_default();
        let version: String = k.get_value("DisplayVersion").unwrap_or_default();
        let est_kb: u32 = k.get_value("EstimatedSize").unwrap_or(0);
        let loc: String = k.get_value("InstallLocation").unwrap_or_default();
        let loc = expand_env(&loc);
        let loc = loc.trim().trim_matches('"').trim_end_matches('\\').to_owned();

        let location = if loc.is_empty() {
            None
        } else {
            let p = PathBuf::from(&loc);
            if p.is_dir() {
                Some(p)
            } else {
                None
            }
        };

        let measured = location.as_ref().and_then(|p| {
            let key = p.to_string_lossy().to_lowercase();
            if let Some(cached) = cache.get(&key) {
                return *cached;
            }
            let from_tree = tree
                .as_ref()
                .and_then(|t| t.find(p).map(|id| t.node(id).size))
                .filter(|size| *size > 0);
            let value = from_tree.or_else(|| {
                // Without an exact size from a scan, the registry estimate is
                // good enough for big apps. Only walk folders that have no
                // estimate at all, and never keep a partial walk.
                if est_kb > 0 {
                    return None;
                }
                let mut budget = (*total_budget).min(400_000);
                if budget == 0 {
                    return None;
                }
                let bytes = dir_size(p, &mut budget);
                *total_budget = total_budget.saturating_sub(budget);
                bytes.filter(|size| *size > 0)
            });
            cache.insert(key, value);
            value
        });

        let (size, estimated) = match measured {
            Some(bytes) if bytes > 0 => (bytes, false),
            _ => (est_kb as u64 * 1024, true),
        };

        out.push(AppEntry {
            name,
            publisher: publisher.trim().to_owned(),
            version: version.trim().to_owned(),
            location,
            size,
            estimated,
        });
    }
}

fn expand_env(value: &str) -> String {
    if !value.contains('%') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let var = &after[..end];
                match std::env::var(var) {
                    Ok(val) => out.push_str(&val),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
