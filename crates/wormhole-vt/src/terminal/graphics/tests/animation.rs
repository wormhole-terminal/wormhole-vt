//! Animation: kitty, "Animation" (`a=f`, `a=a`, `a=c`), and the budget frames are counted in.

use super::{apc, apc_bare, frame_uploads, freed, png, rgb, rig, spots, uploads};
use crate::terminal::tests::Rig;
use crate::terminal::{
    AnimationState, Blend, Compression, Event, FrameBase, FrameRect, Gap, ImageError, Limits, Loops,
    MAX_FRAMES, Position, Tick,
};
use crate::types::ImageHandle;
use std::time::Duration;

/// Store a 4 by 2 picture (32 bytes as RGBA) with id 1 and return its handle.
fn picture(r: &mut Rig) -> ImageHandle {
    r.feed(&apc("a=t,f=24,s=4,v=2,i=1,q=2", &rgb(4, 2)));
    uploads(r)[0].image
}

fn plan(r: &Rig, image: ImageHandle) -> crate::terminal::Animation {
    r.term.animation(image).cloned().expect("the picture is known")
}

#[test]
fn a_picture_alone_is_an_animation_of_one_frame_that_is_stopped() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    let a = plan(&r, image);
    assert_eq!(
        (a.frame_count(), a.state(), a.loops(), a.current()),
        (1, AnimationState::Stopped, Loops::Infinite, 0)
    );
    assert_eq!(a.gap(0), Some(Gap::Millis(0)), "the root frame has no gap until it is set");
    assert_eq!(a.tick(a.start()), Tick::Hold);
    assert!(r.term.animation(ImageHandle::from_raw(99)).is_none());
}

#[test]
fn a_frame_with_data_for_the_whole_picture_is_a_new_frame_with_the_default_gap() {
    // kitty: "exactly the same as for transferring image data, with the addition of a=f,i=".
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=24,s=4,v=2,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    let up = frame_uploads(&mut r);
    assert_eq!(up.len(), 1);
    let u = &up[0];
    assert_eq!((u.image, u.frame, u.created), (image, 2, true));
    assert_eq!(u.base, FrameBase::Color(0), "black and transparent unless said");
    assert_eq!(u.rect, FrameRect { x: 0, y: 0, width: 4, height: 2 });
    assert_eq!((u.blend, u.compression, u.data.len()), (Blend::Over, Compression::None, 24));
    let a = plan(&r, image);
    assert_eq!((a.frame_count(), a.gap(1)), (2, Some(Gap::Millis(40))));
}

#[test]
fn a_frame_without_s_and_v_covers_the_whole_picture() {
    let mut r = rig(20, 5);
    picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    assert_eq!(frame_uploads(&mut r)[0].rect.width, 4);
}

#[test]
fn a_rectangle_a_base_frame_a_blend_a_colour_and_a_gap_are_passed_on() {
    let mut r = rig(20, 5);
    picture(&mut r);
    // x,y,s,v: the rectangle; c: base frame; X=1: overwrite; z: the gap.
    r.feed(&apc("a=f,f=24,s=2,v=1,x=1,y=1,c=1,X=1,z=120,i=1", &rgb(2, 1)));
    let u = frame_uploads(&mut r).remove(0);
    assert_eq!(u.rect, FrameRect { x: 1, y: 1, width: 2, height: 1 });
    assert_eq!((u.base, u.blend), (FrameBase::Frame(1), Blend::Replace));
    // Y: a colour for the canvas (opaque red); z=-1: gapless.
    r.feed(&apc("a=f,f=32,s=1,v=1,Y=4278190335,z=-1,i=1", &[1, 2, 3, 4]));
    let u = frame_uploads(&mut r).remove(0);
    assert_eq!(u.base, FrameBase::Color(0xff00_00ff));
    let a = r.term.animation(u.image).unwrap();
    assert_eq!((a.gap(1), a.gap(2)), (Some(Gap::Millis(120)), Some(Gap::Gapless)));
}

