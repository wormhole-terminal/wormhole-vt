//! `a=c`: compose a rectangle of one frame onto another.
//!
//! Spec: kitty, "Composing animation frames". The source frame is `r`, the destination `c`; the
//! rectangle is `w` by `h` (the picture's size when absent) at `X`,`Y` in the source and `x`,`y` in
//! the destination; `C=1` replaces pixels instead of blending. A missing picture or frame is
//! `ENOENT`; a rectangle out of the picture, or overlapping itself in one frame, `EINVAL`.
//! The engine holds no pixels, so it only checks and passes the work to the host.

use crate::terminal::Event;
use crate::terminal::Terminal;
use crate::terminal::graphics::command::Command;
use crate::terminal::graphics::reply::Error;
use crate::terminal::graphics::transmit::Flow;

use super::host::{Blend, FrameCompose, FrameRect};

impl Terminal {
    pub(in crate::terminal::graphics) fn kitty_compose(&mut self, command: &Command) -> Result<Flow, Error> {
        let (slot, index, assigned) = self.animated_image(command)?;
        let table = &self.graphics.tables[slot];
        let image = table.get(index).ok_or(Error::NotFound)?;
        let (handle, width, height, frames) =
            (image.handle, image.width, image.height, image.animation.frame_count());
        let frame = |n: u32| -> Result<u32, Error> {
            if n == 0 || n as usize > frames { Err(Error::NotFound) } else { Ok(n) }
        };
        let (source, destination) = (frame(command.rows)?, frame(command.cols)?);
        let (w, h) =
            (if command.w == 0 { width } else { command.w }, if command.h == 0 { height } else { command.h });
        // `X`, `Y` are the source corner here and `x`, `y` the destination's, whatever they mean
        // for a placement.
        let (sx, sy, dx, dy) = (command.offset_x, command.offset_y, command.x, command.y);
        let fits = |x: u32, y: u32| {
            u64::from(x) + u64::from(w) <= u64::from(width)
                && u64::from(y) + u64::from(h) <= u64::from(height)
        };
        if !fits(sx, sy) || !fits(dx, dy) {
            return Err(Error::Invalid("the rectangle is outside the picture"));
        }
        let overlap = |a: u32, b: u32, len: u32| {
            u64::from(a) < u64::from(b) + u64::from(len) && u64::from(b) < u64::from(a) + u64::from(len)
        };
        if source == destination && overlap(sx, dx, w) && overlap(sy, dy, h) {
            return Err(Error::Invalid("the rectangles overlap in one frame"));
        }
        if self.events.len() >= self.limits.events {
            return Err(Error::TooBig);
        }
        self.graphics.tables[slot].touch(index);
        self.push_event(Event::ImageCompose(FrameCompose {
            image: handle,
            source,
            destination,
            from: FrameRect { x: sx, y: sy, width: w, height: h },
            to: FrameRect { x: dx, y: dy, width: w, height: h },
            blend: if command.cursor_stays { Blend::Replace } else { Blend::Over },
        }));
        Ok(Flow::Done { assigned })
    }
}
