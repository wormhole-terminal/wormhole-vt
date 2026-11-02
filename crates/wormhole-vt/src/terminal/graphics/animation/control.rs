//! `a=a`: controlling an animation.
//!
//! Spec: kitty, "Controlling animations". `c` makes a frame current, `s` starts or stops (1 stop,
//! 2 run but wait for frames, 3 run), `v` sets the loops (0 is ignored, 1 forever, `n` is `n - 1`
//! passes), and `r` with `z` sets the gap of frame `r` (`z=0` is ignored, negative is gapless).
//! A missing picture or frame is `ENOENT`; a state that is not 1, 2 or 3 is `EINVAL`.

use crate::terminal::Terminal;
use crate::terminal::graphics::command::Command;
use crate::terminal::graphics::reply::Error;
use crate::terminal::graphics::transmit::Flow;

use super::plan::{AnimationState, Gap, Loops};

impl Terminal {
    /// The picture an animation command names (by `i` or `I`): its screen, its place in the
    /// table, and the id a numbered picture is told it has.
    pub(in crate::terminal::graphics) fn animated_image(
        &self,
        command: &Command,
    ) -> Result<(usize, usize, Option<u32>), Error> {
        let slot = self.screen_slot();
        let table = &self.graphics.tables[slot];
        let index = match (command.id, command.number) {
            (0, 0) => return Err(Error::Invalid("no image id or number")),
            (0, number) => table.by_number(number),
            (id, _) => table.by_id(id),
        }
        .ok_or(Error::NotFound)?;
        let assigned = (command.id == 0).then(|| table.get(index).map(|image| image.id)).flatten();
        Ok((slot, index, assigned))
    }

    pub(in crate::terminal::graphics) fn kitty_animate(&mut self, command: &Command) -> Result<Flow, Error> {
        let (slot, index, assigned) = self.animated_image(command)?;
        let image = self.graphics.tables[slot].get(index).ok_or(Error::NotFound)?;
        let (handle, frames) = (image.handle, image.animation.frame_count());
        // Everything is checked before anything changes: a refused command does nothing.
        let state = match command.width {
            0 => None,
            1 => Some(AnimationState::Stopped),
            2 => Some(AnimationState::Loading),
            3 => Some(AnimationState::Running),
            _ => return Err(Error::Invalid("a state the protocol does not have")),
        };
        let current = match command.cols {
            0 => None,
            n if n as usize <= frames => Some(n as usize - 1),
            _ => return Err(Error::NotFound),
        };
        let gap = match (command.rows, command.z) {
            (_, 0) | (0, _) => None,
            (n, _) if n as usize > frames => return Err(Error::NotFound),
            (n, z) => Some((n as usize - 1, Gap::from_z(z))),
        };
        if command.rows as usize > frames {
            return Err(Error::NotFound);
        }
        let loops = match command.height {
            0 => None,
            1 => Some(Loops::Infinite),
            n => Some(Loops::Passes(n - 1)),
        };
        // The announcement is kept past the queue's cap, so the command is refused there.
        if self.events.len() >= self.limits.events {
            return Err(Error::TooBig);
        }
        let table = &mut self.graphics.tables[slot];
        table.touch(index);
        let Some(image) = table.get_mut(index) else { return Err(Error::NotFound) };
        let animation = &mut image.animation;
        if let Some(loops) = loops {
            animation.set_loops(loops);
        }
        if let Some(frame) = current {
            animation.set_current(frame);
        }
        if let Some((frame, gap)) = gap {
            animation.set_gap(frame, gap);
        }
        if let Some(state) = state {
            animation.set_state(state);
        }
        self.animation_changed(handle);
        Ok(Flow::Done { assigned })
    }
}