#[test]
fn editing_a_frame_composes_onto_the_frame_itself_and_a_gap_of_zero_changes_nothing() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,z=90", &rgb(4, 2)));
    frame_uploads(&mut r);
    r.feed(&apc("a=f,f=24,s=1,v=1,r=2,i=1", &rgb(1, 1)));
    let u = frame_uploads(&mut r).remove(0);
    assert_eq!((u.frame, u.created, u.base), (2, false, FrameBase::Itself));
    let a = plan(&r, image);
    assert_eq!((a.frame_count(), a.gap(1)), (2, Some(Gap::Millis(90))), "no new frame, the gap kept");
    r.feed(&apc("a=f,f=24,s=1,v=1,r=2,z=10,i=1", &rgb(1, 1)));
    assert_eq!(plan(&r, image).gap(1), Some(Gap::Millis(10)));
    // The root frame can be edited too.
    r.feed(&apc("a=f,f=24,s=1,v=1,r=1,i=1", &rgb(1, 1)));
    assert_eq!(frame_uploads(&mut r).last().map(|u| u.frame), Some(1));
}

#[test]
fn a_frame_in_chunks_is_put_together_and_answered_once() {
    let mut r = rig(20, 5);
    picture(&mut r);
    let pixels = rgb(4, 2);
    let (a, b) = pixels.split_at(10);
    r.feed(&apc("a=f,f=24,s=4,v=2,i=1,m=1", a));
    assert!(frame_uploads(&mut r).is_empty());
    r.feed(&apc("m=0", b));
    let up = frame_uploads(&mut r);
    assert_eq!((up.len(), up[0].data.as_slice()), (1, pixels.as_slice()));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
}

#[test]
fn a_frame_is_refused_with_the_codes_of_the_protocol() {
    let mut r = rig(20, 5);
    picture(&mut r);
    let code = |r: &mut Rig, keys: &str, data: &[u8]| {
        r.feed(&apc(keys, data));
        r.replies()
    };
    assert!(code(&mut r, "a=f,f=24,i=9", &rgb(4, 2)).starts_with("\x1b_Gi=9;ENOENT"), "no such picture");
    assert!(
        code(&mut r, "a=f,f=24,c=2,i=1", &rgb(4, 2)).starts_with("\x1b_Gi=1;EINVAL"),
        "no such base frame: EINVAL, as kitty has it"
    );
    assert!(code(&mut r, "a=f,f=24,s=4,v=2,x=1,i=1", &rgb(4, 2)).starts_with("\x1b_Gi=1;EINVAL"), "outside");
    assert!(code(&mut r, "a=f,f=24,s=5,v=2,i=1", &rgb(5, 2)).starts_with("\x1b_Gi=1;EINVAL"), "wider");
    assert!(code(&mut r, "a=f,f=24,s=4,v=2,i=1", &rgb(4, 1)).starts_with("\x1b_Gi=1;ENODATA"), "short data");
    assert!(
        code(&mut r, "a=f,f=24,s=4,v=2", &rgb(4, 2)).is_empty(),
        "no id, no answer; and no picture named"
    );
    assert!(code(&mut r, "a=f,f=24,i=1,t=f", b"/etc/passwd").starts_with("\x1b_Gi=1;EBADF"), "no files");
    assert!(code(&mut r, "a=f,f=24,i=1,I=2", &rgb(4, 2)).starts_with("\x1b_Gi=1,I=2;EINVAL"));
    assert!(frame_uploads(&mut r).is_empty(), "nothing reached the host");
    assert_eq!(r.term.animation(uploads_handle(&r)).map(|a| a.frame_count()), Some(1));
    // q: 1 keeps the OK, 2 keeps the errors.
    assert_eq!(code(&mut r, "a=f,f=24,i=1,q=1", &rgb(4, 2)), "");
    assert_eq!(code(&mut r, "a=f,f=24,i=9,q=2", &rgb(4, 2)), "");
    assert!(code(&mut r, "a=f,f=24,i=9,q=1", &rgb(4, 2)).contains("ENOENT"));
}

fn uploads_handle(r: &Rig) -> ImageHandle {
    let _ = r;
    ImageHandle::from_raw(1)
}

