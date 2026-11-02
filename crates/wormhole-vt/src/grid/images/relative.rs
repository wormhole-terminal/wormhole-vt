//! Relative placements: a placement positioned from another placement, not from the cursor.
//!
//! Spec: kitty, "Relative placements". A child names its parent by the parent picture's id and
//! the parent placement's id and sits `dx` columns and `dy` rows from the parent's top left cell.
//! The reference is by name, not by identity: a parent placed again under the same ids moves, and
//! its children move with it. A parent that is gone takes its children with it.
//!
//! # Rules
//!
//! - A child's own `top` and `col` mean nothing. Where it is gets worked out when it is read
//!   ([`Grid::origin`]), from the parent's, up a chain of at most [`MAX_DEPTH`] parents, so
//!   whatever moves a parent (a scroll, a reflow, a new placement) moves its children, and
//!   nothing else moves a child. The scroll, reflow and cut-off code skips children for that
//!   reason.
//! - A parent may be a virtual placement of Unicode placeholders. Its position is then the
//!   top left of the placeholder cells in view, which the grid cannot read (it is below the
//!   layer that knows what a placeholder is): the terminal hands it in as a [`VirtualOrigin`].
//!   A child whose parent has none is not shown.
//! - A child cut off at the left edge (a negative column) is not shown: a placement has no
//!   column to the left of the screen. One below or above the view is not shown either.
//! - Deleting a parent deletes its children, and theirs ([`Grid::drop_orphans`]). The pictures
//!   that lost their last placement that way are handed back ([`Grid::take_orphaned`]).

use crate::types::ImageHandle;

use super::{Grid, Placement, Placements};

/// Most parents above a placement: kitty's own limit (`PARENT_DEPTH_LIMIT` in its `graphics.c`),
/// and the protocol asks for at least 8.
pub const MAX_DEPTH: usize = 8;

/// What a relative placement hangs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parent {
    /// The placement of this picture with this id (0: the one without an id).
    Placement { image: ImageHandle, id: u32 },
    /// The virtual placement of the picture with this id (kitty's) and this placement id, whose
    /// position is that of its placeholders.
    Virtual { image_id: u32, id: u32 },
}

/// A placement's position relative to a parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Relative {
    pub parent: Parent,
    /// Columns right of the parent's top left cell (negative: left).
    pub dx: i32,
    /// Rows down (negative: up).
    pub dy: i32,
}

/// Where a virtual placement's placeholders start: a line counted from the live screen's top
/// row (negative in the history) and a column. Asked with the picture's id and the placement id.
pub(crate) type VirtualOrigin<'a> = &'a dyn Fn(u32, u32) -> Option<(i64, i64)>;

/// Why a relative placement cannot be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// It would be its own ancestor.
    Cycle,
    /// More than [`MAX_DEPTH`] parents.
    TooDeep,
}

impl Placements {
    /// The newest placement of `image` with this id.
    fn find(&self, image: ImageHandle, id: u32) -> Option<usize> {
        self.list.iter().rposition(|p| p.spec.image == image && p.spec.id == id)
    }

    /// Whether any placement is relative to a placeholder: the terminal has work to do reading
    /// them.
    pub(super) fn has_virtual_children(&self) -> bool {
        self.list.iter().any(|p| matches!(p.rel, Some(Relative { parent: Parent::Virtual { .. }, .. })))
    }
}

impl Grid {
    /// Whether the picture has a placement with this id (0: one without an id).
    pub(crate) fn has_placement(&self, image: ImageHandle, id: u32) -> bool {
        self.placements.find(image, id).is_some()
    }

    /// How far the view is scrolled up into the history, in rows.
    pub(crate) fn view_offset(&self) -> usize {
        self.offset
    }

    /// The (picture id, placement id) of each virtual placement something is relative to.
    pub(crate) fn virtual_parents(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for p in &self.placements.list {
            if let Some(Relative { parent: Parent::Virtual { image_id, id }, .. }) = p.rel
                && !out.contains(&(image_id, id))
            {
                out.push((image_id, id));
            }
        }
        out
    }

    /// Whether anything is placed relative to a virtual placement.
    pub(crate) fn has_virtual_children(&self) -> bool {
        self.placements.has_virtual_children()
    }

