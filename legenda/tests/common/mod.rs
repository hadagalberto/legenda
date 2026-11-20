//! Shared capability gate for the ffmpeg burn end-to-end tests.

use std::path::Path;

use legenda_core::burn::probe_libass_filters;

/// Whether the burn e2e tests may run against the ffmpeg on `PATH`.
///
/// CI must always exercise the real burn path, so a missing libass build is a
/// hard failure there. Outside CI the tests may be skipped, but only when the
/// probe proves ffmpeg cannot render subtitles, and only after printing the
/// reason to stderr so the skip is never silent.
pub fn libass_available() -> bool {
    let filters = probe_libass_filters(Path::new("ffmpeg"))
        .expect("ffmpeg must be installed for the e2e tests");
    if filters.supports_libass() {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        panic!(
            "CI ffmpeg lacks libass: neither the 'subtitles' nor the 'ass' filter is available. \
             Install a libass-enabled build (on macOS: brew install ffmpeg-full); the burn e2e \
             tests are never skipped in CI."
        );
    }
    eprintln!(
        "skipping burn e2e: ffmpeg on PATH lacks libass (no 'subtitles'/'ass' filter); \
         install a build with --enable-libass to run these tests"
    );
    false
}
