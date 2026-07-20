use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

/// One problem found in a file. `line` is 0-based to match the buffer.
#[derive(Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
    pub warning: bool,
}

/// What a finished background check sends home.
pub struct CheckResult {
    pub path: PathBuf,
    pub diags: Vec<Diagnostic>,
    /// Set when the checker itself could not run (tool missing, etc).
    pub failed: Option<String>,
    /// Live checks stay quiet: no toast, just updated marks.
    pub quiet: bool,
    /// The buffer revision a live check saw; stale results are dropped.
    pub rev: u64,
    /// Problems the tool reported in *other* files (cargo checks the
    /// whole project), so "this file is fine" is never a false calm.
    pub others: usize,
}

/// Names live snapshot files so parallel checks never collide.
static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Checker commands to try for a file, best tool first. Extras like
/// shellcheck or pyflakes are used when the machine has them and
/// silently skipped when it doesn't — everything has a fallback that
/// ships with the language itself, so there is nothing to install.
fn check_plans(path: &Path, root: &Path) -> Vec<Vec<String>> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let p = path.to_string_lossy().to_string();
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match ext.as_str() {
        "rs" => {
            if root.join("Cargo.toml").exists() {
                vec![s(&["cargo", "check", "--message-format=short"])]
            } else {
                let out = std::env::temp_dir().join("silver_check");
                let _ = std::fs::create_dir_all(&out);
                vec![vec![
                    "rustc".into(),
                    "--edition=2021".into(),
                    "--error-format=short".into(),
                    "--emit=metadata".into(),
                    "--out-dir".into(),
                    out.to_string_lossy().to_string(),
                    p,
                ]]
            }
        }
        "py" => vec![
            vec!["python3".into(), "-m".into(), "pyflakes".into(), p.clone()],
            // Fallback: parse-only, and unlike py_compile it leaves
            // no __pycache__ droppings next to the user's file.
            vec![
                "python3".into(),
                "-c".into(),
                "import ast,sys; ast.parse(open(sys.argv[1]).read(), sys.argv[1])".into(),
                p,
            ],
        ],
        "js" | "mjs" => vec![vec!["node".into(), "--check".into(), p]],
        "c" => vec![vec!["cc".into(), "-fsyntax-only".into(), p]],
        "cpp" | "cc" | "cxx" => vec![vec!["c++".into(), "-fsyntax-only".into(), p]],
        "sh" | "bash" => vec![
            vec!["shellcheck".into(), "-f".into(), "gcc".into(), p.clone()],
            vec!["bash".into(), "-n".into(), p],
        ],
        "zsh" => vec![vec!["zsh".into(), "-n".into(), p]],
        "dart" => vec![
            vec!["dart".into(), "analyze".into(), p.clone()],
            vec!["flutter".into(), "analyze".into(), p],
        ],
        _ => Vec::new(),
    }
}

/// True when this kind of file can be checked at all.
pub fn checkable(path: &Path, root: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ext == "toml" || !check_plans(path, root).is_empty()
}

/// A snapshot check needs no project context around the file, so a
/// temp copy of the unsaved buffer gives honest results. Cargo files
/// are the exception: they only get checked against the saved file.
fn snapshot_ok(ext: &str, root: &Path) -> bool {
    match ext {
        "py" | "js" | "mjs" | "sh" | "bash" | "zsh" | "c" | "cpp" | "cc" | "cxx" | "toml" => true,
        "rs" => !root.join("Cargo.toml").exists(),
        // A temp copy of a package file loses its imports, so project
        // dart files only check against the saved file.
        "dart" => !root.join("pubspec.yaml").exists(),
        _ => false,
    }
}

/// Run candidate commands until one works; Ok(combined output).
fn run_plans(plans: &[Vec<String>], cwd: &Path) -> Result<String, String> {
    let mut spawn_err: Option<String> = None;
    for cmd in plans {
        match Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
        {
            Ok(o) => {
                let text = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                // pyflakes not installed: fall through to the next plan.
                if text.contains("No module named pyflakes") {
                    continue;
                }
                return Ok(text);
            }
            Err(e) => {
                // Tool missing (e.g. no shellcheck): try the fallback.
                if spawn_err.is_none() {
                    spawn_err = Some(format!("{}: {e}", cmd[0]));
                }
            }
        }
    }
    Err(spawn_err.unwrap_or_else(|| "no checker".into()))
}

