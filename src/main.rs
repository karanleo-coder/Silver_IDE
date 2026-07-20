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
            adopt_shell_path();
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

/// Apps launched from Spotlight or the Dock get a bare PATH, so the
/// error checkers (dart, node, cargo, ...) would look missing. Borrow
/// the login shell's PATH once, before anything spawns tools.
fn adopt_shell_path() {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        if let Ok(out) = std::process::Command::new(&shell)
            .args(["-l", "-c", "printf %s \"$PATH\""])
            .output()
        {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let current = std::env::var("PATH").unwrap_or_default();
            if path.len() > current.len() {
                std::env::set_var("PATH", path);
            }
        }
    }
}

/// Create a minimal macOS app bundle that launches `silver_kb --app`,
/// so silver shows up in Spotlight and can live in the Dock. On Linux
/// it writes a desktop entry + icon instead, so app launchers list it.
fn install_app() {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        eprintln!("install-app supports macOS and Linux — run `silver_kb --app` instead");
    }
    #[cfg(target_os = "linux")]
    {
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
        let data = base.home_dir().join(".local/share");
        let icon_dir = data.join("icons/hicolor/256x256/apps");
        let apps_dir = data.join("applications");
        if let Err(e) =
            std::fs::create_dir_all(&icon_dir).and_then(|()| std::fs::create_dir_all(&apps_dir))
        {
            eprintln!("install-app: {e}");
            return;
        }
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Silver\nComment=a cozy, cat-powered IDE\nExec=\"{}\" --app\nIcon=silver\nTerminal=false\nCategories=Development;IDE;\n",
            exe.display()
        );
        let icon_png: &[u8] = include_bytes!("../assets/silver-256.png");
        if let Err(e) = std::fs::write(icon_dir.join("silver.png"), icon_png)
            .and_then(|()| std::fs::write(apps_dir.join("silver.desktop"), desktop))
        {
            eprintln!("install-app: {e}");
            return;
        }
        println!("installed the Silver app entry — find it in your app launcher");
        println!("(log out/in or run `update-desktop-database` if it doesn't show yet)");
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
        let res_dir = bundle.join("Contents/Resources");
        if let Err(e) = std::fs::create_dir_all(&macos_dir)
            .and_then(|()| std::fs::create_dir_all(&res_dir))
        {
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
    <key>CFBundleIconFile</key><string>silver</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
            version = env!("CARGO_PKG_VERSION"),
        );
        let launcher = format!("#!/bin/sh\nexec \"{}\" --app\n", exe.display());

        let plist_path = bundle.join("Contents/Info.plist");
        let bin_path = macos_dir.join("silver");
        // The silver logo, baked into the binary so the bundle is
        // complete wherever the executable travels.
        let icns: &[u8] = include_bytes!("../assets/silver.icns");
        if let Err(e) = std::fs::write(&plist_path, plist)
            .and_then(|()| std::fs::write(&bin_path, launcher))
            .and_then(|()| std::fs::write(res_dir.join("silver.icns"), icns))
        {
            eprintln!("install-app: {e}");
            return;
        }
        // Nudge Finder/Spotlight to notice the (possibly new) icon.
        let _ = std::process::Command::new("touch").arg(&bundle).status();
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
