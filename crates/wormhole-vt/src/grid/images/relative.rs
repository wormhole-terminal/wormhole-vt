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
//!   ([`Grid::origins`], one pass for all placements, memoized), from the parent's, up a chain of
//!   at most [`MAX_DEPTH`] parents, so whatever moves a parent (a scroll, a reflow, a new
//!   placement) moves its children, and nothing else moves a child. The scroll, reflow and
//!   cut-off code skips children for that reason.
//! - A parent may be a virtual placement of Unicode placeholders. Its position is then the top
//!   left of the placeholder cells on the screen (or, to draw, in view). The grid is below the
//!   layer that knows what a placeholder is, so the terminal gives it a function that reads one
//!   ([`PlaceholderKey`]); without it, or without a placeholder in range, the child is not shown.
//! - A child cut off at the left edge (a negative column) is not shown: a placement has no
//!   column to the left of the screen. One below or above the view is not shown either.
//! - Deleting a parent deletes its children, and theirs ([`Grid::drop_orphans`], a worklist over
//!   the whole table, not a search per child). The pictures that lost their last placement that
//!   way are handed back ([`Grid::take_orphaned`]).
//! - A chain is at most [`MAX_DEPTH`] parents deep **counting what hangs below the placement
//!   being made**: re-placing the root of one chain under the tail of another cannot make the
//!   first longer than the limit ([`Grid::check_relative`]).
//! - Cost: while no placement is relative (`Placements::maybe_relative`) nothing here runs and
//!   nothing is allocated, so scrolling and erasing cost what they did before relative placements
//!   existed. With some, a pass costs one hash map of the table and a few vectors of its size,
//!   once per scroll, erase, delete or frame, never one search per placement.

use std::collections::HashMap;

use crate::types::{Cell, CellExtra, ImageHandle, Line, RowExtras};

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

/// Reads a placeholder cell: the picture id and the placement id it names. Given by the
/// terminal, which knows what a placeholder is.
pub(crate) type PlaceholderKey = fn(&Cell, CellExtra<'_>) -> Option<(u32, u32)>;

/// Where the placeholders of each virtual parent start: (picture id, placement id), then a line
/// counted from the live screen's top row (negative in the history) and a column.
type VirtualOrigins = Vec<((u32, u32), (i64, i64))>;

/// What a placement is named by to its children.
type Key = (ImageHandle, u32);

/// Why a relative placement cannot be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// It would be its own ancestor.
    Cycle,
    /// More than [`MAX_DEPTH`] parents above it or above something that hangs on it.
    TooDeep,
}

impl Placements {
    /// The newest placement of each name.
    fn index(&self) -> HashMap<Key, usize> {
        let mut index = HashMap::with_capacity(self.list.len());
        for (i, p) in self.list.iter().enumerate() {
            index.insert((p.spec.image, p.spec.id), i);
        }
        index
    }

    /// The newest placement of `image` with this id.
    fn find(&self, image: ImageHandle, id: u32) -> Option<usize> {
        self.list.iter().rposition(|p| p.spec.image == image && p.spec.id == id)
    }

    /// How many levels hang below the placement `key` (0: nothing does), at most
    /// `MAX_DEPTH + 1`: a level at a time over the table.
    fn levels_below(&self, key: Key) -> usize {
        let mut frontier = vec![key];
        let mut levels = 0;
        while levels <= MAX_DEPTH {
            let next: Vec<Key> = self
                .list
                .iter()
                .filter(|p| matches!(p.rel, Some(Relative { parent: Parent::Placement { image, id }, .. }) if frontier.contains(&(image, id))))
                .map(|p| (p.spec.image, p.spec.id))
                .collect();
            if next.is_empty() {
                break;
            }
            levels += 1;
            frontier = next;
        }
        levels
    }
}

impl Grid {
    /// Let the grid read placeholders, so that a placement relative to a virtual one can be
    /// found wherever its position is asked for (drawing, deleting, erasing).
    pub(crate) fn set_placeholder_key(&mut self, key: PlaceholderKey) {
        self.placements.placeholder_key = Some(key);
    }

