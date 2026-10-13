//! macOS traffic lights: centred in the bar, fading with it (docs/design.md).

/// The close button's left edge, as in mareader.
const INSET_X: f64 = 20.0;
/// AppKit's gap between button origins, if it can't be measured.
const FALLBACK_SPACING: f64 = 20.0;

/// Where the buttons go, in AppKit's bottom-up coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The titlebar container: its bottom edge and height.
    pub container_y: f64,
    pub container_h: f64,
    /// Each button's origin inside the container.
    pub xs: [f64; 3],
    pub y: f64,
}

/// Centres buttons of `button_h` in a bar of `bar_h` at the window's top.
pub fn layout(window_h: f64, bar_h: f64, button_h: f64, spacing: f64) -> Layout {
    let finite = |v: f64, or: f64| if v.is_finite() { v } else { or };
    let bar_h = finite(bar_h, 0.0).max(0.0);
    let button_h = finite(button_h, 0.0).clamp(0.0, bar_h);
    let spacing = finite(spacing, 0.0);
    // A zero or reversed gap means the buttons weren't laid out yet.
    let spacing = if spacing < 1.0 {
        FALLBACK_SPACING
    } else {
        spacing
    };
    Layout {
        container_y: finite(window_h, 0.0) - bar_h,
        container_h: bar_h,
        xs: [0.0, 1.0, 2.0].map(|i| INSET_X + i * spacing),
        y: ((bar_h - button_h) / 2.0).round(),
    }
}

#[cfg(all(target_os = "macos", feature = "traffic-lights"))]
pub use mac::place;

#[cfg(all(target_os = "macos", feature = "traffic-lights"))]
#[allow(unsafe_code)]
mod mac {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSButton, NSView, NSWindow, NSWindowButton};
    use objc2_foundation::NSPoint;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Positions and fades the lights; full screen is left to AppKit.
    pub fn place(frame: &eframe::Frame, opacity: f32, fullscreen: bool) {
        // AppKit objects may only be touched on the main thread.
        if MainThreadMarker::new().is_none() {
            return;
        }
        let Some(window) = ns_window(frame) else {
            return;
        };
        let Some(buttons) = buttons(&window) else {
            return;
        };
        let opacity = if fullscreen { 1.0 } else { opacity };
        fade(&buttons, f64::from(opacity.clamp(0.0, 1.0)));
        if !fullscreen && opacity > 0.0 {
            arrange(&window, &buttons);
        }
    }

    fn ns_window(frame: &eframe::Frame) -> Option<Retained<NSWindow>> {
        let handle = frame.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` lives while borrowed; main thread checked.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    fn buttons(window: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            window.standardWindowButton(NSWindowButton::CloseButton)?,
            window.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            window.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// Hidden at zero, so invisible buttons can't take clicks.
    fn fade(buttons: &[Retained<NSButton>; 3], alpha: f64) {
        for b in buttons {
            if (b.alphaValue() - alpha).abs() > 0.001 {
                b.setAlphaValue(alpha);
            }
            if b.isHidden() != (alpha <= 0.0) {
                b.setHidden(alpha <= 0.0);
            }
        }
    }

    fn arrange(window: &NSWindow, buttons: &[Retained<NSButton>; 3]) {
        // SAFETY: main-thread getters; the grandparent is the container.
        let container = unsafe { buttons[0].superview().and_then(|v| v.superview()) };
        let Some(container) = container else {
            return;
        };
        let close = buttons[0].frame();
        let spacing = buttons[1].frame().origin.x - close.origin.x;
        let want = super::layout(
            window.frame().size.height,
            f64::from(mareader_ui::theme::TITLE_BAR_H),
            close.size.height,
            spacing,
        );
        // AppKit resets these on resize, so write only what moved.
        let mut rect = container.frame();
        if (rect.origin.y - want.container_y).abs() > 0.5
            || (rect.size.height - want.container_h).abs() > 0.5
        {
            rect.origin.y = want.container_y;
            rect.size.height = want.container_h;
            container.setFrame(rect);
        }
        for (b, x) in buttons.iter().zip(want.xs) {
            let at = b.frame().origin;
            if (at.x - x).abs() > 0.5 || (at.y - want.y).abs() > 0.5 {
                b.setFrameOrigin(NSPoint { x, y: want.y });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lights_sit_centred_in_the_bar() {
        let l = layout(800.0, 48.0, 16.0, 20.0);
        assert_eq!(l.container_y, 752.0);
        assert_eq!(l.container_h, 48.0);
        assert_eq!(l.y, 16.0);
        assert_eq!(l.xs, [20.0, 40.0, 60.0]);
    }

    #[test]
    fn they_fit_the_room_the_bar_leaves() {
        let l = layout(800.0, 48.0, 16.0, 20.0);
        let right = l.xs[2] + 14.0;
        assert!(right < f64::from(mareader_ui::platform::Platform::Mac.leading_inset()));
    }

    #[test]
    fn an_unmeasured_gap_falls_back() {
        for gap in [0.0, -5.0, f64::NAN] {
            assert_eq!(layout(800.0, 48.0, 16.0, gap).xs[1], 40.0);
        }
    }

    #[test]
    fn bad_sizes_stay_finite() {
        let l = layout(f64::NAN, 48.0, f64::INFINITY, 20.0);
        assert!(l.container_y.is_finite() && l.y.is_finite());
        assert_eq!(layout(800.0, 48.0, 90.0, 20.0).y, 0.0);
    }
}