    /// Whether a placement of `image` with id `id` may hang on `parent`: it must not be its own
    /// ancestor, and have no more than [`MAX_DEPTH`] ancestors. The parent has to exist
    /// already (a virtual one is never relative itself, and the terminal knows it).
    pub(crate) fn check_relative(&self, image: ImageHandle, id: u32, parent: Parent) -> Result<(), Refused> {
        let mut at = parent;
        let mut depth = 0;
        loop {
            let Parent::Placement { image: at_image, id: at_id } = at else { return Ok(()) };
            if (at_image, at_id) == (image, id) {
                return Err(Refused::Cycle);
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(Refused::TooDeep);
            }
            let Some(next) = self.placements.find(at_image, at_id).and_then(|i| self.placements.list[i].rel)
            else {
                return Ok(());
            };
            at = next.parent;
        }
    }

    /// Place a picture relative to a parent. Everything about `new` is as for
    /// [`place_image`](Grid::place_image) but where: that is `rel`'s. The caller has checked
    /// the parent ([`check_relative`](Self::check_relative)). Replaces the placement of the same
    /// picture and id, and keeps its children.
    pub(crate) fn place_relative(&mut self, new: super::NewPlacement, rel: Relative) {
        let list = &mut self.placements.list;
        if new.id != 0 {
            list.retain(|p| !(p.spec.image == new.image && p.spec.id == new.id));
        }
        if list.len() >= super::MAX_PLACEMENTS {
            list.remove(0);
        }
        list.push(Placement {
            spec: super::NewPlacement { cols: new.cols.max(1), rows: new.rows.max(1), ..new },
            top: 0,
            col: 0,
            rel: Some(rel),
        });
        self.drop_orphans();
        self.all_dirty = true;
    }

    /// Where placement `index` starts: the serial number of its top row, and its column. For a
    /// child, worked out from its parents. `None`: not shown (no parent found, a virtual parent
    /// without placeholders in view, or a chain that is too long).
    pub(super) fn origin(&self, index: usize, virtual_origin: VirtualOrigin<'_>) -> Option<(i64, i64)> {
        let top_serial = self.top_serial() as i64;
        let (mut dx, mut dy) = (0i64, 0i64);
        let mut at = self.placements.list.get(index)?;
        for _ in 0..=MAX_DEPTH + 1 {
            let Some(rel) = at.rel else {
                return Some((at.top.saturating_add(dy), (at.col as i64).saturating_add(dx)));
            };
            dx = dx.saturating_add(i64::from(rel.dx));
            dy = dy.saturating_add(i64::from(rel.dy));
            match rel.parent {
                Parent::Placement { image, id } => {
                    at = self.placements.list.get(self.placements.find(image, id)?)?
                }
                Parent::Virtual { image_id, id } => {
                    let (line, col) = virtual_origin(image_id, id)?;
                    return Some((
                        top_serial.saturating_add(line).saturating_add(dy),
                        col.saturating_add(dx),
                    ));
                }
            }
        }
        None
    }

    /// Delete the children whose parent is gone, and theirs. Returns whether any went. The
    /// pictures of those placements are noted for [`take_orphaned`](Self::take_orphaned).
    pub(super) fn drop_orphans(&mut self) -> bool {
        let mut any = false;
        loop {
            let list = &self.placements.list;
            let gone: Vec<bool> = list
                .iter()
                .map(|p| match p.rel {
                    Some(Relative { parent: Parent::Placement { image, id }, .. }) => {
                        self.placements.find(image, id).is_none()
                    }
                    _ => false,
                })
                .collect();
            if !gone.contains(&true) {
                return any;
            }
            any = true;
            let mut flags = gone.iter();
            let orphaned = &mut self.placements.orphaned;
            self.placements.list.retain(|p| {
                let go = flags.next().copied().unwrap_or(false);
                if go {
                    orphaned.push(p.spec.image);
                }
                !go
            });
            self.all_dirty = true;
        }
    }

    /// The placements relative to a virtual placement of the picture with `image_id` are gone,
    /// because it was (`only` is its placement id) or all of that picture's were.
    pub(crate) fn drop_virtual_children(&mut self, image_id: u32, only: Option<u32>) {
        let orphaned = &mut self.placements.orphaned;
        let before = self.placements.list.len();
        self.placements.list.retain(|p| {
            let hit = matches!(
                p.rel,
                Some(Relative { parent: Parent::Virtual { image_id: i, id }, .. })
                    if i == image_id && only.is_none_or(|o| o == id)
            );
            if hit {
                orphaned.push(p.spec.image);
            }
            !hit
        });
        if self.placements.list.len() != before {
            self.drop_orphans();
            self.all_dirty = true;
        }
    }

    /// The pictures that lost a placement because its parent went, since the last call.
    pub(crate) fn take_orphaned(&mut self) -> Vec<ImageHandle> {
        std::mem::take(&mut self.placements.orphaned)
    }
}