    /// Whether the picture has a placement with this id (0: one without an id).
    pub(crate) fn has_placement(&self, image: ImageHandle, id: u32) -> bool {
        self.placements.find(image, id).is_some()
    }

    /// Whether a placement of `image` with id `id` may hang on `parent`: it must not be its own
    /// ancestor, and neither it nor anything that hangs on it (children placed under its name
    /// earlier) may end up more than [`MAX_DEPTH`] parents below a root. The parent has to exist
    /// already (a virtual one is never relative itself, and the terminal knows it).
    pub(crate) fn check_relative(&self, image: ImageHandle, id: u32, parent: Parent) -> Result<(), Refused> {
        let index = if self.placements.maybe_relative { self.placements.index() } else { HashMap::new() };
        let mut at = parent;
        let mut above = 0;
        while let Parent::Placement { image: at_image, id: at_id } = at {
            if (at_image, at_id) == (image, id) {
                return Err(Refused::Cycle);
            }
            above += 1;
            if above > MAX_DEPTH {
                return Err(Refused::TooDeep);
            }
            let Some(next) = index.get(&(at_image, at_id)).and_then(|&i| self.placements.list[i].rel) else {
                break;
            };
            at = next.parent;
        }
        // What already hangs on this name moves down with it: the deepest of it counts too.
        let below =
            if self.placements.maybe_relative { self.placements.levels_below((image, id)) } else { 0 };
        if above + below > MAX_DEPTH {
            return Err(Refused::TooDeep);
        }
        Ok(())
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
        self.placements.maybe_relative = true;
        self.drop_orphans();
        self.all_dirty = true;
    }

