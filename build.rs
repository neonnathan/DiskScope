fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "DiskScope");
        res.set("FileDescription", "DiskScope - disk usage analyzer");
        res.set("LegalCopyright", "Copyright (c) 2026");
        if let Err(err) = res.compile() {
            println!("cargo:warning=icon/resource compile failed: {err}");
        }
    }
}
