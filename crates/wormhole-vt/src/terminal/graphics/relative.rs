//! Relative placements (`P`, `Q`, `H`, `V`) on a `put`.
//!
//! Spec: kitty, "Relative placements". The grid keeps the relation and works out where a child is
//! when it is read (`grid/images/relative.rs` has the rules); this is the part that needs the
//! picture table: finding the parent, answering the errors, and finding a virtual parent's
//! placeholders.
//!
//! - `ENOPARENT`: no picture with id `P`, or it has no placement (real or virtual) with id `Q`
//!   (0 meaning the one without an id).
//! - `ECYCLE`: the placement would be its own ancestor (including its own parent).
//! - `ETOODEEP`: more than 8 ancestors (kitty's `PARENT_DEPTH_LIMIT`; the protocol asks for at least 8).
//! - `EINVAL`: a virtual placement (`U=1`) made relative.
//! - The cursor never moves and nothing scrolls, whatever `C` says; the child is not cut at the
//!   bottom of the screen the way a placement at the cursor is.
//!
//! A real placement of the same name is preferred to a virtual one when both could be the
//! parent. A virtual parent is where its placeholders are: the top left of the cells in the view
//! (the live screen, or the history if the view is scrolled up) that carry its picture id and
//! placement id; none in view, no child shown.

use crate::grid::{NewPlacement, Parent, Refused, Relative};
use crate::terminal::Terminal;
use crate::types::{Cell, ImageHandle, ImagePlacement, Line, RowExtras};

use super::command::Command;
use super::geometry::Geometry;
use super::placeholder::placeholder_at;
use super::reply::Error;

impl Terminal {
    /// `a=p` or `a=T` with `P`: place the picture `handle` relative to a parent.
    pub(super) fn put_relative(
        &mut self,
        slot: usize,
        (handle, image_id): (ImageHandle, u32),
        command: &Command,
        geometry: Geometry,
    ) -> Result<(), Error> {
        if command.unicode_placeholder {
            return Err(Error::Invalid("a virtual placement cannot be relative"));
        }
        let table = &self.graphics.tables[slot];
        let own = (handle, command.placement);
        let parent_image = table.by_id(command.parent_image).and_then(|i| table.get(i));
        // A placement that names itself is a cycle whether or not it exists yet.
        if parent_image.is_some_and(|p| (p.handle, command.parent_placement) == own) {
            return Err(Error::Cycle);
        }
        let parent_image = parent_image.ok_or(Error::NoParent)?;
        let grid = self.grid_of(slot);
        let parent = if grid.has_placement(parent_image.handle, command.parent_placement) {
            Parent::Placement { image: parent_image.handle, id: command.parent_placement }
        } else if parent_image.virtuals.iter().any(|v| v.placement_id == command.parent_placement) {
            Parent::Virtual { image_id: parent_image.id, id: command.parent_placement }
        } else {
            return Err(Error::NoParent);
        };
        grid.check_relative(handle, command.placement, parent).map_err(|refused| match refused {
            Refused::Cycle => Error::Cycle,
            Refused::TooDeep => Error::TooDeep,
        })?;
        let new = NewPlacement {
            image: handle,
            image_id,
            id: command.placement,
            cols: geometry.cols,
            rows: geometry.rows,
            source: geometry.source,
            offset_x: geometry.offset_x,
            offset_y: geometry.offset_y,
            extent: geometry.extent,
            z: command.z,
        };
        self.grid_of_mut(slot)
            .place_relative(new, Relative { parent, dx: command.offset_h, dy: command.offset_v });
        Ok(())
    }

    /// The placements in view, with the ones relative to a virtual placement positioned from its
    /// placeholders. Reads the view's rows, so it is only used while such a child exists.
    pub(in crate::terminal) fn placements_with_placeholders(&self, out: &mut Vec<ImagePlacement>) {
        let wanted = self.grid.virtual_parents();
        let mut found: Vec<((u32, u32), (i64, i64))> = Vec::new();
        let first = -(self.grid.view_offset() as i64);
        let (mut cells, mut extras): (Vec<Cell>, RowExtras) = (Vec::new(), RowExtras::default());
        for line in first..first + self.rows as i64 {
            let line = Line(line.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
            extras.clear();
            self.grid.copy_row(line, &mut cells, Some(&mut extras));
            for (col, cell) in cells.iter().enumerate() {
                let Some(holder) = placeholder_at(cell, extras.cell(col)) else { continue };
                let key = (holder.image_id, holder.placement_id);
                if !wanted.contains(&key) {
                    continue;
                }
                let at = (i64::from(line.0), col as i64);
                match found.iter_mut().find(|(k, _)| *k == key) {
                    // The top left of all of them: the least line, and the least column.
                    Some((_, min)) => *min = (min.0.min(at.0), min.1.min(at.1)),
                    None => found.push((key, at)),
                }
            }
        }
        self.grid.copy_placements_with(out, self.cell_px, &|image_id, id| {
            found.iter().find(|(k, _)| *k == (image_id, id)).map(|(_, at)| *at)
        });
    }
}