#[test]
fn a_picture_named_by_number_is_told_its_id() {
    let mut r = rig(20, 5);
    r.feed(&apc("a=t,f=24,s=4,v=2,I=13,q=2", &rgb(4, 2)));
    uploads(&mut r);
    r.feed(&apc("a=f,f=24,I=13", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1,I=13;OK\x1b\\");
}

#[test]
fn a_png_frame_is_answered_when_the_host_has_decoded_it_and_a_failed_one_is_still_a_frame() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=100,i=1", &png(4, 2)));
    r.feed("\x1b[5n");
    assert_eq!(r.replies(), "", "waits for the host, and so do the replies behind it");
    let u = frame_uploads(&mut r).remove(0);
    assert_eq!(plan(&r, image).frame_count(), 2);
    r.term.frame_decoded(u.ticket, Err(ImageError::BadData));
    assert!(r.replies().starts_with("\x1b_Gi=1;ENODATA"));
    assert_eq!(
        plan(&r, image).frame_count(),
        2,
        "the frame that could not be decoded is still a frame: its number was decided by the command"
    );

    r.feed(&apc("a=f,f=100,i=1", &png(4, 2)));
    let u = frame_uploads(&mut r).remove(0);
    assert_eq!(u.frame, 3, "the next number, whatever became of the one before");
    r.term.frame_decoded(u.ticket, Ok(()));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    assert_eq!(plan(&r, image).frame_count(), 3);

    // Two uploads waiting at once are answered each its own: the second one first, and the
    // first one's answer still waits for the first.
    r.feed(&apc("a=f,f=100,i=1,r=2", &png(4, 2)));
    r.feed(&apc("a=f,f=100,i=1,r=2", &png(4, 2)));
    let both = frame_uploads(&mut r);
    assert_eq!(both.len(), 2);
    assert_ne!(both[0].ticket, both[1].ticket, "two edits of one frame are two tickets");
    r.term.frame_decoded(both[1].ticket, Err(ImageError::BadData));
    assert_eq!(r.replies(), "", "the first is still waiting, and the answers go in the order asked");
    r.term.frame_decoded(both[0].ticket, Ok(()));
    let replies = r.replies();
    assert!(replies.starts_with("\x1b_Gi=1;OK") && replies.contains("ENODATA"), "{replies:?}");
}

#[test]
fn a_frame_number_past_the_last_makes_a_new_frame_and_zero_does_too() {
    // kitty (`handle_animation_frame_load_command`): not an error, a new frame.
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,r=9,q=2", &rgb(4, 2)));
    r.feed(&apc("a=f,f=24,i=1,r=3,q=2", &rgb(4, 2)));
    r.feed(&apc("a=f,f=24,i=1,r=0,q=2", &rgb(4, 2)));
    let up = frame_uploads(&mut r);
    assert_eq!(
        up.iter().map(|u| (u.frame, u.created)).collect::<Vec<_>>(),
        [(2, true), (3, true), (4, true)]
    );
    assert_eq!(plan(&r, image).frame_count(), 4);
}

#[test]
fn a_png_that_is_not_the_size_of_a_rectangle_in_the_picture_is_refused() {
    let mut r = rig(20, 5);
    picture(&mut r);
    r.feed(&apc("a=f,f=100,i=1", &png(40, 2)));
    assert!(r.replies().contains("EINVAL"));
}

// ---- the quota ----

fn limited(bytes: u64) -> Rig {
    let mut r = rig(20, 5);
    let limits = Limits { image_bytes: bytes, ..Limits::default() };
    r.term.set_limits(limits);
    r
}

#[test]
fn frames_count_against_the_budget_and_one_that_does_not_fit_is_refused_with_enospc() {
    // 32 bytes a canvas, room for three.
    let mut r = limited(96);
    let image = picture(&mut r);
    for _ in 0..2 {
        r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
        assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    }
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert!(r.replies().starts_with("\x1b_Gi=1;ENOSPC"));
    assert_eq!(plan(&r, image).frame_count(), 3);
    assert_eq!(frame_uploads(&mut r).len(), 2, "the refused frame never reached the host");
    // Editing a frame costs nothing.
    r.feed(&apc("a=f,f=24,r=3,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    // The budget is exact: a new picture of 32 bytes cannot join, a frame's worth is all used.
    r.feed(&apc("a=t,f=24,s=4,v=2,i=2", &rgb(4, 2)));
    assert!(r.replies().contains("i=2;OK"), "a new picture evicts the older ones, as it always did");
}

#[test]
fn a_frame_makes_room_by_evicting_pictures_nothing_is_placed_from_but_not_the_ones_on_show() {
    let mut r = limited(96);
    let image = picture(&mut r); // 32
    r.feed(&apc("a=t,f=24,s=4,v=2,i=2,q=2", &rgb(4, 2))); // 64
    r.feed(&apc("a=T,f=24,s=4,v=2,i=3,q=2", &rgb(4, 2))); // 96, and on show
    uploads(&mut r);
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\", "picture 2 left to make room");
    let gone: Vec<_> = freed(&mut r);
    assert_eq!(gone.len(), 1);
    assert_eq!(plan(&r, image).frame_count(), 2);
    assert_eq!(spots(&r).len(), 1, "the picture on show stayed");
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert!(r.replies().starts_with("\x1b_Gi=1;ENOSPC"), "nothing else may go: the other is on show");
    assert!(freed(&mut r).is_empty());
}

#[test]
fn a_picture_with_frames_costs_all_of_them_when_it_is_evicted_or_replaced() {
    let mut r = limited(128);
    picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    r.replies();
    // Replacing the picture under its id gives the three canvases back (the invariants check the sum).
    r.feed(&apc("a=t,f=24,s=4,v=2,i=1,q=2", &rgb(4, 2)));
    for _ in 0..3 {
        r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
        assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    }
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert!(r.replies().contains("ENOSPC"));
}

#[test]
fn lowering_the_budget_evicts_pictures_with_frames_by_their_whole_cost() {
    let mut r = rig(20, 5);
    picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    freed(&mut r);
    let limits = Limits { image_bytes: 95, ..Limits::default() };
    r.term.set_limits(limits);
    assert_eq!(freed(&mut r).len(), 1);
}

#[test]
fn a_picture_has_at_most_max_frames() {
    let mut r = rig(20, 5);
    r.feed(&apc("a=t,f=24,s=1,v=1,i=1,q=2", &rgb(1, 1)));
    for n in 1..MAX_FRAMES {
        r.feed(&apc("a=f,f=24,s=1,v=1,i=1,q=2", &rgb(1, 1)));
        if n % 1000 == 0 {
            r.events();
        }
    }
    r.events();
    r.feed(&apc("a=f,f=24,s=1,v=1,i=1", &rgb(1, 1)));
    assert!(r.replies().starts_with("\x1b_Gi=1;EFBIG"));
    assert_eq!(r.term.animation(ImageHandle::from_raw(1)).map(|a| a.frame_count()), Some(MAX_FRAMES));
}

#[test]
fn frames_wait_in_the_queue_under_the_same_caps_as_pictures() {
    let mut r = rig(20, 5);
    picture(&mut r);
    let limits = Limits { image_uploads: 40, ..Limits::default() };
    r.term.set_limits(limits);
    r.events();
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert!(r.replies().starts_with("\x1b_Gi=1;EFBIG"), "24 and 24 bytes are over 40");
    r.events();
    r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
}

// ---- control ----

#[test]
fn the_animation_is_controlled_with_state_loops_current_frame_and_gaps() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    for z in [10, 20, -1] {
        r.feed(&apc(&format!("a=f,f=24,i=1,z={z},q=2"), &rgb(4, 2)));
    }
    let g0 = plan(&r, image).generation();
    r.feed(&apc_bare("a=a,i=1,s=3,v=3,c=2,r=1,z=30"));
    assert_eq!(r.replies(), "", "kitty does not answer a successful a=a");
    let a = plan(&r, image);
    assert_eq!((a.state(), a.loops(), a.current()), (AnimationState::Running, Loops::Passes(2), 1));
    assert_eq!(a.gaps(), [Gap::Millis(30), Gap::Millis(10), Gap::Millis(20), Gap::Gapless]);
    assert_ne!(a.generation(), g0);
    // The host plays it: from the current frame, frame 2, round the gapless one and back.
    assert_eq!(a.start(), Position { frame: 1, pass: 0 });
    assert_eq!(
        a.tick(Position { frame: 2, pass: 0 }),
        Tick::After { wait: Duration::from_millis(20), next: Position { frame: 3, pass: 0 } }
    );
    assert_eq!(
        a.tick(Position { frame: 3, pass: 0 }),
        Tick::After { wait: Duration::ZERO, next: Position { frame: 0, pass: 1 } }
    );
    assert_eq!(a.tick(Position { frame: 3, pass: 1 }), Tick::Hold, "v=3 is two passes");
    // v=0 is ignored, v=1 is for ever, s=1 stops, s=2 loads.
    r.feed(&apc_bare("a=a,i=1,v=0,s=2"));
    let a = plan(&r, image);
    assert_eq!((a.state(), a.loops()), (AnimationState::Loading, Loops::Passes(2)));
    r.feed(&apc_bare("a=a,i=1,v=1,s=1"));
    let a = plan(&r, image);
    assert_eq!((a.state(), a.loops()), (AnimationState::Stopped, Loops::Infinite));
}

#[test]
fn z_zero_changes_no_gap_and_a_negative_z_makes_one_gapless() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,z=50,q=2", &rgb(4, 2)));
    r.feed(&apc_bare("a=a,i=1,r=2,z=0"));
    assert_eq!(plan(&r, image).gap(1), Some(Gap::Millis(50)));
    r.feed(&apc_bare("a=a,i=1,r=2,z=-5"));
    assert_eq!(plan(&r, image).gap(1), Some(Gap::Gapless));
}

#[test]
fn a_control_skips_what_names_nothing_and_does_the_rest() {
    // kitty (`handle_animation_control_command`): a frame that is not there for `r` or `c` and a
    // state that is not 1, 2 or 3 are skipped without an error; the other keys still count.
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    let before = plan(&r, image);
    for keys in ["a=a,i=1,c=2", "a=a,i=1,r=2,z=5", "a=a,i=1,s=7", "a=a,i=1,c=4294967295,r=4294967295,z=1"] {
        r.feed(&apc_bare(keys));
        assert_eq!(r.replies(), "", "{keys}");
        assert_eq!(plan(&r, image).gaps(), before.gaps(), "{keys}");
        assert_eq!(plan(&r, image).state(), before.state(), "{keys}");
    }
    r.feed(&apc_bare("a=a,i=1,c=2,s=3,v=4"));
    let a = plan(&r, image);
    assert_eq!((a.state(), a.loops(), a.current()), (AnimationState::Running, Loops::Passes(3), 0));
    // A picture that is not there is the one error.
    r.feed(&apc_bare("a=a,i=9,s=3"));
    assert!(r.replies().contains("ENOENT"));
    r.feed(&apc_bare("a=a,i=1,I=1,s=3"));
    assert!(r.replies().contains("EINVAL"));
    // Without an id or number there is nothing to answer, and no picture to change.
    r.feed(&apc_bare("a=a,s=3"));
    assert_eq!(r.replies(), "");
    // A numbered picture is changed too, and not answered.
    r.feed(&apc("a=t,f=24,s=4,v=2,I=5,q=2", &rgb(4, 2)));
    r.feed(&apc_bare("a=a,I=5,s=3"));
    assert_eq!(r.replies(), "");
}

#[test]
fn the_host_reporting_its_frame_is_remembered_without_a_new_generation_or_an_event() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,q=2", &rgb(4, 2)));
    r.events();
    let generation = plan(&r, image).generation();
    r.term.set_animation_current(image, 1);
    assert_eq!((plan(&r, image).current(), plan(&r, image).generation()), (1, generation));
    assert!(r.events().is_empty());
    r.term.set_animation_current(image, 9);
    assert_eq!(plan(&r, image).current(), 1, "there is no such frame");
}

