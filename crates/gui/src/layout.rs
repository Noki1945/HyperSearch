//! Main-window geometry in device-independent pixels; shared with regression tests.
use super::app::*;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Box {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Box {
    #[cfg(test)]
    pub fn intersects(self, b: Self) -> bool {
        self.x < b.x + b.w && b.x < self.x + self.w && self.y < b.y + b.h && b.y < self.y + self.h
    }
}
pub struct Layout {
    pub controls: Vec<(usize, Box)>,
    pub details: bool,
    pub columns: [i32; 3],
}
pub fn main(w: i32, h: i32, filters: bool, show_details: bool, hide_details: bool) -> Layout {
    let w = w.max(600);
    let h = h.max(300);
    let gap = 8;
    let side = if w < 900 { 164 } else { 184 };
    let x = side + 24;
    let right = w - 12;
    let content = right - x;
    let details = h >= 500 && !hide_details && (w >= 1100 || (show_details && w >= 850));
    let mut controls = vec![];
    let mut add = |id, x, y, w, h| controls.push((id, Box { x, y, w, h }));
    for (id, x) in [(BACK, 12), (FORWARD, 52), (UP, 92)] {
        add(id, x, 12, 32, 36);
    }
    let available = w - 256;
    let address = available * 52 / 100;
    add(ADDRESS, 136, 12, address, 36);
    add(QUERY, 144 + address, 12, available - address, 36);
    add(FILTERS, w - 100, 12, 40, 36);
    add(SETTINGS, w - 52, 12, 40, 36);
    let mut cursor = x;
    if w >= 1000 {
        for (i, width) in [48, 116, 102, 114, 62, 64, 66].iter().enumerate() {
            add(CATEGORY_CHIPS + i, cursor, 64, *width, 30);
            cursor += *width + 4;
        }
    }
    let buttons = if w >= 1000 {
        vec![(SORT, 80), (VIEW, 48), (INFO, 36), (SPOTLIGHT, 100)]
    } else {
        vec![(SORT, 100), (VIEW, 60), (INFO, 36), (SPOTLIGHT, 132)]
    };
    for (id, width) in buttons {
        if cursor + width <= right {
            add(id, cursor, 64, width, 30);
            cursor += width + 6;
        }
    }
    let mut top = 108;
    if filters {
        let mut left = x;
        for (id, width) in [(CATEGORY, 128), (EXTENSION, 88), (MODE, 124), (RANK, 140)] {
            if left + width > right {
                left = x;
                top += 40;
            }
            add(id, left, top, width, 32);
            left += width + gap;
        }
        top += 44;
    }
    let bottom = h - 48;
    let preview = if details { 256 } else { 0 };
    let listw = content - if details { preview + 12 } else { 0 };
    add(SIDEBAR, 12, 64, side, bottom - 64);
    add(OUTPUT, x, top, listw, bottom - top);
    if details {
        add(PREVIEW_ICON, right - preview + 10, top + 12, 48, 48);
        add(
            PREVIEW_TITLE,
            right - preview + 68,
            top + 12,
            preview - 78,
            36,
        );
        add(
            DETAILS,
            right - preview + 10,
            top + 64,
            preview - 20,
            (bottom - top - 200).max(32),
        );
        for (i, id) in [OPEN, LOCATION, COPY].iter().enumerate() {
            add(
                *id,
                right - preview + 10,
                bottom - 120 + i as i32 * 38,
                preview - 20,
                32,
            );
        }
    }
    add(
        EMPTY,
        x + 16,
        top + 32,
        (listw - 32).max(100),
        (bottom - top - 40).clamp(16, 72),
    );
    add(STATUS, 12, h - 36, w - 144, 28);
    add(NEXT, w - 124, h - 36, 112, 28);
    let name = (listw * 36 / 100).max(120);
    let kind = (listw * 19 / 100).clamp(64, 130);
    Layout {
        controls,
        details,
        columns: [name, (listw - name - kind - 24).max(80), kind],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_resolutions_dpi_and_resize_have_no_overlaps() {
        for (px, py) in [(1280, 720), (1920, 1080), (2560, 1440), (800, 600)] {
            for dpi in [96, 120, 144, 192] {
                let w = (px * 96 / dpi).max(600);
                let h = (py * 96 / dpi).max(300);
                for filters in [false, true] {
                    let l = main(w, h, filters, false, false);
                    for (id, r) in &l.controls {
                        assert!(
                            r.x >= 0 && r.y >= 0 && r.w > 0 && r.h > 0,
                            "{id}: {r:?} at {w}x{h}"
                        );
                        assert!(r.x + r.w <= w && r.y + r.h <= h, "{id}: {r:?} at {w}x{h}");
                        for (other, b) in &l.controls {
                            if id < other && *id != EMPTY && *other != EMPTY {
                                assert!(!r.intersects(*b), "overlap {id} {other} at {w}x{h}");
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn preview_is_responsive_and_explicitly_hideable() {
        assert!(!main(900, 600, false, false, false).details);
        assert!(main(1200, 700, false, false, false).details);
        assert!(!main(1200, 700, false, false, true).details);
        assert!(main(900, 600, false, true, false).details);
    }
}
