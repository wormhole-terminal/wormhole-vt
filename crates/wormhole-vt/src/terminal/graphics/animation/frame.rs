//! `a=f`: data for an animation frame.
//!
//! Spec: kitty, "Transferring animation frame data". It is a transmission like a picture's
//! (`f`, `s`, `v`, `o`, `m`, `i`/`I`, `q`) with the picture named by `i` or `I`, plus: `x`,`y`
//! and `s`,`v` the rectangle the data covers (the whole picture when `s`,`v` are absent); `c`
//! the frame whose canvas the new one starts as, `Y` a colour for the canvas instead; `r` to edit
//! frame `r` rather than make a new one (its canvas is the frame itself); `X=1` to overwrite
//! pixels instead of blending; `z` the gap.
//!
//! The engine checks the keys and the sizes, counts the frame against the budget and tells the
//! host to do the pixels ([`FrameUpload`]).

use crate::terminal::graphics::command::Command;
use crate::terminal::graphics::reply::Error;
use crate::terminal::graphics::transmit::Flow;
use crate::terminal::graphics::upload::{Compression, ImageError, ImageFormat};
use crate::terminal::{Event, Terminal};
use crate::types::ImageHandle;

use super::host::{Blend, FrameBase, FrameRect, FrameTicket, FrameUpload};
use super::plan::{DEFAULT_GAP_MS, Gap, MAX_FRAMES};

impl Terminal {
    pub(in crate::terminal::graphics) fn kitty_frame(
        &mut self,
        command: &Command,
        payload: Vec<u8>,
    ) -> Result<Flow, Error> {
        let (slot, index, assigned) = self.animated_image(command)?;
        let image = self.graphics.tables[slot].get(index).ok_or(Error::NotFound)?;
        let (handle, width, height, frames, frame_cost) =
            (image.handle, image.width, image.height, image.animation.frame_count(), image.frame_cost());

        // The data covers the whole picture unless `s` and `v` say a rectangle.
        let mut sized = command.clone();
        if matches!(command.format, 24 | 32) {
            if sized.width == 0 {
                sized.width = width;
            }
            if sized.height == 0 {
                sized.height = height;
            }
        }
        let checked = self.check_transmission(&sized, &payload)?;
        let rect = FrameRect { x: command.x, y: command.y, width: checked.width, height: checked.height };
        if u64::from(rect.x) + u64::from(rect.width) > u64::from(width)
            || u64::from(rect.y) + u64::from(rect.height) > u64::from(height)
        {
            return Err(Error::Invalid("the frame's data is outside the picture"));
        }

        // The frame: one that exists (`r`), or a new one at the end.
        let editing = command.rows != 0;
        let (frame, base) = if editing {
            if command.rows as usize > frames {
                return Err(Error::NotFound);
            }
            (command.rows, FrameBase::Itself)
        } else {
            if frames >= MAX_FRAMES {
                return Err(Error::TooBig);
            }
            let base = match command.cols {
                0 => FrameBase::Color(command.offset_y),
                n if n as usize <= frames => FrameBase::Frame(n),
                _ => return Err(Error::NotFound),
            };
            (frames as u32 + 1, base)
        };

        // The events are the host's work to do: a queue that is full refuses the frame, as it
        // refuses a picture, rather than losing it.
        if self.events.len() >= self.limits.events {
            return Err(Error::TooBig);
        }
        let queued: usize = self
            .events
            .iter()
            .map(|event| match event {
                Event::ImageData(upload) => upload.data.len(),
                Event::ImageFrame(upload) => upload.data.len(),
                _ => 0,
            })
            .sum();
        if queued + payload.len() > self.limits.image_uploads {
            return Err(Error::TooBig);
        }
        if !editing {
            self.make_room_for_frame(slot, handle, frame_cost)?;
        }

        // Nothing can be refused from here on.
        let gap = match command.z {
            0 => (!editing).then_some(Gap::Millis(DEFAULT_GAP_MS)),
            z => Some(Gap::from_z(z)),
        };
        let table = &mut self.graphics.tables[slot];
        table.touch(index);
        if editing {
            if let (Some(gap), Some(image)) = (gap, table.get_mut(index)) {
                image.animation.set_gap(frame as usize - 1, gap);
            }
        } else {
            table.push_frame(index, gap.unwrap_or(Gap::Millis(DEFAULT_GAP_MS)));
        }
        let decoded_later = checked.format == ImageFormat::Png || command.zlib;
        let token = FrameTicket::token(handle, frame);
        self.push_event(Event::ImageFrame(FrameUpload {
            image: handle,
            frame,
            created: !editing,
            base,
            rect,
            blend: if command.offset_x == 1 { Blend::Replace } else { Blend::Over },
            format: checked.format,
            compression: if command.zlib { Compression::Zlib } else { Compression::None },
            data: payload,
            ticket: FrameTicket { token, image: handle, frame, created: !editing },
        }));
        self.animation_changed(handle);
        Ok(if decoded_later { Flow::Waiting { image: token, assigned } } else { Flow::Done { assigned } })
    }

    /// Room in the screen's budget for one more canvas of `cost` bytes of the picture `target`,
    /// by evicting pictures nothing is placed from. Never the target, and never one on show:
    /// a frame that does not fit is refused (`ENOSPC`), not paid for with someone's picture.
    fn make_room_for_frame(&mut self, slot: usize, target: ImageHandle, cost: u64) -> Result<(), Error> {
        while !self.graphics.tables[slot].has_room(cost, self.limits.image_bytes) {
            let grid = self.grid_of(slot);
            let victim = self.graphics.tables[slot].evictable(|image| {
                image.handle != target && !grid.is_placed(image.handle) && image.virtuals.is_empty()
            });
            let Some(victim) = victim else { return Err(Error::NoSpace) };
            self.free_image(slot, victim);
        }
        Ok(())
    }

    /// The host decoded the data of a [`FrameUpload`], or could not; see there. Answers the
    /// program if it was waiting, and takes a new frame that failed away again when it is the last.
    pub fn frame_decoded(&mut self, ticket: FrameTicket, result: Result<(), ImageError>) {
        let answer = result.map_err(|error| match error {
            ImageError::BadData => Error::BadData("the data could not be decoded as that format"),
            ImageError::TooBig => Error::TooBig,
            ImageError::Unsupported => Error::Unsupported("that picture format"),
        });
        if answer.is_err() && ticket.created {
            for slot in 0..2 {
                let table = &mut self.graphics.tables[slot];
                if let Some(index) = table.find(ticket.image) {
                    let last = table
                        .get(index)
                        .is_some_and(|image| image.animation.frame_count() == ticket.frame as usize);
                    if last {
                        table.pop_frame(index);
                        self.animation_changed(ticket.image);
                    }
                }
            }
        }
        self.replies.settle(ticket.token, answer);
    }
}