#[test]
fn a_changed_plan_is_announced_once_in_a_row() {
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    r.events();
    r.feed(&apc_bare("a=a,i=1,s=3"));
    r.feed(&apc_bare("a=a,i=1,s=3"));
    let events = r.events();
    assert_eq!(events, [Event::ImageAnimation(image)]);
}

// ---- composition ----

#[test]
fn a_rectangle_of_one_frame_is_composed_onto_another() {
    let mut r = rig(20, 5);
    picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,q=2", &rgb(4, 2)));
    r.events();
    r.feed(&apc_bare("a=c,i=1,r=1,c=2,w=2,h=1,X=1,Y=0,x=2,y=1"));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    let composed: Vec<_> = r
        .events()
        .into_iter()
        .filter_map(|e| if let Event::ImageCompose(c) = e { Some(c) } else { None })
        .collect();
    assert_eq!(composed.len(), 1);
    let c = composed[0];
    assert_eq!((c.source, c.destination, c.blend), (1, 2, Blend::Over));
    assert_eq!(c.from, FrameRect { x: 1, y: 0, width: 2, height: 1 });
    assert_eq!(c.to, FrameRect { x: 2, y: 1, width: 2, height: 1 });
    // The whole picture when w and h are absent; C=1 replaces.
    r.feed(&apc_bare("a=c,i=1,r=1,c=2,C=1"));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    let Some(Event::ImageCompose(c)) = r.events().into_iter().find(|e| matches!(e, Event::ImageCompose(_)))
    else {
        panic!("no composition")
    };
    assert_eq!((c.blend, c.from.width, c.from.height), (Blend::Replace, 4, 2));
}

