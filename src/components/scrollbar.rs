use iocraft::prelude::*;

// ---------------------------------------------------------------------------
// Scroll metadata
// ---------------------------------------------------------------------------

/// Carries scroll metadata used to render a scrollbar.
#[derive(Debug, Clone, Copy)]
pub struct ScrollInfo {
    pub scroll_offset: usize,
    pub visible_count: usize,
    pub total_count: usize,
}

impl ScrollInfo {
    /// Returns `true` when the content overflows the visible area.
    pub fn needs_scrollbar(&self) -> bool {
        self.total_count > self.visible_count
    }

    /// Compute the thumb start position and size within `track_height` rows.
    ///
    /// Returns `(thumb_start, thumb_size)` where both are in rows. The thumb is
    /// always at least 1 row tall and positioned proportionally to the scroll
    /// offset.
    pub fn thumb_geometry(&self, track_height: u32) -> (u32, u32) {
        if track_height == 0 || !self.needs_scrollbar() {
            return (0, track_height);
        }

        #[allow(clippy::cast_precision_loss)]
        let ratio = self.visible_count as f64 / self.total_count as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let thumb_size = (ratio * f64::from(track_height)).round().max(1.0) as u32;
        let thumb_size = thumb_size.min(track_height);

        let max_scroll = self.total_count.saturating_sub(self.visible_count);
        let available = track_height.saturating_sub(thumb_size);

        #[allow(clippy::cast_precision_loss)]
        let thumb_start = if max_scroll == 0 {
            0
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let start = (self.scroll_offset as f64 / max_scroll as f64 * f64::from(available))
                .round() as u32;
            start.min(available)
        };

        (thumb_start, thumb_size)
    }
}

// ---------------------------------------------------------------------------
// Scrollbar component
// ---------------------------------------------------------------------------

// const TRACK_CHAR: &str = "│"; // U+2502 box-drawing light vertical
const TRACK_CHAR: &str = "░"; // U+2591 light shade
const THUMB_CHAR: &str = "█"; // U+2588 full block

#[derive(Default, Props)]
pub struct ScrollbarProps {
    pub scroll_info: Option<ScrollInfo>,
    pub track_height: u32,
    pub track_color: Option<Color>,
    pub thumb_color: Option<Color>,
}

#[component]
pub fn Scrollbar(props: &mut ScrollbarProps) -> impl Into<AnyElement<'static>> {
    let info = props.scroll_info.take();
    let track_height = props.track_height;
    let track_color = props.track_color.unwrap_or(Color::DarkGrey);
    let thumb_color = props.thumb_color.unwrap_or(Color::White);

    let show = info.is_some_and(|i| i.needs_scrollbar());
    if !show || track_height == 0 {
        return element! { View(width: 0u32) }.into_any();
    }

    let info = info.unwrap();
    let (thumb_start, thumb_size) = info.thumb_geometry(track_height);

    let cells: Vec<(usize, &str, Color)> = (0..track_height)
        .map(|row| {
            let in_thumb = row >= thumb_start && row < thumb_start + thumb_size;
            let (ch, color) = if in_thumb {
                (THUMB_CHAR, thumb_color)
            } else {
                (TRACK_CHAR, track_color)
            };
            (row as usize, ch, color)
        })
        .collect();

    element! {
        View(flex_direction: FlexDirection::Column, width: 1u32) {
            #(cells.into_iter().map(|(key, ch, color)| {
                element! {
                    View(key) {
                        Text(content: ch, color: color)
                    }
                }
            }))
        }
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scrollbar_when_content_fits() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 20,
            total_count: 10,
        };
        assert!(!info.needs_scrollbar());
    }

    #[test]
    fn no_scrollbar_when_equal() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 10,
            total_count: 10,
        };
        assert!(!info.needs_scrollbar());
    }

    #[test]
    fn needs_scrollbar_when_overflows() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 10,
            total_count: 20,
        };
        assert!(info.needs_scrollbar());
    }

    #[test]
    fn thumb_at_top_when_offset_zero() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 10,
            total_count: 100,
        };
        let (start, size) = info.thumb_geometry(20);
        assert_eq!(start, 0);
        assert!(size >= 1);
        assert!(size <= 20);
    }

    #[test]
    fn thumb_at_bottom_when_max_offset() {
        let info = ScrollInfo {
            scroll_offset: 90,
            visible_count: 10,
            total_count: 100,
        };
        let (start, size) = info.thumb_geometry(20);
        assert_eq!(start + size, 20);
    }

    #[test]
    fn thumb_proportional_to_viewport() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 50,
            total_count: 100,
        };
        let (_, size) = info.thumb_geometry(20);
        assert_eq!(size, 10);
    }

    #[test]
    fn thumb_min_size_one() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 1,
            total_count: 10000,
        };
        let (_, size) = info.thumb_geometry(20);
        assert_eq!(size, 1);
    }

    #[test]
    fn zero_track_height() {
        let info = ScrollInfo {
            scroll_offset: 0,
            visible_count: 10,
            total_count: 100,
        };
        let (start, size) = info.thumb_geometry(0);
        assert_eq!(start, 0);
        assert_eq!(size, 0);
    }
}
