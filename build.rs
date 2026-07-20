fn main() {
    // On Windows, stamp the silver logo into the .exe so Explorer,
    // the Start menu, and the taskbar all show it.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/silver.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource skipped: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/silver.ico");
}
