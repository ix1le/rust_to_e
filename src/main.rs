use indicatif::{ProgressBar, ProgressStyle};
use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{exit, Command},
    thread::sleep,
    time::{Duration, Instant},
};

const RUSTUP_URL: &str = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe";
const VS_URL: &str = "https://aka.ms/vs/17/release/vs_BuildTools.exe";

fn read_line() -> String {
    let mut s = String::new();
    io::stdin().read_line(&mut s).ok();
    s.trim().to_string()
}

fn pause() {
    println!("\nPress Enter to close...");
    let _ = io::stdin().read_line(&mut String::new());
}

fn fail(msg: &str) -> ! {
    eprintln!("{}", msg);
    pause();
    exit(1);
}

fn is_admin() -> bool {
    Command::new("fltmc").output().map(|o| o.status.success()).unwrap_or(false)
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = fs::read_dir(p) {
        for entry in rd.flatten() {
            if let Ok(m) = fs::symlink_metadata(entry.path()) {
                if m.file_type().is_symlink() {
                    continue;
                }
                if m.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

fn is_empty_dir(p: &Path) -> bool {
    fs::read_dir(p).map(|mut r| r.next().is_none()).unwrap_or(false)
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

#[derive(PartialEq)]
enum Action {
    Move,
    Delete,
    Skip,
}

fn ask_action(link: &Path, real: &Path) -> Action {
    println!("\n{} already exists, measuring size...", link.display());
    let size = dir_size(link);
    loop {
        println!("{} is {:.2} GB.", link.display(), gb(size));
        print!("[M]ove to {} (default) / [D]elete / [S]kip: ", real.display());
        io::stdout().flush().ok();

        match read_line().to_uppercase().as_str() {
            "" | "M" => return Action::Move,
            "S" => return Action::Skip,
            "D" => {
                print!("Permanently delete {}? Type YES to confirm: ", link.display());
                io::stdout().flush().ok();
                if read_line() == "YES" {
                    return Action::Delete;
                }
                println!("Not confirmed.");
            }
            _ => println!("Enter M, D or S."),
        }
    }
}

fn junction(link: &Path, real: &Path) {
    fs::create_dir_all(real)
        .unwrap_or_else(|e| fail(&format!("Cannot create {}: {}", real.display(), e)));

    if let Ok(meta) = fs::symlink_metadata(link) {
        if meta.file_type().is_symlink() {
            println!("[=] already a link: {}", link.display());
            return;
        }

        if is_empty_dir(link) {
            fs::remove_dir(link)
                .unwrap_or_else(|e| fail(&format!("[!] Failed to remove {}: {}", link.display(), e)));
        } else {
            match ask_action(link, real) {
                Action::Skip => {
                    println!("[~] skipped {} (it will stay on C:)", link.display());
                    return;
                }
                Action::Move => {
                    println!("[>] moving {} -> {}", link.display(), real.display());
                    let st = Command::new("robocopy")
                        .arg(link)
                        .arg(real)
                        .args(["/E", "/MOVE", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS", "/NP"])
                        .status()
                        .unwrap_or_else(|_| fail("Failed to run robocopy"));
                    if st.code().map(|c| c >= 8).unwrap_or(true) {
                        fail(&format!("[!] robocopy failed on {}", link.display()));
                    }
                }
                Action::Delete => {}
            }

            if link.exists() {
                println!("[x] removing {}", link.display());
                if let Err(e) = fs::remove_dir_all(link) {
                    fail(&format!(
                        "[!] Failed to remove {}: {}\n    (close programs that use it and try again)",
                        link.display(),
                        e
                    ));
                }
            }
        }
    }

    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).ok();
    }

    let ok = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(real)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        fail(&format!("[!] Failed to create junction {}", link.display()));
    }
    println!("[+] {} -> {}", link.display(), real.display());
}

fn download(url: &str, to: &Path) {
    let resp = ureq::get(url)
        .call()
        .unwrap_or_else(|e| fail(&format!("Download failed {}: {}", url, e)));

    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    let mut reader = pb.wrap_read(resp.into_reader());
    let mut file = File::create(to).unwrap_or_else(|e| fail(&format!("Cannot create file: {}", e)));
    io::copy(&mut reader, &mut file).unwrap_or_else(|e| fail(&format!("Download error: {}", e)));
    pb.finish_with_message("done");
}

fn install_vs(setup: &Path, args: &[String], tmp: Option<&Path>, watch: &Path) -> Option<i32> {
    let mut cmd = Command::new(setup);
    cmd.args(args);
    if let Some(t) = tmp {
        cmd.env("TEMP", t).env("TMP", t);
    }
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| fail(&format!("Failed to start the VS installer: {}", e)));

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner} [{elapsed_precise}] {msg}").unwrap());
    pb.enable_steady_tick(Duration::from_millis(120));

    let mut last = 0u64;
    let mut last_time = Instant::now();
    let mut last_check = Instant::now() - Duration::from_secs(10);

    loop {
        if let Ok(Some(status)) = child.try_wait() {
            pb.finish_with_message(format!(
                "installation finished, {}: {:.2} GB",
                watch.display(),
                gb(dir_size(watch))
            ));
            return status.code();
        }
        if last_check.elapsed() >= Duration::from_secs(3) {
            let now = dir_size(watch);
            let dt = last_time.elapsed().as_secs_f64().max(0.1);
            pb.set_message(format!(
                "{} is now {:.2} GB (+{:.1} MB/s), expect ~3-6 GB total",
                watch.display(),
                gb(now),
                now.saturating_sub(last) as f64 / 1024.0 / 1024.0 / dt,
            ));
            last = now;
            last_time = Instant::now();
            last_check = Instant::now();
        }
        sleep(Duration::from_millis(500));
    }
}

fn ask_drive() -> char {
    loop {
        println!("Where to install? Enter a drive letter (C = regular install, Enter = C): ");
        let s = read_line();
        let s = s.trim_end_matches([':', '\\']).to_uppercase();

        let letter = if s.is_empty() { 'C' } else { s.chars().next().unwrap() };
        if !letter.is_ascii_alphabetic() || s.chars().count() > 1 {
            println!("Enter a single letter, e.g. E.");
            continue;
        }
        if !Path::new(&format!("{}:\\", letter)).exists() {
            println!("Drive {}: does not exist, try another one.", letter);
            continue;
        }
        return letter;
    }
}

fn main() {
    if !is_admin() {
        fail("Run as administrator (right-click -> Run as administrator).");
    }

    let drive = ask_drive();
    let custom = drive != 'C';

    let (work_dir, rust_home, vs_root): (PathBuf, Option<PathBuf>, Option<PathBuf>) = if custom {
        let rust = PathBuf::from(format!(r"{}:\Rust", drive));
        let vs = PathBuf::from(format!(r"{}:\VS", drive));
        fs::create_dir_all(&rust).unwrap();
        fs::create_dir_all(&vs).unwrap();
        (vs.clone(), Some(rust), Some(vs))
    } else {
        (env::temp_dir(), None, None)
    };

    let mut tmp: Option<PathBuf> = None;

    if let (Some(e), Some(vs)) = (&rust_home, &vs_root) {
        let home = PathBuf::from(env::var("USERPROFILE").unwrap_or_else(|_| fail("USERPROFILE is not set")));

        println!("\n=== Junctions to drive {}: ===", drive);
        junction(&home.join(".cargo"), &e.join(".cargo"));
        junction(&home.join(".rustup"), &e.join(".rustup"));
        junction(Path::new(r"C:\Program Files (x86)\Microsoft Visual Studio"), &vs.join("ProgramFilesX86_VS"));
        junction(Path::new(r"C:\Program Files (x86)\Windows Kits"), &vs.join("WindowsKits"));
        junction(Path::new(r"C:\Program Files (x86)\Microsoft SDKs"), &vs.join("MicrosoftSDKs"));
        junction(Path::new(r"C:\ProgramData\Microsoft\VisualStudio"), &vs.join("ProgramData_VS"));
        junction(Path::new(r"C:\ProgramData\Package Cache"), &vs.join("PackageCache"));

        let t = vs.join("tmp");
        fs::create_dir_all(&t).unwrap();
        tmp = Some(t);
    } else {
        println!("\nRegular install on C, no junctions needed.");
    }

    println!("\n=== Downloading Build Tools installer ===");
    let vs_setup = work_dir.join("vs_BuildTools.exe");
    download(VS_URL, &vs_setup);

    println!("\n=== Installing Build Tools ===");
    let mut args: Vec<String> = vec!["--quiet", "--wait", "--norestart", "--nocache"]
        .into_iter()
        .map(String::from)
        .collect();
    if let Some(vs) = &vs_root {
        args.push("--installPath".into());
        args.push(vs.join("BuildTools").to_string_lossy().into_owned());
    }
    for c in [
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "Microsoft.VisualStudio.Component.Windows11SDK.22621",
    ] {
        args.push("--add".into());
        args.push(c.into());
    }

    let watch: PathBuf = match &vs_root {
        Some(vs) => vs.clone(),
        None => PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio"),
    };

    match install_vs(&vs_setup, &args, tmp.as_deref(), &watch) {
        Some(0) | Some(3010) => println!("Build Tools installed."),
        c => fail(&format!("VS installer exited with code {:?}", c)),
    }

    println!("\n=== Downloading rustup ===");
    let rustup = work_dir.join("rustup-init.exe");
    download(RUSTUP_URL, &rustup);

    println!("\n=== Installing Rust ===");
    let mut cmd = Command::new(&rustup);
    cmd.arg("-y");
    if let Some(e) = &rust_home {
        cmd.env("CARGO_HOME", e.join(".cargo"))
            .env("RUSTUP_HOME", e.join(".rustup"));
    }
    let status = cmd
        .status()
        .unwrap_or_else(|e| fail(&format!("Failed to start rustup-init: {}", e)));

    println!("\nDone. Restart your terminal so PATH is picked up.");
    pause();
    exit(status.code().unwrap_or(1));
}