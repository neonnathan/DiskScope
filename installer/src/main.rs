//! DiskScope installer / uninstaller.
//!
//! A single self-contained Setup.exe: it embeds the app binary, copies it to
//! %LOCALAPPDATA%\Programs\DiskScope, creates shortcuts, registers itself in
//! "Apps & features", and can remove everything again with `--uninstall`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use winreg::enums::*;
use winreg::RegKey;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const APP_BYTES: &[u8] = include_bytes!("../../dist/DiskScope.exe");
const APP_NAME: &str = "DiskScope";
const VERSION: &str = "1.0.0";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DiskScope";

const MB_OK: u32 = 0x0000_0000;
const MB_YESNO: u32 = 0x0000_0004;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const MB_ICONQUESTION: u32 = 0x0000_0020;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const IDYES: i32 = 6;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--uninstall") {
        uninstall();
    } else {
        install();
    }
}

fn install() {
    let Some(dir) = install_dir() else {
        error("Could not find %LOCALAPPDATA%.");
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        error(&format!("Could not create {}:\n{err}", dir.display()));
        return;
    }

    let exe = dir.join("DiskScope.exe");
    if let Err(err) = std::fs::write(&exe, APP_BYTES) {
        error(&format!(
            "Could not write:\n{}\n\n{err}\n\nIf DiskScope is open, close it and run Setup again.",
            exe.display()
        ));
        return;
    }

    let uninstaller = dir.join("Uninstall.exe");
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::copy(me, &uninstaller);
    }

    let mut shortcuts = Vec::new();
    if let Some(start_menu) = start_menu_dir() {
        shortcuts.push(start_menu.join("DiskScope.lnk"));
    }
    if let Some(desktop) = desktop_dir() {
        shortcuts.push(desktop.join("DiskScope.lnk"));
    }
    for link in &shortcuts {
        make_shortcut(link, &exe, &dir);
    }

    if let Err(err) = write_registry(&dir, &exe, &uninstaller) {
        error(&format!("Installed the files, but could not register the app:\n{err}"));
    }

    info(&format!(
        "DiskScope {VERSION} is installed.\n\n{}\n\nStarting DiskScope now.",
        dir.display()
    ));
    let _ = Command::new(&exe).spawn();
}

fn uninstall() {
    let Some(dir) = install_dir() else {
        error("Could not find %LOCALAPPDATA%.");
        return;
    };
    let answer = message(
        &format!(
            "Remove DiskScope from this PC?\n\nThis deletes the app, its shortcuts and its registry entry.\nYour own files are not touched.\n\n{}",
            dir.display()
        ),
        "Uninstall DiskScope",
        MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND,
    );
    if answer != IDYES {
        return;
    }

    let exe = dir.join("DiskScope.exe");
    let _ = std::fs::remove_file(&exe);
    if exe.exists() {
        // The app is still open: stop it, then try once more.
        let _ = Command::new("taskkill")
            .args(["/IM", "DiskScope.exe", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let _ = std::fs::remove_file(&exe);
    }
    for link in shortcut_paths() {
        let _ = std::fs::remove_file(link);
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);

    // Tell the user first: this dialog blocks, and the folder cannot be deleted
    // until this process has exited, so the cleanup is spawned afterwards.
    info("DiskScope was removed.");

    // Retry in the background until the folder is gone: this process still has
    // Uninstall.exe open, so the first attempts are expected to fail.
    let target = dir.display().to_string().replace('\'', "''");
    let ps = format!(
        "for($i=0;$i -lt 40;$i++){{ Remove-Item -LiteralPath '{target}' -Recurse -Force -ErrorAction SilentlyContinue; if(-not (Test-Path -LiteralPath '{target}')){{break}}; Start-Sleep -Milliseconds 500 }}"
    );
    let _ = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn install_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Programs").join(APP_NAME))
}

fn start_menu_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn desktop_dir() -> Option<PathBuf> {
    let profile = std::env::var_os("USERPROFILE")?;
    let dir = PathBuf::from(profile).join("Desktop");
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

fn shortcut_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = start_menu_dir() {
        out.push(dir.join("DiskScope.lnk"));
    }
    if let Some(dir) = desktop_dir() {
        out.push(dir.join("DiskScope.lnk"));
    }
    out
}

fn make_shortcut(link: &Path, target: &Path, workdir: &Path) {
    let quote = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell\r\n\
         $s = $ws.CreateShortcut('{}')\r\n\
         $s.TargetPath = '{}'\r\n\
         $s.WorkingDirectory = '{}'\r\n\
         $s.IconLocation = '{}'\r\n\
         $s.Description = 'DiskScope - disk usage analyzer'\r\n\
         $s.Save()\r\n",
        quote(link),
        quote(target),
        quote(workdir),
        quote(target)
    );
    let tmp = std::env::temp_dir().join("diskscope_shortcut.ps1");
    if std::fs::write(&tmp, script).is_ok() {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
            ])
            .arg(&tmp)
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let _ = std::fs::remove_file(&tmp);
    }
}

fn write_registry(dir: &Path, exe: &Path, uninstaller: &Path) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(UNINSTALL_KEY)?;
    key.set_value("DisplayName", &"DiskScope")?;
    key.set_value("DisplayVersion", &VERSION)?;
    key.set_value("Publisher", &"Nathan")?;
    key.set_value("DisplayIcon", &exe.to_string_lossy().to_string())?;
    key.set_value("InstallLocation", &dir.to_string_lossy().to_string())?;
    key.set_value(
        "UninstallString",
        &format!("\"{}\" --uninstall", uninstaller.display()),
    )?;
    key.set_value(
        "QuietUninstallString",
        &format!("\"{}\" --uninstall", uninstaller.display()),
    )?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    key.set_value("EstimatedSize", &((APP_BYTES.len() / 1024) as u32))?;
    Ok(())
}

fn message(text: &str, title: &str, flags: u32) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
    let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            flags,
        )
    }
}

fn info(text: &str) {
    message(text, "DiskScope", MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND);
}

fn error(text: &str) {
    message(text, "DiskScope", MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
}
