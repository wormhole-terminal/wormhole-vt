//! What the host is told to do with a frame's pixels: data to compose onto a canvas, and a
//! rectangle of one frame to compose onto another.
//!
//! Every frame is a canvas the size of the picture. The host keeps one per frame (frame 1, the
//! root, is the picture as it was uploaded) and builds it from these.

use crate::terminal::graphics::upload::{Compression, ImageFormat};
use crate::types::ImageHandle;

/// How pixels are put onto a canvas (kitty's `X` of `a=f`, `C` of `a=c`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Blend {
    /// Source over destination, straight (not premultiplied) alpha: the default.
    #[default]
    Over,
    /// The destination pixels are replaced, alpha included.
    Replace,
}

/// What the canvas holds before a frame's data goes onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameBase {
    /// The whole canvas is one colour first: `0xRRGGBBAA` (kitty's `Y`; 0 is transparent black,
    /// the default).
    Color(u32),
    /// A copy of this frame (kitty's `c`; counted from 1, the root frame being 1).
    Frame(u32),
    /// The frame being edited (kitty's `r`): the data goes onto what it already is.
    Itself,
}

/// A rectangle in a frame, in the picture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRect {
    /// Pixels from the left edge.
    pub x: u32,
    /// Pixels from the top edge.
    pub y: u32,
    /// Pixels wide.
    pub width: u32,
    /// Pixels high.
    pub height: u32,
}

/// Names one [`FrameUpload`] when the host answers it with
/// [`Terminal::frame_decoded`](crate::terminal::Terminal::frame_decoded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTicket {
    pub(crate) token: ImageHandle,
    pub(crate) image: ImageHandle,
    pub(crate) frame: u32,
    pub(crate) created: bool,
}

impl FrameTicket {
    /// What the reply queue waits on for a frame: not a picture's handle, so it never meets one
    /// (the top bit is set), and made from the picture and the frame instead of a counter, so that
    /// the engine's state does not depend on how many frames came before (a replica started
    /// halfway has not seen them). Two uploads of one frame waiting at once share it, which is
    /// right: the host answers in the order the engine sent them, and the queue settles the first.
    pub(crate) fn token(image: ImageHandle, frame: u32) -> ImageHandle {
        ImageHandle(1 << 63 | (image.get() & 0xFFFF_FFFF_FFFF) << 16 | u64::from(frame & 0xFFFF))
    }
}

/// Data for one frame of a picture (kitty's `a=f`).
///
/// The host builds the frame (`frame`, counted from 1): the canvas starts as `base`, the
/// decoded `data` (exactly `rect.width` by `rect.height` pixels) goes onto `rect` by `blend`, and
/// the result is that frame's canvas. A new frame (`created`) is the next number after the last;
/// otherwise it replaces the canvas of an existing one.
///
/// The host must call [`Terminal::frame_decoded`](crate::terminal::Terminal::frame_decoded) once
/// for each, in the order they were sent, when it has the pixels or has given up (the program's `OK` waits for a PNG or
/// compressed data; the replies after it wait behind it). A frame that could not be decoded
/// is the base canvas alone: the engine takes the frame away again if it is the last, and
/// otherwise keeps it, so the host must keep it too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameUpload {
    /// The picture the frame belongs to.
    pub image: ImageHandle,
    /// The frame, counted from 1.
    pub frame: u32,
    /// A new frame, one past the last, and not an edit of an existing one.
    pub created: bool,
    /// What the canvas is before the data goes on.
    pub base: FrameBase,
    /// Where the data goes.
    pub rect: FrameRect,
    /// How it goes on.
    pub blend: Blend,
    /// How `data` is laid out.
    pub format: ImageFormat,
    /// What was done to `data` on top.
    pub compression: Compression,
    /// The bytes (base64 already undone).
    pub data: Vec<u8>,
    /// What to answer with.
    pub ticket: FrameTicket,
}

/// A rectangle of one frame composed onto another (kitty's `a=c`). The frames are different, or
/// their rectangles do not overlap; both rectangles lie inside the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCompose {
    /// The picture.
    pub image: ImageHandle,
    /// The frame the pixels come from, counted from 1.
    pub source: u32,
    /// The frame they go onto, counted from 1.
    pub destination: u32,
    /// The rectangle in the source frame.
    pub from: FrameRect,
    /// Where it goes in the destination: the same size, so only the corner matters.
    pub to: FrameRect,
    /// How it goes on.
    pub blend: Blend,
}
