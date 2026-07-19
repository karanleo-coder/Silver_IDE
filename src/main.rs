mod app;
mod cat;
mod commands;
mod config;
mod editor;
mod gui;
mod media;
mod ui;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "--app" | "app" | "-a" => {
            if let Err(e) = gui::run() {
                eprintln!("silver: could not open the app window: {e}");
                std::process::exit(1);
            }
        }
        "install-app" => install_app(),
        "--help" | "-h" | "help" => {
            println!("silver — a cozy, cat-powered IDE");
            println!();
            println!("  silver_kb              run in this terminal (TUI)");
            println!("  silver_kb --app        open as a native window");
            println!("  silver_kb install-app  create Silver.app in ~/Applications (macOS)");
        }
        _ => {
            let mut terminal = ratatui::init();
            let result = app::App::new().run(&mut terminal);
            ratatui::restore();
            // Hand the cursor back the way the terminal likes it.
            let _ = ratatui::crossterm::execute!(
                std::io::stdout(),
                ratatui::crossterm::cursor::SetCursorStyle::DefaultUserShape
            );
            if let Err(e) = result {
                eprintln!("silver: {e}");
                std::process::exit(1);
            }
        }
    }
}

/// Create a minimal macOS app bundle that launches `silver_kb --app`,
/// so silver shows up in Spotlight and can live in the Dock.
fn install_app() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("install-app currently supports macOS only — run `silver_kb --app` instead");
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;

        let exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("install-app: cannot locate the silver binary: {e}");
                return;
            }
        };
        let Some(base) = directories::BaseDirs::new() else {
            eprintln!("install-app: cannot find your home directory");
            return;
        };
        let bundle = base.home_dir().join("Applications/Silver.app");
        let macos_dir = bundle.join("Contents/MacOS");
        if let Err(e) = std::fs::create_dir_all(&macos_dir) {
            eprintln!("install-app: {e}");
            return;
        }

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Silver</string>
    <key>CFBundleDisplayName</key><string>Silver</string>
    <key>CFBundleIdentifier</key><string>dev.silver.silver-cli</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>silver</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
            version = env!("CARGO_PKG_VERSION"),
        );
        let launcher = format!("#!/bin/sh\nexec \"{}\" --app\n", exe.display());

        let plist_path = bundle.join("Contents/Info.plist");
        let bin_path = macos_dir.join("silver");
        if let Err(e) = std::fs::write(&plist_path, plist)
            .and_then(|()| std::fs::write(&bin_path, launcher))
        {
            eprintln!("install-app: {e}");
            return;
        }
        if let Ok(meta) = std::fs::metadata(&bin_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&bin_path, perms);
        }
        println!("installed {}", bundle.display());
        println!("find it in Spotlight as “Silver”, or open it now with:");
        println!("  open ~/Applications/Silver.app");
        println!("(re-run `silver_kb install-app` after moving the binary)");
    }
}
