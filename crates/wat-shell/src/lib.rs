//! The window shell: desktop and mobile entry points.
//!
//! [`Browser`] holds all the behaviour and knows nothing about windowing; this
//! module is the thin adapter that pumps a platform event loop into it and
//! presents the resulting pixels.
//!
//! * Desktop (Linux, macOS, Windows): [`run`] creates a window and runs to
//!   completion.
//! * Android: `android_main` is exported when building for Android, using the
//!   same [`Browser`] and the mobile chrome layout.
//! * iOS: the same [`run`] entry point works, since winit drives UIKit; the
//!   Xcode project calls it from `main`.
//!
//! Pixels are produced by the software rasterizer and handed to the platform as
//! a plain framebuffer, so no GPU driver is required.

pub mod browser;

pub use browser::{Browser, ShellConfig};

#[cfg(feature = "window")]
mod window;

#[cfg(feature = "window")]
pub use window::run;

#[cfg(not(feature = "window"))]
/// Stub used when the crate is built without windowing support.
pub fn run(_config: ShellConfig) -> Result<(), String> {
    Err("this build has no window support; use the CLI commands instead".to_string())
}

/// Maps a scroll delta in lines to logical pixels.
pub fn lines_to_pixels(lines: f32) -> f32 {
    // A line is roughly one and a half text lines at the default size.
    lines * 24.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_scrolling_moves_a_sensible_amount() {
        assert!(lines_to_pixels(1.0) > 10.0);
        assert!(lines_to_pixels(-3.0) < 0.0);
        assert_eq!(lines_to_pixels(0.0), 0.0);
    }

    #[test]
    fn the_default_config_is_a_desktop_window() {
        let config = ShellConfig::default();
        assert!(!config.touch);
        assert!(config.size.width > 640.0);
        assert_eq!(config.home, "about:home");
    }

    #[test]
    fn the_mobile_config_is_a_phone() {
        let config = ShellConfig::mobile();
        assert!(config.touch);
        assert!(config.size.width < 640.0);
    }
}