/// Check the saved file in a background thread; the result arrives on
/// `tx`. Returns false when no checker exists for this kind of file.
/// `quiet` results update the marks without a toast (used on open).
/// The editor never blocks: typing stays instant while the tool runs.
pub fn spawn_check(path: &Path, root: &Path, quiet: bool, tx: Sender<CheckResult>) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // TOML is parsed right here with the crate silver already ships.
    if ext == "toml" {
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let diags = toml_diags(&text);
            let _ = tx.send(CheckResult { path, diags, failed: None, quiet, rev: 0, others: 0 });
        });
        return true;
    }
    let plans = check_plans(path, root);
    if plans.is_empty() {
        return false;
    }
    let path = path.to_path_buf();
    let cwd = root.to_path_buf();
    std::thread::spawn(move || {
        let fname =
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let result = match run_plans(&plans, &cwd) {
            Ok(text) => {
                let diags = parse_output(&text, &fname);
                let others = count_other_file_errors(&text, &fname);
                CheckResult { path, diags, failed: None, quiet, rev: 0, others }
            }
            Err(e) => CheckResult {
                path,
                diags: Vec::new(),
                failed: Some(e),
                quiet,
                rev: 0,
                others: 0,
            },
        };
        let _ = tx.send(result);
    });
    true
}

/// Check unsaved buffer contents: the text goes to a temp file, the
/// checker runs on that, and the marks come back against the real
/// path. Quiet — used for the as-you-type checks. Returns false when
/// this kind of file can't be checked from a snapshot.
pub fn spawn_snapshot(
    content: String,
    path: &Path,
    root: &Path,
    rev: u64,
    tx: Sender<CheckResult>,
) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !snapshot_ok(&ext, root) {
        return false;
    }
    let path = path.to_path_buf();
    let cwd = root.to_path_buf();
    std::thread::spawn(move || {
        let diags = if ext == "toml" {
            toml_diags(&content)
        } else {
            let n = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
            let tmp = std::env::temp_dir().join(format!("silver_live_{n}.{ext}"));
            if std::fs::write(&tmp, &content).is_err() {
                return;
            }
            let plans = check_plans(&tmp, &cwd);
            let fname =
                tmp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let diags = match run_plans(&plans, &cwd) {
                Ok(text) => parse_output(&text, &fname),
                Err(_) => Vec::new(), // quietly: a toast per keystroke would be noise
            };
            let _ = std::fs::remove_file(&tmp);
            diags
        };
        let _ = tx.send(CheckResult { path, diags, failed: None, quiet: true, rev, others: 0 });
    });
    true
}

/// TOML is validated in-process — no external tool at all.
fn toml_diags(text: &str) -> Vec<Diagnostic> {
    match toml::from_str::<toml::Value>(text) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let msg = e.to_string();
            let line = msg
                .find("line ")
                .and_then(|i| {
                    msg[i + 5..]
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<usize>()
                        .ok()
                })
                .unwrap_or(1);
            let short = msg.lines().last().unwrap_or("invalid TOML").trim().to_string();
            vec![Diagnostic { line: line.saturating_sub(1), message: short, warning: false }]
        }
    }
}

/// Errors a project-wide tool reported in files other than the open
/// one, so the toast can say "fine here, broken over there".
fn count_other_file_errors(text: &str, fname: &str) -> usize {
    text.lines().filter(|l| l.contains(": error") && !l.contains(fname)).count()
}

