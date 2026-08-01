//! The winit + softbuffer adapter.
//!
//! This file is deliberately thin: it translates platform events into
//! [`Browser`] calls and copies the rasterizer's output into the window. All the
//! behaviour lives in [`crate::browser`], which is why the interaction model can
//! be tested without a display.

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId};

use crate::browser::{Browser, ShellConfig};
use crate::lines_to_pixels;
use wat_layout::geom::{Point, Size2D};
use wat_paint::Canvas;
use wat_style::Cursor;

/// Opens a window and runs the browser until it is closed.
pub fn run(config: ShellConfig) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    run_with_event_loop(event_loop, config)
}

/// Runs on an event loop the caller built, which is how the Android entry point
/// supplies its platform-specific loop.
pub fn run_with_event_loop(event_loop: EventLoop<()>, config: ShellConfig) -> Result<(), String> {
    event_loop.set_control_flow(ControlFlow::Wait);
    let browser = Browser::new(&config)?;
    let mut app = App {
        browser,
        config,
        window: None,
        surface: None,
        canvas: Canvas::new(1, 1),
        modifiers: Default::default(),
        first_frame: true,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())
}

struct App {
    browser: Browser,
    config: ShellConfig,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    /// Reused between frames so a resize is the only thing that reallocates.
    canvas: Canvas,
    modifiers: wat_ui::Modifiers,
    /// Cleared once the first frame has gone up, which is drawn cheaply.
    first_frame: bool,
}

impl App {
    /// The display's device pixel ratio.
    ///
    /// A phone reports 2.5 to 3.5 here, and treating those pixels as CSS pixels
    /// would draw the whole interface at a third of its intended size.
    fn scale_factor(&self) -> f32 {
        match &self.window {
            Some(window) => {
                let scale = window.scale_factor() as f32;
                // A nonsensical value from a platform must not make the window
                // unrenderable.
                if scale.is_finite() && scale > 0.0 {
                    scale.clamp(0.5, 8.0)
                } else {
                    1.0
                }
            }
            None => 1.0,
        }
    }

    /// The window size in CSS pixels, which is what the chrome lays out in.
    fn logical_size(&self) -> Size2D {
        match &self.window {
            Some(window) => {
                let size = window.inner_size();
                let scale = self.scale_factor();
                Size2D::new(
                    (size.width.max(1) as f32 / scale).max(1.0),
                    (size.height.max(1) as f32 / scale).max(1.0),
                )
            }
            None => self.config.size,
        }
    }

    /// A position winit reported, in CSS pixels.
    ///
    /// winit works in device pixels; everything above this file works in CSS
    /// pixels, so every incoming coordinate is divided once, here.
    fn to_logical(&self, x: f64, y: f64) -> Point {
        let scale = self.scale_factor();
        Point::new(x as f32 / scale, y as f32 / scale)
    }

