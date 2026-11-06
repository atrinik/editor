// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain drag brush geometry and gesture accumulation. A stroke is local
//! input only: it collects the cells one press-move-release gesture covers and
//! leaves this module as a single [`Action::PaintStroke`]. Session prepares it
//! as one reviewed World `terrain.set` transaction; nothing is written here.
use crate::{Action, tools::ToolError};
use std::collections::BTreeSet;

/// Largest brush radius the editor offers, in cells (a 17 x 17 square).
pub const MAXIMUM_RADIUS: u16 = 8;

/// Most distinct cells one stroke may cover. It equals the preview's default
/// view cell budget, so a stroke clipped to the view cannot normally reach it.
pub const MAXIMUM_STROKE_CELLS: usize = 4096;

/// Most cell steps between two pointer samples that a stroke fills in. A jump
/// further than this stamps only where the pointer lands.
pub const MAXIMUM_SEGMENT: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrushShape {
    #[default]
    Square,
    Circle,
}
impl BrushShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Circle => "circle",
        }
    }
}

/// A brush footprint: a square or a disc of cells around the pointer's cell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Brush {
    pub shape: BrushShape,
    /// Cells from the centre to the edge; 0 paints one cell. Values above
    /// [`MAXIMUM_RADIUS`] count as that radius.
    pub radius: u16,
}
impl Brush {
    /// The cells the brush covers around `center`, row by row. Cells beyond
    /// the integer range are left out; callers clip to the editable view.
    pub fn footprint(self, [x, y]: [i64; 2]) -> impl Iterator<Item = [i64; 2]> {
        let radius = i64::from(self.radius.min(MAXIMUM_RADIUS));
        let shape = self.shape;
        (-radius..=radius).flat_map(move |dy| {
            (-radius..=radius).filter_map(move |dx| {
                // A disc of cells whose centres lie within half a cell past
                // the radius, so small discs are round rather than diamonds.
                let inside = match shape {
                    BrushShape::Square => true,
                    BrushShape::Circle => dx * dx + dy * dy <= radius * radius + radius,
                };
                if !inside {
                    return None;
                }
                Some([x.checked_add(dx)?, y.checked_add(dy)?])
            })
        })
    }
    /// Cells across the footprint, for display.
    pub fn diameter(self) -> u32 {
        u32::from(self.radius.min(MAXIMUM_RADIUS)) * 2 + 1
    }
    #[must_use]
    pub fn resized(self, delta: i32) -> Self {
        let radius = (i32::from(self.radius.min(MAXIMUM_RADIUS)) + delta)
            .clamp(0, i32::from(MAXIMUM_RADIUS));
        Self {
            shape: self.shape,
            radius: u16::try_from(radius).unwrap_or(MAXIMUM_RADIUS),
        }
    }
    #[must_use]
    pub fn toggled_shape(self) -> Self {
        Self {
            shape: match self.shape {
                BrushShape::Square => BrushShape::Circle,
                BrushShape::Circle => BrushShape::Square,
            },
            radius: self.radius,
        }
    }
}

/// The cells of the line from `from` to `to`, both included, in order. A line
/// longer than [`MAXIMUM_SEGMENT`] steps is just `to`.
#[must_use]
pub fn segment(from: [i64; 2], to: [i64; 2]) -> Vec<[i64; 2]> {
    let [x0, y0] = from.map(i128::from);
    let [x1, y1] = to.map(i128::from);
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    if dx.max(-dy) > i128::from(MAXIMUM_SEGMENT) {
        return vec![to];
    }
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut error) = (x0, y0, dx + dy);
    let mut cells = Vec::new();
    loop {
        // Every cell lies between the two ends, so it fits in an i64.
        if let (Ok(cx), Ok(cy)) = (i64::try_from(x), i64::try_from(y)) {
            cells.push([cx, cy]);
        }
        if x == x1 && y == y1 {
            return cells;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

/// One drag gesture on one terrain grid, from press to release.
#[derive(Clone, Debug)]
pub struct Stroke {
    plane: String,
    layer: i32,
    grid: String,
    proto: String,
    brush: Brush,
    /// The committed World revision the author inspected at press.
    expected: [u8; 32],
    /// The frame whose exact cell mask supplies this stroke's samples.
    generation: u64,
    /// Editable view bounds `[min_x, min_y, width, height]`.
    bounds: [i64; 4],
    last: Option<[i64; 2]>,
    cells: BTreeSet<[i64; 2]>,
    capped: bool,
}
impl Stroke {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        plane: String,
        layer: i32,
        grid: String,
        proto: String,
        brush: Brush,
        expected: [u8; 32],
        generation: u64,
        bounds: [i64; 4],
    ) -> Self {
        Self {
            plane,
            layer,
            grid,
            proto,
            brush,
            expected,
            generation,
            bounds,
            last: None,
            cells: BTreeSet::new(),
            capped: false,
        }
    }
    fn in_bounds(&self, [x, y]: [i64; 2]) -> bool {
        let [min_x, min_y, width, height] = self.bounds;
        x.checked_sub(min_x).is_some_and(|v| v >= 0 && v < width)
            && y.checked_sub(min_y).is_some_and(|v| v >= 0 && v < height)
    }
    /// Stamp the brush at `cell` and along the line from the previous sample,
    /// clipping footprints to the editable view. Samples that leave the view
    /// do not reset the line, so a fast drag along a jagged edge stays joined.
    /// A stamp that would exceed [`MAXIMUM_STROKE_CELLS`] adds nothing and
    /// marks the stroke capped. Returns how many cells the stamp added.
    pub(crate) fn stamp(&mut self, cell: [i64; 2]) -> usize {
        let path = segment(self.last.unwrap_or(cell), cell);
        self.last = Some(cell);
        let mut added = BTreeSet::new();
        for center in path {
            for covered in self.brush.footprint(center) {
                if self.in_bounds(covered) && !self.cells.contains(&covered) {
                    added.insert(covered);
                }
            }
        }
        if self.cells.len() + added.len() > MAXIMUM_STROKE_CELLS {
            self.capped = true;
            return 0;
        }
        let count = added.len();
        self.cells.extend(added);
        count
    }
    pub fn plane(&self) -> &str {
        &self.plane
    }
    pub fn layer(&self) -> i32 {
        self.layer
    }
    pub fn grid(&self) -> &str {
        &self.grid
    }
    pub fn prototype(&self) -> &str {
        &self.proto
    }
    pub fn brush(&self) -> Brush {
        self.brush
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Covered cells in `[x, y]` order.
    pub fn cells(&self) -> impl ExactSizeIterator<Item = [i64; 2]> + '_ {
        self.cells.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.cells.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    /// Whether a stamp was dropped because the stroke reached its cell cap.
    pub fn capped(&self) -> bool {
        self.capped
    }
    pub(crate) fn into_action(self) -> Result<Action, ToolError> {
        if self.cells.is_empty() {
            return Err(ToolError("Brush stroke covered no editable cell"));
        }
        Ok(Action::PaintStroke {
            plane: self.plane,
            layer: self.layer,
            grid: self.grid,
            cells: self.cells.into_iter().collect(),
            proto: self.proto,
            expected: self.expected,
        })
    }
}
