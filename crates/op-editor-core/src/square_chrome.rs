//! Square-corner chrome — the Figma-style "no radius" editor shell.
//!
//! Chrome widgets are written with rounded radii in hundreds of places. A
//! single switch makes the *backends* square them off while the host paints
//! the editor chrome, so no widget needs its own radius edit and design
//! content (canvas nodes, thumbnails, exports) keeps its authored corners.
//!
//! - The host paint pass turns the switch on for the frame ([`SquareChrome::enter`]).
//! - Canvas content painting turns it back off ([`SquareChrome::suspend`]).
//! - Backends route every round-rect call through [`square_radius`] /
//!   [`square_radii`].
//!
//! True circles (a square box whose radius reaches half its side — radio
//! dots, avatars, switch knobs, status dots) are kept: they are shapes, not
//! corners. Pills (wider than tall) are squared like everything else.

use std::cell::Cell;

// Per-thread, not process-global: a paint pass runs start to finish on one
// thread, and concurrent paints (tests, offscreen renders) must not flip each
// other's switch.
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Whether chrome corners are currently being squared.
pub fn is_active() -> bool {
    ACTIVE.with(Cell::get)
}

/// RAII switch: sets the flag and restores the previous value on drop.
pub struct SquareChrome {
    previous: bool,
}

impl SquareChrome {
    /// Square chrome corners until the guard drops.
    pub fn enter() -> Self {
        Self::set(true)
    }

    /// Keep authored corners (design content) until the guard drops.
    pub fn suspend() -> Self {
        Self::set(false)
    }

    fn set(on: bool) -> Self {
        Self {
            previous: ACTIVE.with(|active| active.replace(on)),
        }
    }
}

impl Drop for SquareChrome {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(self.previous));
    }
}

const CIRCLE_SLACK: f32 = 0.75;

fn is_circle(width: f32, height: f32, radius: f32) -> bool {
    (width - height).abs() <= CIRCLE_SLACK && radius >= width.min(height) / 2.0 - CIRCLE_SLACK
}

/// The radius to paint for a `width` x `height` box asked to use `radius`.
pub fn square_radius(width: f32, height: f32, radius: f32) -> f32 {
    if !is_active() || radius <= 0.0 || is_circle(width, height, radius) {
        radius
    } else {
        0.0
    }
}

/// Per-corner variant of [`square_radius`].
pub fn square_radii(width: f32, height: f32, radii: [f32; 4]) -> [f32; 4] {
    if !is_active() || radii.iter().all(|r| *r <= 0.0) {
        return radii;
    }
    if radii.iter().all(|r| is_circle(width, height, *r)) {
        return radii;
    }
    [0.0; 4]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squares_corners_only_while_active_and_keeps_circles() {
        assert_eq!(square_radius(100.0, 40.0, 8.0), 8.0, "off by default");
        {
            let _on = SquareChrome::enter();
            assert_eq!(square_radius(100.0, 40.0, 8.0), 0.0);
            assert_eq!(square_radius(100.0, 40.0, 20.0), 0.0, "pills are squared");
            assert_eq!(square_radius(12.0, 12.0, 6.0), 6.0, "circles stay round");
            assert_eq!(square_radii(100.0, 40.0, [8.0, 8.0, 0.0, 0.0]), [0.0; 4]);
            {
                let _content = SquareChrome::suspend();
                assert_eq!(square_radius(100.0, 40.0, 8.0), 8.0, "content keeps radius");
            }
            assert_eq!(
                square_radius(100.0, 40.0, 8.0),
                0.0,
                "restored after suspend"
            );
        }
        assert_eq!(square_radius(100.0, 40.0, 8.0), 8.0, "restored after enter");
    }
}
