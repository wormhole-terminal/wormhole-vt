//! Reading placements: what the host draws, and what a delete by position asks about.

use crate::types::{Extent, ImageHandle, ImagePlacement, Line, SourceRect};

use super::{Grid, Placement, VirtualOrigin};

/// A placement as a delete asks about it. Lines count as everywhere (0 is the live screen's top
/// row), but as `i64` so that a picture far up in the history is still a number.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    /// The picture.
    pub image: ImageHandle,
    /// The placement's id, 0 for none.
    pub placement_id: u32,
    /// Stacking order.
    pub z: i32,
    /// The top row.
    pub line: i64,
    /// Rows covered.
    pub rows: usize,
    /// The left column.
    pub column: usize,
    /// Columns covered, cut to the screen's width.
    pub cols: usize,
    screen_rows: usize,
}

impl Hit {
    /// Whether the placement covers the cell.
    pub fn covers(&self, line: i64, column: usize) -> bool {
        self.spans_line(line) && self.spans_column(column)
    }

    /// Whether the placement covers any cell of the row.
    pub fn spans_line(&self, line: i64) -> bool {
        line >= self.line && line < self.line + self.rows as i64
    }

    /// Whether the placement covers any cell of the column.
    pub fn spans_column(&self, column: usize) -> bool {
        column >= self.column && column < self.column + self.cols
    }

    /// Whether any row of the placement is on the live screen (not only in the history).
    pub fn on_screen(&self) -> bool {
        self.line < self.screen_rows as i64 && self.line + self.rows as i64 > 0
    }
}

/// A placement as a delete asks about it; `origin` is where it starts (its top row's serial and
/// column, see `Grid::origin`). One that is not shown covers nothing.
pub(super) fn hit(
    p: &Placement,
    origin: Option<(i64, i64)>,
    top: i64,
    screen_rows: usize,
    grid_cols: usize,
) -> Hit {
    let shown = origin.filter(|&(_, col)| col >= 0 && col < grid_cols as i64);
    let Some((serial, col)) = shown else {
        return Hit {
            image: p.spec.image,
            placement_id: p.spec.id,
            z: p.spec.z,
            line: i64::MIN / 2,
            rows: 0,
            column: 0,
            cols: 0,
            screen_rows,
        };
    };
    let col = col as usize;
    Hit {
        image: p.spec.image,
        placement_id: p.spec.id,
        z: p.spec.z,
        line: serial - top,
        rows: p.spec.rows,
        column: col,
        cols: p.spec.cols.min(grid_cols.saturating_sub(col)),
        screen_rows,
    }
}

impl Grid {
    /// The placements that touch the view (the live screen, or the history if the view is
    /// scrolled up), lowest `z` first and the oldest first within one `z`, into `out`. `cell` is
    /// the size of a cell in pixels, which a placement cut at the screen's edge needs to know how
    /// much of its picture is left.
    pub fn copy_placements(&self, out: &mut Vec<ImagePlacement>, cell: (u16, u16)) {
        self.copy_placements_with(out, cell, &|_, _| None);
    }

    /// [`copy_placements`](Self::copy_placements) for a terminal that can say where the
    /// placeholders of a virtual placement are, for the placements relative to one.
    pub(crate) fn copy_placements_with(
        &self,
        out: &mut Vec<ImagePlacement>,
        cell: (u16, u16),
        virtual_origin: VirtualOrigin<'_>,
    ) {
        out.clear();
        if self.placements.is_empty() {
            return;
        }
        let top = self.top_serial() as i64;
        let first = -(self.offset as i64);
        let last = first + self.rows as i64 - 1;
        for (i, p) in self.placements.list.iter().enumerate() {
            let Some((serial, col)) = self.origin(i, virtual_origin) else { continue };
            // A column left of the screen or right of it has no cell to start in.
            if col < 0 || col >= self.cols as i64 {
                continue;
            }
            let line = serial - top;
            if line.saturating_add(p.spec.rows as i64) - 1 < first || line > last {
                continue;
            }
            if let Some(shown) = shown(p, line, col as usize, self.cols, cell) {
                out.push(shown);
            }
        }
        out.sort_by_key(|p| p.z);
    }
}

/// The placement as drawn: cut at the right edge of a screen `grid_cols` wide.
fn shown(
    p: &Placement,
    line: i64,
    column: usize,
    grid_cols: usize,
    (cell_w, _): (u16, u16),
) -> Option<ImagePlacement> {
    let spec = &p.spec;
    let cols = spec.cols.min(grid_cols.checked_sub(column)?);
    let mut source = spec.source;
    let mut extent = spec.extent;
    if cols < spec.cols {
        let cell_w = u64::from(cell_w.max(1));
        match extent {
            Extent::Pixels { width, height } => {
                let room = (cols as u64 * cell_w).saturating_sub(u64::from(spec.offset_x));
                let kept = room.min(u64::from(width)).max(1);
                source.width = scale(source.width, kept, u64::from(width));
                extent = Extent::Pixels { width: kept as u32, height };
            }
            Extent::Fill | Extent::Fit => {
                source.width = scale(source.width, cols as u64, spec.cols as u64);
            }
        }
    }
    Some(ImagePlacement {
        image: spec.image,
        image_id: spec.image_id,
        placement_id: spec.id,
        line: Line(line.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32),
        column,
        cols,
        rows: spec.rows,
        source: SourceRect { ..source },
        offset_x: spec.offset_x,
        offset_y: spec.offset_y,
        extent,
        z: spec.z,
    })
}

/// `value * num / den`, at least 1.
fn scale(value: u32, num: u64, den: u64) -> u32 {
    (u64::from(value) * num / den.max(1)).clamp(1, u64::from(u32::MAX)) as u32
}