/// Pull `file:line: message` style problems out of whatever a checker
/// printed. Understands rustc/cargo (short), cc/clang, shellcheck,
/// pyflakes, node --check, python tracebacks, and bash/zsh -n. Only
/// problems in `fname` are kept.
pub fn parse_output(text: &str, fname: &str) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = Vec::new();
    // A location seen on one line whose message arrives a few lines
    // later (python's `File "x.py", line 4` / node's `x.js:4`).
    let mut pending: Option<usize> = None;
    // A rust panic names the line first and the reason on the next
    // line; when set, the next non-empty line completes it.
    let mut panic_pending: Option<usize> = None;
    // The most recent message-looking line, for stack-frame formats
    // (dart) where `#1 main (file:///x.dart:3:10)` follows the reason.
    let mut last_msg: Option<String> = None;
    if fname.is_empty() {
        return out;
    }

    for raw in text.lines() {
        let line = raw.trim_end();

        // A panic's reason is the first non-empty line after it.
        if let Some(n) = panic_pending.take() {
            let t = line.trim();
            if !t.is_empty() {
                push(&mut out, n, format!("panicked: {t}"));
            } else {
                panic_pending = Some(n);
            }
            continue;
        }

        // dart analyze: `  error - main.dart:4:1 - Expected ... - code`
        let parts: Vec<&str> = line.trim().split(" - ").collect();
        if parts.len() >= 3 {
            if let Some(i) = parts[1].find(fname) {
                if let Some(rest) = parts[1][i + fname.len()..].strip_prefix(':') {
                    let digits: String =
                        rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = digits.parse::<usize>() {
                        push(&mut out, n, format!("{}: {}", parts[0].trim(), parts[2].trim()));
                        continue;
                    }
                }
            }
        }

        // Stack frames (`#1  main (file:///x.dart:3:10)`) point at the
        // line; the reason is whatever was printed just above them.
        if line.trim_start().starts_with('#') {
            if let Some(i) = line.find(fname) {
                if let Some(rest) = line[i + fname.len()..].strip_prefix(':') {
                    let digits: String =
                        rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = digits.parse::<usize>() {
                        let msg =
                            last_msg.clone().unwrap_or_else(|| "runtime error here".into());
                        push(&mut out, n, msg);
                    }
                }
            }
            continue;
        }

        // python: `  File "path/x.py", line 4`
        if let Some(rest) = line.trim_start().strip_prefix("File \"") {
            if let Some((path, after)) = rest.split_once('"') {
                if path.ends_with(fname) {
                    if let Some(num) = after.trim_start_matches(',').trim().strip_prefix("line ") {
                        let digits: String =
                            num.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if let Ok(n) = digits.parse::<usize>() {
                            pending = Some(n);
                        }
                    }
                }
            }
            continue;
        }

        if let Some(idx) = line.find(fname) {
            let after = &line[idx + fname.len()..];
            // bash: `x.sh: line 4: syntax error near ...`
            if let Some(rest) = after.strip_prefix(": line ") {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<usize>() {
                    let msg = rest[digits.len()..].trim_start_matches(':').trim();
                    if !msg.is_empty() {
                        push(&mut out, n, msg.to_string());
                        continue;
                    }
                }
            }
            // compilers: `x.rs:4:9: error: ...` / `x.c:4: error: ...`
            // node: `x.js:4` alone, message on a later line.
            if let Some(rest) = after.strip_prefix(':') {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let n: usize = digits.parse().unwrap_or(1);
                    let tail = &rest[digits.len()..];
                    if tail.is_empty() {
                        pending = Some(n);
                        continue;
                    }
                    if let Some(t) = tail.strip_prefix(':') {
                        let t_trim = t.trim_start();
                        let col: String =
                            t_trim.chars().take_while(|c| c.is_ascii_digit()).collect();
                        let msg = if !col.is_empty() {
                            match t_trim[col.len()..].strip_prefix(':') {
                                Some(m) => m.trim().to_string(),
                                None => t.trim().to_string(),
                            }
                        } else {
                            t.trim().to_string()
                        };
                        if !msg.is_empty() {
                            push(&mut out, n, msg);
                            continue;
                        }
                        // rust: `thread 'main' panicked at src/main.rs:5:10:`
                        if line.contains("panicked at") {
                            panic_pending = Some(n);
                            continue;
                        }
                    }
                }
            }
        }

        // `SyntaxError: ...` and friends complete a pending location.
        if let Some(n) = pending {
            let t = line.trim_start();
            let first = t.split(':').next().unwrap_or("");
            if first.ends_with("Error") && !first.contains(' ') {
                push(&mut out, n, t.to_string());
                pending = None;
                continue;
            }
        }

        // Anything else informative may be the reason a stack frame
        // below it will need.
        let t = line.trim();
        if !t.is_empty() {
            last_msg = Some(t.to_string());
        }
    }

    out.sort_by(|a, b| a.line.cmp(&b.line));
    out.dedup_by(|a, b| a.line == b.line && a.message == b.message);
    out.truncate(50);
    out
}