    /// Where the placeholders of each virtual parent start, among the `rows` lines from `first`
    /// (0 is the live screen's top row). Empty without a reader or a virtual parent.
    fn virtual_origins(&self, first: i64, rows: usize) -> VirtualOrigins {
        let Some(key_of) = self.placements.placeholder_key else { return Vec::new() };
        let mut wanted: Vec<(u32, u32)> = Vec::new();
        for p in &self.placements.list {
            if let Some(Relative { parent: Parent::Virtual { image_id, id }, .. }) = p.rel
                && !wanted.contains(&(image_id, id))
            {
                wanted.push((image_id, id));
            }
        }
        if wanted.is_empty() {
            return Vec::new();
        }
        let mut found: VirtualOrigins = Vec::new();
        let (mut cells, mut extras): (Vec<Cell>, RowExtras) = (Vec::new(), RowExtras::default());
        for line in first..first.saturating_add(rows as i64) {
            let line = Line(line.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
            self.copy_row(line, &mut cells, Some(&mut extras));
            for (col, cell) in cells.iter().enumerate() {
                let Some(key) = key_of(cell, extras.cell(col)) else { continue };
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
        found
    }

    /// Where every placement starts, in one pass over the table, each worked out once: the serial
    /// number of its top row and its column, or `None` when it is not shown (its parent is gone,
    /// a virtual parent has no placeholders in range, or the chain is longer than allowed).
    /// `virtual_origins` say where the placeholders are. Only for a table with relatives.
    fn origins(&self, virtual_origins: &VirtualOrigins) -> Vec<Option<(i64, i64)>> {
        let list = &self.placements.list;
        let top_serial = self.top_serial() as i64;
        let index = self.placements.index();
        let mut out: Vec<Option<(i64, i64)>> = vec![None; list.len()];
        let mut done = vec![false; list.len()];
        let mut chain: Vec<usize> = Vec::with_capacity(MAX_DEPTH + 1);
        for start in 0..list.len() {
            if done[start] {
                continue;
            }
            chain.clear();
            let mut at = start;
            // Up the parents until something known: a root, a virtual parent, a node done already.
            let mut base = loop {
                if done[at] {
                    break out[at];
                }
                let p = &list[at];
                let Some(rel) = p.rel else {
                    out[at] = Some((p.top, p.col as i64));
                    done[at] = true;
                    break out[at];
                };
                chain.push(at);
                if chain.len() > MAX_DEPTH {
                    break None;
                }
                match rel.parent {
                    Parent::Placement { image, id } => match index.get(&(image, id)) {
                        Some(&parent) => at = parent,
                        None => break None,
                    },
                    Parent::Virtual { image_id, id } => {
                        break virtual_origins
                            .iter()
                            .find(|(k, _)| *k == (image_id, id))
                            .map(|(_, (line, col))| (top_serial.saturating_add(*line), *col));
                    }
                }
            };
            // And back down, adding each one's offset.
            for &node in chain.iter().rev() {
                if let Some(rel) = list[node].rel {
                    base = base.map(|(line, col)| {
                        (line.saturating_add(i64::from(rel.dy)), col.saturating_add(i64::from(rel.dx)))
                    });
                }
                out[node] = base;
                done[node] = true;
            }
        }
        out
    }

    /// Where every placement starts as [`origins`](Self::origins) says, for a table that has
    /// relatives, and `None` (use the stored position) for one that has none: the common case
    /// allocates nothing. `first` and `rows` are the lines placeholders are looked for in.
    pub(super) fn resolved(&self, first: i64, rows: usize) -> Option<Vec<Option<(i64, i64)>>> {
        self.placements.maybe_relative.then(|| self.origins(&self.virtual_origins(first, rows)))
    }

    /// Delete the children whose parent is gone, and theirs, with a worklist: a placement leaves
    /// once, and the children of a name leave when its last placement does. Returns whether any
    /// went. The pictures of those placements are noted for [`take_orphaned`](Self::take_orphaned).
    /// Nothing runs while no placement is relative.
    pub(super) fn drop_orphans(&mut self) -> bool {
        if !self.placements.maybe_relative {
            return false;
        }
        let list = &self.placements.list;
        // How many placements carry each name, and who hangs on each name: a chain of indexes
        // through `next_child` from `first_child`, so that this makes a fixed number of vectors
        // whatever the number of parents is.
        let mut alive_by_name: HashMap<Key, usize> = HashMap::with_capacity(list.len());
        let mut first_child: HashMap<Key, usize> = HashMap::with_capacity(list.len());
        let mut next_child = vec![usize::MAX; list.len()];
        for (i, p) in list.iter().enumerate() {
            *alive_by_name.entry((p.spec.image, p.spec.id)).or_insert(0) += 1;
            if let Some(Relative { parent: Parent::Placement { image, id }, .. }) = p.rel {
                next_child[i] = first_child.insert((image, id), i).unwrap_or(usize::MAX);
            }
        }
        let mut gone = vec![false; list.len()];
        let mut work: Vec<usize> = Vec::new();
        let push_children = |name: &Key, work: &mut Vec<usize>| {
            let mut at = first_child.get(name).copied().unwrap_or(usize::MAX);
            while at != usize::MAX {
                work.push(at);
                at = next_child[at];
            }
        };
        for name in first_child.keys().filter(|name| !alive_by_name.contains_key(*name)) {
            push_children(name, &mut work);
        }
        let mut any = false;
        while let Some(i) = work.pop() {
            if gone[i] {
                continue;
            }
            gone[i] = true;
            any = true;
            let name = (list[i].spec.image, list[i].spec.id);
            self.placements.orphaned.push(list[i].spec.image);
            if let Some(count) = alive_by_name.get_mut(&name) {
                *count -= 1;
                if *count == 0 {
                    push_children(&name, &mut work);
                }
            }
        }
        if any {
            let mut flags = gone.iter();
            self.placements.list.retain(|_| !flags.next().copied().unwrap_or(false));
            self.all_dirty = true;
        }
        // The flag is only a "maybe": this pass looked at everything, so make it exact.
        self.placements.maybe_relative = self.placements.list.iter().any(|p| p.rel.is_some());
        any
    }

    /// The placements relative to a virtual placement of the picture with `image_id` are gone,
    /// because it was (`only` is its placement id) or all of that picture's were.
    pub(crate) fn drop_virtual_children(&mut self, image_id: u32, only: Option<u32>) {
        if !self.placements.maybe_relative {
            return;
        }
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