#[test]
fn a_composition_names_what_is_wrong_with_it() {
    let mut r = rig(20, 5);
    picture(&mut r);
    r.feed(&apc("a=f,f=24,i=1,q=2", &rgb(4, 2)));
    r.events();
    for (keys, code) in [
        ("a=c,i=9,r=1,c=2", "ENOENT"),
        ("a=c,i=1,r=3,c=2", "ENOENT"),
        ("a=c,i=1,r=1,c=3", "ENOENT"),
        ("a=c,i=1,c=2", "ENOENT"),
        ("a=c,i=1,r=1,c=2,w=3,x=2", "EINVAL"),
        ("a=c,i=1,r=1,c=2,h=2,Y=1", "EINVAL"),
        ("a=c,i=1,r=1,c=2,w=4294967295", "EINVAL"),
        ("a=c,i=1,r=1,c=1,w=2,h=1,X=1,x=2", "EINVAL"),
    ] {
        r.feed(&apc_bare(keys));
        let reply = r.replies();
        assert!(reply.contains(code), "{keys}: {reply:?}");
    }
    assert!(r.events().is_empty());
    // The same frame is fine when the rectangles do not overlap, and the edges touch.
    r.feed(&apc_bare("a=c,i=1,r=1,c=1,w=2,h=1,X=2,x=0"));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
}

