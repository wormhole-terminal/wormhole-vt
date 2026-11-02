//! `a=d`: deleting placements and pictures.
//!
//! Spec: kitty, "Deleting images". The letter in `d` says which placements go; a capital also
//! frees the pictures' data when nothing is placed from them any more. Without `d` the default is
//! `a`: the placements on the screen. A delete naming a picture that does not exist does
//! nothing, and is answered `OK`.
//!
//! "On the screen" means the live screen's rows, not the history; cell coordinates `x` and `y`
//! count from 1 at the screen's top left corner.

use crate::terminal::{Event, Terminal};
use crate::types::ImageHandle;

use super::command::{Command, Selector};
use super::reply::Error;

impl Terminal {
    pub(super) fn kitty_delete(&mut self, command: &Command) -> Result<(), Error> {
        let slot = self.screen_slot();
        let (cursor_line, cursor_col) = (self.cursor.y as i64, self.cursor.x);
        let cell = (i64::from(command.y) - 1, (command.x as usize).wrapping_sub(1));
        let table = &self.graphics.tables[slot];

        // The pictures a selector names, for the ones that name pictures.
        let named: Option<Vec<ImageHandle>> = match command.selector {
            Selector::ById => {
                if command.id == 0 {
                    return Err(Error::Invalid("no image id"));
                }
                Some(
                    table
                        .by_id(command.id)
                        .and_then(|i| table.get(i))
                        .map(|image| image.handle)
                        .into_iter()
                        .collect(),
                )
            }
            Selector::ByNumber => {
                if command.number == 0 {
                    return Err(Error::Invalid("no image number"));
                }
                Some(
                    table
                        .by_number(command.number)
                        .and_then(|i| table.get(i))
                        .map(|image| image.handle)
                        .into_iter()
                        .collect(),
                )
            }
            // The pictures whose frames go: the one named, else those placed on the screen.
            Selector::Frames => {
                let by_name = match (command.id, command.number) {
                    (0, 0) => None,
                    (0, number) => Some(table.by_number(number)),
                    (id, _) => Some(table.by_id(id)),
                };
                let grid = self.grid_of(slot);
                Some(match by_name {
                    Some(index) => {
                        index.and_then(|i| table.get(i)).map(|image| image.handle).into_iter().collect()
                    }
                    None => table
                        .iter()
                        .filter(|image| grid.is_placed(image.handle))
                        .map(|image| image.handle)
                        .collect(),
                })
            }
            Selector::IdRange => Some(
                table
                    .iter()
                    .filter(|image| image.id != 0 && image.id >= command.x && image.id <= command.y)
                    .map(|image| image.handle)
                    .collect(),
            ),
            _ => None,
        };
        // A placement id narrows a delete by id or number to that placement.
        let only_placement = matches!(command.selector, Selector::ById | Selector::ByNumber)
            .then_some(command.placement)
            .filter(|&p| p != 0);

        if command.selector == Selector::Frames {
            self.delete_frames(slot, named.unwrap_or_default(), command.free);
            return Ok(());
        }
        let mut removed: Vec<ImageHandle> = Vec::new();
        let selector = command.selector;
        let z = command.z;
        let screen = self.grid_of_mut(slot);
        match selector {
            // Taken care of above.
            Selector::Frames => {}
            Selector::ById | Selector::ByNumber | Selector::IdRange => {
                let handles = named.as_deref().unwrap_or(&[]);
                screen.delete_placements(
                    |hit| {
                        handles.contains(&hit.image) && only_placement.is_none_or(|p| hit.placement_id == p)
                    },
                    &mut removed,
                );
            }
            Selector::Visible => {
                screen.delete_placements(|hit| hit.on_screen(), &mut removed);
            }
            Selector::AtCursor => {
                screen.delete_placements(|hit| hit.covers(cursor_line, cursor_col), &mut removed);
            }
            Selector::AtCell => {
                screen.delete_placements(|hit| hit.covers(cell.0, cell.1), &mut removed);
            }
            Selector::AtCellWithZ => {
                screen.delete_placements(|hit| hit.z == z && hit.covers(cell.0, cell.1), &mut removed);
            }
            Selector::InColumn => {
                screen.delete_placements(|hit| hit.on_screen() && hit.spans_column(cell.1), &mut removed);
            }
            Selector::InRow => {
                screen.delete_placements(|hit| hit.spans_line(cell.0), &mut removed);
            }
            Selector::WithZ => {
                screen.delete_placements(|hit| hit.z == z, &mut removed);
            }
        }

        // The virtual placements of the pictures a selector names go with them, and what is
        // placed relative to them.
        if let Some(handles) = &named {
            for &handle in handles {
                let table = &mut self.graphics.tables[slot];
                let Some(image) = table.find(handle).and_then(|i| table.get_mut(i)) else { continue };
                image.virtuals.retain(|v| only_placement.is_some_and(|p| v.placement_id != p));
                let image_id = image.id;
                if image_id != 0 {
                    self.grid_of_mut(slot).drop_virtual_children(image_id, only_placement);
                }
            }
        }

        // "If the image that a relative placement is a placement of has no more placements, the
        // image is deleted as well": the children that went with their parent free their picture.
        let orphaned = self.grid_of_mut(slot).take_orphaned();
        self.free_unreferenced(slot, orphaned);

        if command.free {
            let candidates = removed.into_iter().chain(named.into_iter().flatten()).collect();
            self.free_unreferenced(slot, candidates);
        }
        Ok(())
    }

    /// `d=f`: the frames but the root go from each picture named, and the animation stops. A
    /// capital frees a picture nothing is placed from too, like the other capitals.
    fn delete_frames(&mut self, slot: usize, handles: Vec<ImageHandle>, free: bool) {
        let table = &mut self.graphics.tables[slot];
        let mut cleared = Vec::new();
        for &handle in &handles {
            if let Some(index) = table.find(handle)
                && table.get(index).is_some_and(|image| image.animation.frame_count() > 1)
            {
                table.clear_frames(index);
                cleared.push(handle);
            }
        }
        for handle in cleared {
            self.push_event(Event::ImageFramesCleared(handle));
            self.animation_changed(handle);
        }
        if free {
            self.free_unreferenced(slot, handles);
        }
    }

    /// Free the pictures among `candidates` that nothing is placed from and no virtual placement
    /// names. In the order the pictures came (handles are given out in order): the events the
    /// host hears must not depend on a hash set's random order, or the same bytes fed in other
    /// pieces would tell it the frees in another order.
    fn free_unreferenced(&mut self, slot: usize, mut candidates: Vec<ImageHandle>) {
        candidates.sort_unstable();
        candidates.dedup();
        for handle in candidates {
            let screen = self.grid_of(slot);
            let table = &self.graphics.tables[slot];
            let Some(index) = table.find(handle) else { continue };
            let referenced =
                screen.is_placed(handle) || table.get(index).is_some_and(|i| !i.virtuals.is_empty());
            if !referenced {
                self.free_image(slot, index);
            }
        }
    }
}
