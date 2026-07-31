# Contributing

Thanks for looking. This is a browser engine built from scratch, which means
there is a lot of surface area and plenty of it is unfinished — see the
"What is not" section of the README and the deviations list in
`docs/ARCHITECTURE.md` for the honest state of things.

## Getting set up

```sh
cargo test --workspace     # the whole suite, no network needed
cargo run --release        # open the browser
cargo run --release -- shot about:home -o out.png   # see a change without a window
```

On Linux, the window shell needs X11 or Wayland development headers:

```sh
sudo apt-get install libxkbcommon-dev libwayland-dev libx11-dev
```

## Before opening a pull request

* `cargo test --workspace` passes.
* `cargo clippy --workspace --all-targets` is clean.
* `cargo fmt --all` has been run.
* New behaviour has tests. The engine is only trustworthy because it is covered,
  and a rendering bug that no test would have caught tends to come back.

## Where things live

Each crate's `lib.rs` says what it is for; read that first. The pipeline is
described in `docs/ARCHITECTURE.md`, which is the fastest way to work out which
crate a change belongs in.

## Writing tests for rendering code

* Layout tests use `wat_text::FontStore::empty()`, whose synthetic metrics make
  every glyph exactly half the font size wide. That makes line breaking and
  alignment assertable to the pixel on any machine.
* Painting tests assert on display lists when structure is the point, and on
  pixels when the visual result is the point.
* Anything that would need the network uses `wat_net::StaticLoader`.

## Things worth doing

* Real table layout, replacing the flex approximation.
* Floats that line boxes actually flow around.
* Grid areas and explicit placement.
* Group opacity, and `transform` scale and rotation in the painter.
* HiDPI: render at the device scale factor.
* Reading the platform light/dark preference so `appearance = "auto"` works.
* Exercising the Android and iOS entry points on real devices.

If you are starting something large, open an issue first so the work is not
duplicated.