// ---- deleting frames ----

#[test]
fn deleting_a_frame_takes_one_frame_and_gives_back_its_budget() {
    // kitty (`handle_delete_frame_command`): the frame is `r`, 0 is the first, past the last is the last.
    let mut r = limited(96);
    let image = picture(&mut r);
    for z in [10, 20] {
        r.feed(&apc(&format!("a=f,f=24,i=1,z={z},q=2"), &rgb(4, 2)));
    }
    r.feed(&apc_bare("a=a,i=1,s=3,c=3,r=1,z=5,q=2"));
    r.events();
    r.feed(&apc_bare("a=d,d=f,i=1,r=2"));
    assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    let a = plan(&r, image);
    assert_eq!(
        (a.gaps(), a.current(), a.state()),
        (&[Gap::Millis(5), Gap::Millis(20)][..], 1, AnimationState::Running)
    );
    let events = r.events();
    assert!(events.contains(&Event::ImageFrameDeleted { image, frame: 2 }));
    assert!(!events.iter().any(|e| matches!(e, Event::ImageFree(_))));
    // The root frame can go, and the picture is what was the second frame.
    r.feed(&apc_bare("a=d,d=f,i=1"));
    assert_eq!(plan(&r, image).gaps(), &[Gap::Millis(20)][..]);
    assert!(r.events().contains(&Event::ImageFrameDeleted { image, frame: 1 }));
    r.replies(); // the answers to the two deletes
    // Room for two frames again.
    for _ in 0..2 {
        r.feed(&apc("a=f,f=24,i=1", &rgb(4, 2)));
        assert_eq!(r.replies(), "\x1b_Gi=1;OK\x1b\\");
    }
    r.feed(&apc_bare("a=d,d=f,i=1,r=99"));
    assert_eq!(plan(&r, image).frame_count(), 2, "past the last is the last");
    assert!(r.events().contains(&Event::ImageFrameDeleted { image, frame: 3 }));
}

