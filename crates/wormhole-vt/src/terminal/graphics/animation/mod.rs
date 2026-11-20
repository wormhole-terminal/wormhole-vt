//! Kitty's animation frames: what the engine keeps of an animated picture, and how a host plays it.
//!
//! Spec: kitty, "Animation" (`a=f` transmit a frame, `a=a` control the animation, `a=c` compose
//! frames). The engine owns the *plan* of an animation and the host owns pixels and time.
//!
//! # Who does what
//!
//! | | engine | host |
//! |-|--------|------|
//! | frames' pixels | no (it has no decoder and no pixels) | keeps one canvas per frame, built from the events |
//! | frame count, gaps, state, loop count, current frame | yes: [`Animation`] | reads it |
//! | quota | every frame counts `width * height * 4` bytes against the screen's picture budget | frees what [`Event::ImageFree`](crate::terminal::Event::ImageFree) and [`Event::ImageFrameDeleted`](crate::terminal::Event::ImageFrameDeleted) name |
//! | time | none | its own clock |
//!
//! The host hears about pixels as [`Event::ImageFrame`](crate::terminal::Event::ImageFrame) (data
//! for a frame, to decode and compose onto a canvas; see [`FrameUpload`]) and
//! [`Event::ImageCompose`](crate::terminal::Event::ImageCompose) (a rectangle of one frame onto
//! another; see [`FrameCompose`]). Both must be applied **in the order they arrive**, per picture:
//! a composition may read a frame whose upload is still being decoded. About the plan it hears
//! as [`Event::ImageAnimation`](crate::terminal::Event::ImageAnimation) and reads
//! [`Terminal::animation`](crate::terminal::Terminal::animation).
//!
//! # The time API, and why
//!
//! The engine has no clock, so it cannot say "frame 3 is current now". Three shapes were
//! considered: a pure `frame_at(elapsed)` over the whole plan; the host telling the engine every
//! frame change; or a **stepper** the host drives. A pure `frame_at` cannot express kitty's
//! *loading* mode (state 2: wait at the last frame for frames that have not arrived yet; whatever
//! arrives later would be judged against time that has already passed) or a gap edited while the
//! animation runs, so it would need a second, stateful API next to it anyway. A host that reports every change makes the
//! engine a mirror of a clock it cannot see. The stepper is small and stays pure:
//!
//! - [`Animation::start`] is where to begin ([`Position`]: a frame and the pass through the
//!   frames); it is also the place to start over whenever [`Animation::generation`] changes (the
//!   program started, stopped or re-aimed the animation).
//! - [`Animation::tick`] says, for a frame being shown, how long to show it and what comes next,
//!   or that nothing does ([`Tick::Hold`]): the animation is stopped, finished its loops, or waits
//!   for more frames. Gapless frames come out as a wait of zero, so the host skips them without
//!   showing them. A cycle that takes no time at all is a `Hold`, never a busy loop. Each call is
//!   constant time (a pass's duration is kept as the gaps change), and a host that was away skips
//!   the whole passes it missed with [`Animation::skip_passes`] instead of stepping through them.
//! - The host calls [`Terminal::set_animation_current`](crate::terminal::Terminal::set_animation_current)
//!   when it moves on, so that a program that stops the animation leaves it on the frame the viewer
//!   saw (kitty keeps its own running position the same way), and a replica of the screen agrees.
//!   It changes nothing else and sends no event.
//!
//! # Choices
//!
//! - **Gaps** (`z`): a new frame's gap is `z` ms, 40 ms when `z` is 0 or absent, none (gapless)
//!   when `z` is negative; the root frame's gap starts at 0 and is set with `a=a,r=1,z=…`. On an
//!   existing frame `z=0` changes nothing. A gap of 0 is a frame shown for no time: a host should
//!   not draw faster than its own frames.
//! - **Loops** (`v`): 0 is ignored, 1 is forever (the default), `n > 1` is `n - 1` passes. Stopping
//!   (`s=1`) starts the count again, as every change of state does.
//! - **Quota**: every frame, edited or not, is a full canvas for the host, so a frame costs the
//!   picture's `width * height * 4` bytes and is refused with `ENOSPC` when the screen's budget
//!   ([`Limits::image_bytes`](crate::terminal::Limits::image_bytes)) cannot hold it even after
//!   the pictures nothing is placed from have been evicted. The target picture is never evicted
//!   for its own frames. A picture has at most [`MAX_FRAMES`] frames (`EFBIG` past it), and a
//!   frame's data waits in the event queue under the same caps as a picture's.
//! - **Not done**: kitty stores a frame as operations on other frames until it is composed and
//!   keeps them on disk with a quota of five times the base; the host here keeps whole canvases
//!   under the one quota.

mod compose;
mod control;
mod frame;
mod host;
mod plan;

pub use host::{Blend, FrameBase, FrameCompose, FrameRect, FrameTicket, FrameUpload};
pub use plan::{Animation, AnimationState, Gap, Loops, MAX_FRAMES, Position, Skipped, Tick};

use crate::terminal::Terminal;
use crate::types::ImageHandle;

impl Terminal {
    /// The animation plan of a picture, `None` when the handle names no picture. A picture
    /// that was never given a frame has a plan of one frame (the picture itself).
    ///
    /// How a host plays it is in the module docs of `terminal::graphics::animation`: start at
    /// [`Animation::start`], then follow [`Animation::tick`].
    pub fn animation(&self, image: ImageHandle) -> Option<&Animation> {
        self.graphics
            .tables
            .iter()
            .find_map(|table| table.find(image).and_then(|i| table.get(i)))
            .map(|image| &image.animation)
    }

    /// The host moved an animation on to frame `frame` (counted from 0; one that does not exist
    /// is ignored). Changes [`Animation::current`] and nothing else: no event, no new
    /// [`generation`](Animation::generation).
    pub fn set_animation_current(&mut self, image: ImageHandle, frame: usize) {
        for table in &mut self.graphics.tables {
            if let Some(image) = table.find(image).and_then(|i| table.get_mut(i)) {
                image.animation.set_current_quietly(frame);
            }
        }
    }

    /// Tell the host the plan of `image` changed.
    pub(super) fn animation_changed(&mut self, image: ImageHandle) {
        if !matches!(self.events.last(), Some(crate::terminal::Event::ImageAnimation(h)) if *h == image) {
            self.push_event(crate::terminal::Event::ImageAnimation(image));
        }
    }
}
