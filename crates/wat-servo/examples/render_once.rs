//! Loads one page with Servo and paints it into a WAT canvas.
//!
//! `cargo run --example render_once --manifest-path crates/wat-servo/Cargo.toml`
//!
//! This is the end-to-end check for the backend: it proves that Servo starts,
//! lays a document out, renders into the software surface, and that the frame
//! arrives in the canvas the Liquid Glass chrome composites onto. Compiling
//! against the API proves none of that.

use std::time::{Duration, Instant};

use wat_layout::geom::{Rect, Size2D};
use wat_paint::Canvas;
use wat_servo::ServoEngine;
use wat_web::Engine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (width, height) = (800.0f32, 600.0f32);
    let mut engine = ServoEngine::new(Size2D::new(width, height), 1.0)?;

    // A data URL keeps the check off the network.
    let page = "data:text/html,<style>body{margin:0;background:%23204080}\
                h1{color:%23ffcc00;font:48px sans-serif;padding:40px}</style>\
                <h1>Servo</h1>";
    engine.open_tab(page);

    // Everything Servo does is asynchronous, so it has to be pumped. Pump for a
    // fixed spell rather than until the engine says it is idle: a load that has
    // settled is not the same as a first frame having been composited, and this
    // check is about the frame.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_frame = false;
    while Instant::now() < deadline {
        saw_frame |= engine.run_pending_work();
        if saw_frame && !engine.has_pending_work() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("frame reported ready: {saw_frame}");

    let mut canvas = Canvas::new(width as u32, height as u32);
    engine.paint(&mut canvas, Rect::new(0.0, 0.0, width, height), 0.0, 1.0);

    let painted = canvas
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count();
    let total = (width * height) as usize;
    println!("tabs: {:?}", engine.tabs());
    println!("painted {painted} of {total} pixels");

    std::fs::write("servo-frame.png", canvas.to_png()?)?;
    println!("wrote servo-frame.png");

    if painted == 0 {
        return Err("Servo produced no pixels".into());
    }
    Ok(())
}