#[test]
fn deleting_a_frame_needs_a_picture_and_the_last_frame_stays_unless_the_capital_is_used() {
    let mut r = rig(20, 5);
    let a = picture(&mut r);
    r.feed(&apc("a=T,f=24,s=4,v=2,i=2,q=2", &rgb(4, 2)));
    r.events();
    r.feed(&apc_bare("a=d,d=f"));
    assert!(
        r.replies().is_empty() || r.replies().contains("EINVAL"),
        "a delete without i or I names no picture"
    );
    assert!(r.events().is_empty());
    r.feed(&apc_bare("a=d,d=f,i=1"));
    assert!(r.term.animation(a).is_some(), "the only frame stays");
    assert!(freed(&mut r).is_empty());
    // F on a picture with one frame deletes the whole picture, as kitty does, placed or not.
    r.feed(&apc_bare("a=d,d=F,i=2"));
    assert_eq!(freed(&mut r).len(), 1);
    assert!(spots(&r).is_empty());
    r.feed(&apc_bare("a=d,d=F,i=1"));
    assert_eq!(freed(&mut r), [a]);
    assert!(r.term.animation(a).is_none());
    // A capital on a picture with frames deletes the frame and frees nothing else.
    r.feed(&apc("a=t,f=24,s=4,v=2,i=3,q=2", &rgb(4, 2)));
    r.feed(&apc("a=f,f=24,i=3,q=2", &rgb(4, 2)));
    r.events();
    r.feed(&apc_bare("a=d,d=F,i=3"));
    assert!(freed(&mut r).is_empty());
}

#[test]
fn a_plan_of_thousands_of_gapless_frames_and_one_that_takes_time_costs_no_scan_per_step() {
    // 4095 gapless frames and a root frame of 1 ms: a pass is 1 ms. `tick` reads the cached length of a
    // pass, so stepping through all frames is 4096 constant-time steps, and a host that was away for
    // ten seconds skips ten thousand passes at once instead of stepping through them.
    let mut r = rig(20, 5);
    let image = picture(&mut r);
    for _ in 1..MAX_FRAMES {
        r.feed(&apc("a=f,f=24,s=1,v=1,z=-1,i=1,q=2", &rgb(1, 1)));
        if r.term.animation(image).is_some_and(|a| a.frame_count() % 1000 == 0) {
            r.events();
        }
    }
    r.feed(&apc_bare("a=a,i=1,r=1,z=1,s=3,q=2"));
    let a = plan(&r, image);
    assert_eq!(a.frame_count(), MAX_FRAMES);
    assert_eq!(a.pass_duration(), Duration::from_millis(1));
    let mut at = Position { frame: 0, pass: 0 };
    let mut steps = 0;
    while let Tick::After { next, .. } = a.tick(at) {
        steps += 1;
        at = next;
        if at.pass == 1 {
            break;
        }
    }
    assert_eq!((steps, at), (MAX_FRAMES, Position { frame: 0, pass: 1 }));
    let skipped = a.skip_passes(Position { frame: 0, pass: 0 }, Duration::from_secs(10));
    assert_eq!((skipped.position.pass, skipped.lag), (10_000, Duration::ZERO));
}

// ---- the whole thing stays sound ----

#[test]
fn hostile_animation_commands_never_panic_and_leave_the_tables_sound() {
    let mut r = rig(20, 5);
    picture(&mut r);
    for keys in [
        "a=f,i=1,f=24,s=4294967295,v=4294967295",
        "a=f,i=1,f=32,x=4294967295,y=4294967295,s=1,v=1",
        "a=a,i=1,c=4294967295,r=4294967295,s=4294967295,v=4294967295,z=-2147483648",
        "a=c,i=1,r=4294967295,c=4294967295",
        "a=c,i=1,r=1,c=1,w=4294967295,h=4294967295,X=4294967295,Y=4294967295,x=4294967295,y=4294967295",
        "a=f,i=1,f=24,Y=4294967295,X=4294967295,c=4294967295",
        "a=f,i=1,f=24,r=4294967295",
    ] {
        r.feed(&apc(keys, &rgb(1, 1)));
        r.feed(&apc_bare(keys));
        r.replies();
        r.events();
    }
    r.term.check_invariants().expect("sound");
}
