// Compile-time game stamp shared by the app and the headless runner.
// Included from each binary's build script.

include!("calver.rs");

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn emit_stamp() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..").canonicalize().unwrap_or_else(|_| manifest_dir.join("../.."));
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed={}", root.join("crates/calver.rs").display());
    println!("cargo:rerun-if-changed={}", root.join("crates/stamp.rs").display());
    println!("cargo:rerun-if-changed={}", root.join("Cargo.toml").display());
    println!("cargo:rerun-if-changed={}", root.join("Cargo.lock").display());
    watch_dir(&manifest_dir.join("src"));
    watch_dir(&root.join("crates/luminal-core/src"));
    println!("cargo:rerun-if-changed={}", root.join("crates/luminal-core/Cargo.toml").display());
    if std::env::var("CARGO_PKG_NAME").as_deref() == Ok("luminal-app") {
        watch_dir(&root.join("assets"));
    }
    // `git status` may rewrite the index. Watching that file reruns this script forever.
    watch_git(&root);

    let (year, month) = utc_year_month(now_secs());
    let commit = git(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let status = git(&root, &["status", "--porcelain"]);
    let tags = git(&root, &["tag", "--points-at", "HEAD"]);
    let dirty = commit == "unknown" || status.as_ref().is_none_or(|text| !text.is_empty());
    let tags_known = tags.is_some();
    let tag_lines = tags.unwrap_or_default();
    let tag_refs: Vec<&str> = tag_lines.lines().collect();
    let version = product_version(&tag_refs, dirty || !tags_known, year, month);
    println!("cargo:rustc-env=LUMINAL_VERSION={version}");
    println!("cargo:rustc-env=LUMINAL_COMMIT={commit}");
    println!("cargo:rustc-env=LUMINAL_DIRTY={}", if dirty { "1" } else { "0" });
}

fn now_secs() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|value| value.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    })
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(|text| text.trim().to_string())
}

fn watch_git(root: &Path) {
    let head = git_meta(root, "HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    if let Ok(text) = std::fs::read_to_string(&head)
        && let Some(reference) = text.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed={}", git_meta(root, reference).display());
    }
}

fn git_meta(root: &Path, relative: &str) -> PathBuf {
    let git = root.join(".git");
    if git.is_file()
        && let Ok(text) = std::fs::read_to_string(&git)
        && let Some(directory) = text.trim().strip_prefix("gitdir: ")
    {
        let directory = PathBuf::from(directory);
        let directory = if directory.is_absolute() { directory } else { root.join(directory) };
        return directory.join(relative);
    }
    git.join(relative)
}

fn watch_dir(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            println!("cargo:rerun-if-changed={}", path.display());
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
}