    /// The window size in device pixels, which is what the canvas holds.
    fn physical_size(&self) -> (u32, u32) {
        match &self.window {
            Some(window) => {
                let size = window.inner_size();
                (size.width.max(1), size.height.max(1))
            }
            None => (
                self.config.size.width.max(1.0) as u32,
                self.config.size.height.max(1.0) as u32,
            ),
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn sync_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&self.browser.title());
        }
    }

    fn sync_cursor(&self) {
        let Some(window) = &self.window else { return };
        use winit::window::CursorIcon;
        let icon = match self.browser.cursor() {
            Cursor::Pointer => CursorIcon::Pointer,
            Cursor::Text => CursorIcon::Text,
            Cursor::Move => CursorIcon::Move,
            Cursor::NotAllowed => CursorIcon::NotAllowed,
            Cursor::Grab => CursorIcon::Grab,
            Cursor::Auto | Cursor::Default => CursorIcon::Default,
        };
        window.set_cursor(icon);
    }

    fn draw(&mut self) {
        // The sizes are read before the surface is borrowed mutably.
        let (width, height) = self.physical_size();
        let scale = self.scale_factor();
        let (Some(nz_width), Some(nz_height)) = (NonZeroU32::new(width), NonZeroU32::new(height))
        else {
            return;
        };

        let resized = self.canvas.width() != width || self.canvas.height() != height;
        if resized {
            self.canvas = Canvas::new(width, height);
        }
        // A redraw request does not mean anything changed: the compositor asks
        // for one when the window is exposed, and the event loop asks for one on
        // every turn while a page has a timer pending. Re-rendering regardless
        // meant repainting the whole window, at tens of milliseconds a frame,
        // to arrive at the pixels already on screen. The canvas is kept, so when
        // nothing is dirty the frame just gets presented again.
        if resized || self.browser.needs_redraw {
            // The canvas is in device pixels; everything above is in CSS pixels.
            if self.first_frame {
                // Launch is the one moment where showing something now beats
                // showing everything later, so the first frame goes up without
                // the backdrop filters and shadows — most of what a glass frame
                // costs — and the real one follows immediately. The layout is
                // the same either way, so what the user sees is the interface
                // sharpening, not moving.
                self.first_frame = false;
                self.browser
                    .render_preview_into_scaled(&mut self.canvas, scale);
                self.request_redraw();
            } else {
                self.browser.render_into_scaled(&mut self.canvas, scale);
            }
        }

        let Some(surface) = &mut self.surface else {
            return;
        };
        if let Err(error) = surface.resize(nz_width, nz_height) {
            log::error!("cannot resize the window surface: {error}");
            return;
        }

        match surface.buffer_mut() {
            Ok(mut buffer) => {
                // softbuffer wants 0xAARRGGBB, which the canvas can produce.
                let pixels = self.canvas.to_argb32();
                let count = buffer.len().min(pixels.len());
                buffer[..count].copy_from_slice(&pixels[..count]);
                if let Err(error) = buffer.present() {
                    log::error!("cannot present the frame: {error}");
                }
            }
            Err(error) => log::error!("cannot lock the window surface: {error}"),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.browser.title())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.size.width,
                self.config.size.height,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                log::error!("cannot create a window: {error}");
                event_loop.exit();
                return;
            }
        };

        match softbuffer::Context::new(window.clone())
            .and_then(|context| softbuffer::Surface::new(&context, window.clone()))
        {
            Ok(surface) => self.surface = Some(surface),
            Err(error) => {
                log::error!("cannot create a drawing surface: {error}");
                event_loop.exit();
                return;
            }
        }

        self.window = Some(window);
        self.browser.resize(self.logical_size());
        self.request_redraw();
    }

    /// Runs the page's queued timer callbacks between events.
    ///
    /// A page with a pending timer keeps the loop polling; an idle page goes
    /// back to waiting, so a window that is doing nothing costs nothing.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.browser.has_pending_script_work() {
            self.browser.run_script_timers();
            self.sync_title();
            // Only if running them actually changed something. A timer that
            // fires without touching the document — or a page still waiting for
            // one to come due — used to request a frame on every turn of the
            // loop, which with `Poll` below is a full repaint as fast as the
            // renderer can manage, forever.
            if self.browser.needs_redraw {
                self.request_redraw();
            }
        }
        event_loop.set_control_flow(if self.browser.has_pending_script_work() {
            ControlFlow::Poll
        } else {
            ControlFlow::Wait
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.browser.resize(self.logical_size());
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::ModifiersChanged(state) => {
                let state = state.state();
                self.modifiers = wat_ui::Modifiers {
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    alt: state.alt_key(),
                    meta: state.super_key(),
                };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let point = self.to_logical(position.x, position.y);
                self.browser.pointer_moved(point);
                self.sync_cursor();
                if self.browser.needs_redraw {
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.browser.pointer_left();
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // The pointer position comes from the last CursorMoved event.
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        if let Some(position) = self.last_pointer() {
                            self.browser.pointer_down(position);
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        if let Some(position) = self.last_pointer() {
                            self.browser.pointer_up(position);
                        }
                    }
                    (MouseButton::Middle, ElementState::Released) => {
                        if let Some(position) = self.last_pointer() {
                            self.browser.middle_click(position);
                        }
                    }
                    _ => {}
                }
                self.sync_title();
                self.request_redraw();
                if self.browser.should_quit {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => -lines_to_pixels(lines),
                    // A pixel delta is in device pixels, like every other
                    // position winit reports.
                    MouseScrollDelta::PixelDelta(position) => {
                        -(position.y as f32) / self.scale_factor()
                    }
                };
                if let Some(position) = self.last_pointer() {
                    self.browser.scroll(position, delta_y);
                }
                if self.browser.needs_redraw {
                    self.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let point = self.to_logical(touch.location.x, touch.location.y);
                match touch.phase {
                    winit::event::TouchPhase::Started => self.browser.touch_started(point),
                    winit::event::TouchPhase::Moved => self.browser.touch_moved(point),
                    winit::event::TouchPhase::Ended => self.browser.touch_ended(point),
                    winit::event::TouchPhase::Cancelled => self.browser.touch_cancelled(),
                }
                self.sync_title();
                self.request_redraw();
                if self.browser.should_quit {
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                // Android's back gesture arrives as a key. It means "go back",
                // and only leaves the app when there is nowhere left to go —
                // which is what every browser on the platform does.
                if event.logical_key == WinitKey::Named(NamedKey::BrowserBack) {
                    if self.browser.can_go_back() {
                        self.browser.apply(wat_ui::UiAction::GoBack);
                    } else {
                        event_loop.exit();
                    }
                    self.sync_title();
                    self.request_redraw();
                    return;
                }
                if let Some(key) = translate_key(&event.logical_key) {
                    self.browser.key_pressed(key, self.modifiers);
                    self.sync_title();
                    self.request_redraw();
                    if self.browser.should_quit {
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// winit reports button presses without a position, so the last known
    /// pointer position is used.
    fn last_pointer(&self) -> Option<Point> {
        self.browser.pointer_position()
    }
}

/// Maps a winit key to the chrome's key model.
fn translate_key(key: &WinitKey) -> Option<wat_ui::Key> {
    use wat_ui::Key;
    match key {
        WinitKey::Character(text) => text.chars().next().map(Key::Char),
        WinitKey::Named(named) => Some(match named {
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Tab => Key::Tab,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Space => Key::Char(' '),
            NamedKey::F1 => Key::Function(1),
            NamedKey::F2 => Key::Function(2),
            NamedKey::F3 => Key::Function(3),
            NamedKey::F4 => Key::Function(4),
            NamedKey::F5 => Key::Function(5),
            NamedKey::F6 => Key::Function(6),
            NamedKey::F11 => Key::Function(11),
            NamedKey::F12 => Key::Function(12),
            _ => return None,
        }),
        _ => None,
    }
}

/// The Android entry point.
///
/// Built only for Android; it hands winit the platform's `AndroidApp` and then
/// runs the same browser as the desktop, in the mobile chrome layout. The size
/// in the config is only a starting guess — the real one arrives with the first
/// resize, and the window is laid out in CSS pixels either way.
/// The glue calls this by symbol name with the Rust ABI, so it is `no_mangle`
/// but deliberately not `extern "C"`: `AndroidApp` has no C representation.
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    // Nothing here writes to stdout, and a device has no console anyway, so the
    // log goes to logcat: `adb logcat -s WAT`.
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("WAT"),
    );
    log::info!("What-A-Browser {} starting", env!("CARGO_PKG_VERSION"));

    let event_loop = match EventLoop::builder().with_android_app(app).build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("cannot build the Android event loop: {error}");
            return;
        }
    };
    if let Err(error) = run_with_event_loop(event_loop, ShellConfig::mobile()) {
        log::error!("the browser exited with an error: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_keys_map_through() {
        assert_eq!(
            translate_key(&WinitKey::Character("a".into())),
            Some(wat_ui::Key::Char('a'))
        );
        assert_eq!(
            translate_key(&WinitKey::Named(NamedKey::Enter)),
            Some(wat_ui::Key::Enter)
        );
        assert_eq!(
            translate_key(&WinitKey::Named(NamedKey::Space)),
            Some(wat_ui::Key::Char(' '))
        );
        assert_eq!(
            translate_key(&WinitKey::Named(NamedKey::F5)),
            Some(wat_ui::Key::Function(5))
        );
    }

    #[test]
    fn unhandled_keys_are_dropped_rather_than_guessed() {
        assert_eq!(translate_key(&WinitKey::Named(NamedKey::Meta)), None);
        assert_eq!(translate_key(&WinitKey::Named(NamedKey::PrintScreen)), None);
    }
}