fn push(out: &mut Vec<Diagnostic>, one_based: usize, message: String) {
    let head = message.trim_start();
    let warning =
        head.starts_with("warning") || head.starts_with("note") || head.starts_with("info");
    out.push(Diagnostic { line: one_based.saturating_sub(1), message, warning });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every parser sample below is captured verbatim from the real tool.

    #[test]
    fn parses_rustc_short() {
        let text = "bad.rs:2:18: error[E0308]: mismatched types: expected `i32`, found `&str`\nerror: aborting due to 1 previous error\n";
        let d = parse_output(text, "bad.rs");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 1);
        assert!(d[0].message.contains("mismatched types"));
        assert!(!d[0].warning);
    }

    #[test]
    fn parses_rustc_warning() {
        let text = "bad.rs:3:9: warning: unused variable: `unused`\n";
        let d = parse_output(text, "bad.rs");
        assert_eq!(d.len(), 1);
        assert!(d[0].warning);
    }

    #[test]
    fn parses_python_compile() {
        let text = "  File \"bad.py\", line 4\n    print(\"hi\")\n               ^\nSyntaxError: unexpected EOF while parsing\n";
        let d = parse_output(text, "bad.py");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 3);
        assert!(d[0].message.starts_with("SyntaxError"));
    }

    #[test]
    fn parses_node_check() {
        let text = "/tmp/x/bad.js:2\n  return {;\n          ^\n\nSyntaxError: Unexpected token ';'\n    at wrapSafe (node:internal/modules/cjs/loader:1804:18)\n\nNode.js v24.18.0\n";
        let d = parse_output(text, "bad.js");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 1);
        assert!(d[0].message.contains("Unexpected token"));
    }

    #[test]
    fn parses_bash_n() {
        let text = "bad.sh: line 3: syntax error: unexpected end of file\n";
        let d = parse_output(text, "bad.sh");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 2);
        assert!(d[0].message.contains("syntax error"));
    }

    #[test]
    fn parses_cc() {
        let text = "bad.c:2:11: error: expected expression\n    2 |   int x = ;\n      |           ^\n1 error generated.\n";
        let d = parse_output(text, "bad.c");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 1);
    }

    #[test]
    fn parses_dart_analyze() {
        let text = "Analyzing main.dart...\n\n  error - main.dart:4:1 - Expected to find '}'. - expected_token\n\n1 issue found.\n";
        let d = parse_output(text, "main.dart");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 3);
        assert!(d[0].message.contains("Expected to find"));
        assert!(!d[0].warning);
    }

    #[test]
    fn parses_dart_runtime_crash() {
        let text = "Unhandled exception:\nRangeError (length): Invalid value: Valid value range is empty: 3\n#0      List.[] (dart:core-patch/growable_array.dart)\n#1      main (file:///tmp/x/crash.dart:3:10)\n#2      _delayEntrypointInvocation.<anonymous closure> (dart:isolate-patch/isolate_patch.dart:313:19)\n";
        let d = parse_output(text, "crash.dart");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 2);
        assert!(d[0].message.contains("RangeError"));
    }

    #[test]
    fn parses_rust_panic() {
        let text = "thread 'main' panicked at src/main.rs:5:10:\ncalled `Option::unwrap()` on a `None` value\nnote: run with `RUST_BACKTRACE=1` ...\n";
        let d = parse_output(text, "main.rs");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 4);
        assert!(d[0].message.contains("unwrap"));
    }

    #[test]
    fn ignores_other_files_but_counts_them() {
        let text = "other.rs:9:1: error: something\n";
        assert!(parse_output(text, "bad.rs").is_empty());
        assert_eq!(count_other_file_errors(text, "bad.rs"), 1);
    }

    // End-to-end: the real tools, through the real spawn path.

    fn e2e(name: &str, content: &str) -> CheckResult {
        let dir = std::env::temp_dir().join("silver_diag_e2e");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, content).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(spawn_check(&file, &dir, false, tx), "no checker for {name}");
        rx.recv_timeout(std::time::Duration::from_secs(20)).expect("check result")
    }

    #[test]
    fn end_to_end_dart_missing_bracket() {
        // The user's exact case: a bracket removed from main.dart.
        // Skipped quietly on machines without dart.
        if std::process::Command::new("dart").arg("--version").output().is_err() {
            return;
        }
        let r = e2e("e2e.dart", "void main() {\n  print(\"hi\");\n\n");
        assert!(r.failed.is_none());
        assert!(!r.diags.is_empty(), "dart missing-bracket not detected");
        assert!(!r.diags[0].warning);
    }

    #[test]
    fn end_to_end_bash_syntax_error() {
        let r = e2e("e2e.sh", "if [ 1 ]; then\necho hi\n");
        assert!(r.failed.is_none());
        assert!(!r.diags.is_empty(), "bash error not detected");
    }

    #[test]
    fn end_to_end_python_syntax_error() {
        let r = e2e("e2e.py", "def f():\n    return (\n\nprint('hi')\n");
        assert!(r.failed.is_none());
        assert!(!r.diags.is_empty(), "python error not detected");
    }

    #[test]
    fn end_to_end_python_leaves_no_pycache() {
        let _ = e2e("clean.py", "x = 1\n");
        let dir = std::env::temp_dir().join("silver_diag_e2e");
        assert!(!dir.join("__pycache__").exists(), "checker littered __pycache__");
    }

    #[test]
    fn snapshot_checks_unsaved_text() {
        let dir = std::env::temp_dir().join("silver_diag_e2e");
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("live.py");
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(spawn_snapshot("def f(:\n".into(), &real, &dir, 7, tx));
        let r = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
        assert!(r.quiet);
        assert_eq!(r.rev, 7);
        assert_eq!(r.path, real, "marks must come back against the real file");
        assert!(!r.diags.is_empty(), "snapshot error not detected");
    }
}
