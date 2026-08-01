# WAT on Servo

WAT was written as its own engine. This is the other option: the same browser —
same Liquid Glass chrome, same input handling, same window shell — with Servo
turning URLs into pixels instead of `wat-html` through `wat-paint`.

Why Servo and not Firefox: Gecko has no supported embedding path on the desktop.
Mozilla retired it years ago, and GeckoView, the one supported embedding API,
targets Android only. Servo is the engine in that family that can actually be
embedded, it is Rust, and as of `servo 0.4.0` it is on crates.io — so this needs
no `mach`, no git checkout, and no separate build system.

## The seam

`wat-web` is the boundary. It defines one trait, `Engine`, and two engines
implement it:

| | |
|---|---|
| `wat-engine::WatEngine` | WAT's own engine, the default |
| `wat-servo::ServoEngine` | Servo |

Two decisions shaped the trait.

**An engine owns its networking.** The shell used to hold a `Loader` and pass it
into every navigation call, which only worked because WAT's network layer happens
to be a small trait. Servo brings a whole network stack, a resource thread and a
process constellation; it cannot be driven that way. So no loader is threaded
through the seam, and `WatEngine` holds its own.

**Painting cannot be a display list.** WAT's engine produces one, and the chrome
used to splice the page's items into the same list as its own so the backdrop
filters had page pixels to blur. Servo produces a rendered surface instead. The
only thing both can agree to do is put their pixels into a region of the target
canvas, so that is what `Engine::paint` asks for:

```rust
fn paint(&self, canvas: &mut Canvas, area: Rect, corner_radius: f32, scale: f32);
```

The engine must not draw outside `area` rounded by `corner_radius`, because the
chrome is composited on top afterwards and expects the page underneath it.

## How Servo reaches a software canvas

WAT composites in software: the glass reads the pixels beneath it out of a CPU
buffer. Servo renders through WebRender, which normally means an OpenGL surface.
`SoftwareRenderingContext` bridges the two — Servo renders into memory, and
`read_to_image` hands the frame over to be blitted under the chrome.

That is a copy per frame. A GPU compositor would not need it, and moving the
whole shell onto `wgpu` would make the glass much cheaper as well as removing the
copy — but it would mean rewriting the interface, not just the engine. This is
the version where the interface is untouched.

## Building it

`wat-servo` is **excluded from the workspace** on purpose. Building it builds
SpiderMonkey from source: tens of minutes and several gigabytes. An ordinary
`cargo build` should not pay that, and neither should CI on every push.

```sh
cargo build --release --manifest-path crates/wat-servo/Cargo.toml
```

You need `clang` and `python3` for SpiderMonkey, and roughly 15 GB of free disk.

### The dependency pin you cannot skip

`servo 0.4.0` does not build out of the box. `servo-script` pins `p256` and
`p384` to `=0.14.0-rc.14`, and those want `primeorder ^0.14.0-rc.14`. Cargo
prefers the released `primeorder 0.14.0`, where the `WnafSize` bound the release
candidates rely on is no longer satisfied, and the build fails with:

```
error[E0277]: the trait bound `Scalar: WnafSize` is not satisfied
```

`crates/wat-servo/Cargo.toml` pins the pre-release back into place:

```toml
primeorder = "=0.14.0-rc.14"
```

This is upstream churn in a published dependency tree, not something wrong with
the embedding. Expect it to stop being necessary.

## What the Servo backend does not do yet

Honest list, because the trait has corners Servo answers differently:

- **`link_at` returns `None`.** Servo hit-tests inside the engine and reports
  link targets through a delegate as the pointer moves, rather than answering a
  query. The shell uses this only to show a link target, so wiring it means
  keeping the last reported target from the delegate.
- **`can_go_back` and `can_go_forward` always return true when a tab exists.**
  Servo traverses history in another process and does not report depth. A
  traversal with nowhere to go is a no-op, so the buttons work; they just do not
  grey out.
- **Scroll offset is tracked locally**, by accumulating the deltas sent to Servo,
  because the embedder cannot read the real one back. It will drift from the
  engine's own position on an anchor jump or a script scroll.
- **Keyboard and mouse input are not forwarded.** `Engine` does not carry them
  yet — the existing shell resolves clicks to links itself, which Servo needs to
  do instead via `InputEvent`.
- **Nothing is wired into `wat-shell` yet.** `ServoEngine` implements the trait
  and compiles against Servo; choosing it at runtime is the next step and needs
  the shell moved off `Session` and onto `dyn Engine`.

## Cost of the swap

What it buys: real web compatibility. What it costs is the reason WAT existed —
a JavaScript engine, an HTML parser, a CSS cascade, a layout engine and a
rasterizer, all written from scratch, become unused for browsing. They are still
what `wat shot` renders headlessly, and still what the theme inspector draws
with, so nothing is deleted; but the browser stops being an independent engine
and becomes a front end for someone else's.
