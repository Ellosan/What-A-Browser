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
}

impl App {
    /// The window size in logical pixels, which is what the chrome lays out in.
    fn logical_size(&self) -> Size2D {
        match &self.window {
            Some(window) => {
                let size = window.inner_size();
                Size2D::new(size.width.max(1) as f32, size.height.max(1) as f32)
            }
            None => self.config.size,
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
        // The size is read before the surface is borrowed mutably.
        let size = self.logical_size();
        let width = size.width as u32;
        let height = size.height as u32;
        let (Some(nz_width), Some(nz_height)) = (NonZeroU32::new(width), NonZeroU32::new(height))
        else {
            return;
        };

        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas = Canvas::new(width, height);
        }
        self.browser.render_into(&mut self.canvas);

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
                self.browser
                    .pointer_moved(Point::new(position.x as f32, position.y as f32));
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
                    MouseScrollDelta::PixelDelta(position) => -position.y as f32,
                };
                if let Some(position) = self.last_pointer() {
                    self.browser.scroll(position, delta_y);
                }
                if self.browser.needs_redraw {
                    self.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let point = Point::new(touch.location.x as f32, touch.location.y as f32);
                match touch.phase {
                    winit::event::TouchPhase::Started => {
                        self.browser.pointer_moved(point);
                        self.browser.pointer_down(point);
                    }
                    winit::event::TouchPhase::Moved => self.browser.pointer_moved(point),
                    winit::event::TouchPhase::Ended => self.browser.pointer_up(point),
                    winit::event::TouchPhase::Cancelled => self.browser.pointer_left(),
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
/// runs the same browser as the desktop, in the mobile chrome layout.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

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
