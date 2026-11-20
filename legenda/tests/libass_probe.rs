//! End-to-end coverage of the ffmpeg libass probe and its fail-fast error.
//!
//! A tiny platform-native fake ffmpeg answers `-filters` with a controlled
//! listing and fails any other invocation. No real media or network is needed,
//! so these tests run on every platform and in CI without gating.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use legenda_core::burn::probe_libass_filters;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Writes a fake ffmpeg that prints `listing` for `-filters` and exits 7 for
/// everything else, returning the path to the executable script.
fn fake_ffmpeg(listing: &str) -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("legenda-fake-ffmpeg-{nanos}-{sequence}"));
    std::fs::create_dir_all(&directory).unwrap();
    let path =
        if cfg!(windows) { directory.join("ffmpeg.cmd") } else { directory.join("ffmpeg.sh") };
    let script = if cfg!(windows) {
        // `>` must be escaped for batch's echo; our listings contain V->V.
        let escaped: Vec<String> = listing.lines().map(|line| line.replace('>', "^>")).collect();
        let echoes = escaped.iter().map(|line| format!("echo {line}\r\n")).collect::<String>();
        format!(
            "@echo off\r\necho %* | findstr /C:\"-filters\" >nul\r\n\
             if errorlevel 1 goto burn\r\n{echoes}exit /b 0\r\n\
             :burn\r\necho fake burn invoked 1>&2\r\nexit /b 7\r\n"
        )
    } else {
        let quoted: Vec<String> = listing.lines().map(|line| format!("'{line}'")).collect();
        let arguments = quoted.join(" ");
        format!(
            "#!/bin/sh\ncase \"$*\" in\n  *-filters*)\n    printf '%s\\n' {arguments}\n    exit 0\n    ;;\nesac\necho 'fake burn invoked' >&2\nexit 7\n"
        )
    };
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}

fn caption_file(ffmpeg: &Path) -> PathBuf {
    let path = ffmpeg.parent().unwrap().join("captions.ass");
    std::fs::write(&path, "[Script Info]\nScriptType: v4.00+\n").unwrap();
    path
}

fn burn_with(ffmpeg: &Path, captions: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_legenda"))
        .args(["burn", "in.mp4"])
        .arg(captions)
        .args(["-o", "out.mp4", "--ffmpeg"])
        .arg(ffmpeg)
        .output()
        .unwrap()
}

#[test]
fn library_probe_reads_filters_from_a_real_process() {
    let fake = fake_ffmpeg("Filters:\n .. subtitles         V->V       Render text subtitles.\n");
    let filters = probe_libass_filters(&fake).unwrap();
    assert!(filters.subtitles, "{filters:?}");
    assert!(!filters.ass, "{filters:?}");
    assert!(filters.supports_libass());
}

#[test]
fn cli_fails_fast_when_ffmpeg_lacks_libass() {
    let fake = fake_ffmpeg("Filters:\n .. scale             V->V       Scale the input video.\n");
    let captions = caption_file(&fake);
    let output = burn_with(&fake, &captions);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("lacks libass"), "{stderr}");
    assert!(stderr.contains("--enable-libass"), "{stderr}");
    assert!(stderr.contains("macOS"), "{stderr}");
    assert!(!stderr.contains("burned captions"), "{stderr}");
}

#[test]
fn cli_proceeds_to_ffmpeg_when_an_ass_filter_is_available() {
    let fake = fake_ffmpeg("Filters:\n .. ass               V->V       Render ASS subtitles.\n");
    let captions = caption_file(&fake);
    let output = burn_with(&fake, &captions);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("lacks libass"), "{stderr}");
    assert!(stderr.contains("fake burn invoked") || stderr.contains("ffmpeg failed"), "{stderr}");
}
