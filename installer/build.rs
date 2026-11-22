fn main() {
    // The installer embeds the built app, so a fresh clone has to build the app
    // first. Fail with an instruction instead of a cryptic include_bytes! error.
    let app = std::path::Path::new("../dist/DiskScope.exe");
    if !app.exists() {
        panic!(
            "dist\\DiskScope.exe is missing.\n\
             The installer embeds the app binary, so build it first:\n\
             \n    .\\build.ps1        (from the repository root)\n"
        );
    }
    println!("cargo:rerun-if-changed=../dist/DiskScope.exe");

    let mut res = winres::WindowsResource::new();
    res.set_icon("../assets/icon.ico");
    res.set("ProductName", "DiskScope");
    res.set("FileDescription", "DiskScope Setup");
    if let Err(err) = res.compile() {
        println!("cargo:warning=icon/resource compile failed: {err}");
    }
}
