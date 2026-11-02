//! Relative placements: kitty, "Relative placements" (`P`, `Q`, `H`, `V`).

use super::{apc, apc_bare, freed, placed, rgb, rig, spots, uploads};
use crate::terminal::tests::Rig;
use crate::types::Scroll;

/// A one-cell picture (8 by 16 pixels in the rigs) with id `id`, placed at the cursor with
/// placement id `p`, plus the keys `extra`.
fn put(r: &mut Rig, id: u32, p: u32, extra: &str) -> String {
    r.feed(&apc(&format!("a=T,f=24,s=8,v=16,i={id},p={p},{extra}"), &rgb(8, 16)));
    r.replies()
}

#[test]
fn a_child_sits_at_its_offset_from_the_parents_top_left_cell() {
    // kitty: "H and V … positive values move right and down … the origin is the top left cell of the parent".
    let mut r = rig(20, 10);
    r.feed("\x1b[3;4H");
    assert_eq!(put(&mut r, 1, 1, "q=2"), "");
    assert_eq!(r.cursor(), (2, 4));
    assert_eq!(put(&mut r, 2, 1, "P=1,Q=1,H=3,V=2"), "\x1b_Gi=2,p=1;OK\x1b\\");
    assert_eq!(spots(&r), [(1, 2, 3, 1, 1), (2, 4, 6, 1, 1)]);
    assert_eq!(r.cursor(), (2, 4), "the cursor does not move after a relative placement");
}

#[test]
fn offsets_may_be_negative_and_the_cursor_stays_whatever_c_says() {
    let mut r = rig(20, 10);
    r.feed("\x1b[5;5H");
    put(&mut r, 1, 1, "q=2,C=1");
    let before = r.cursor();
    put(&mut r, 2, 1, "P=1,Q=1,H=-2,V=-3,C=0,q=2");
    assert_eq!(spots(&r), [(1, 4, 4, 1, 1), (2, 1, 2, 1, 1)]);
    assert_eq!(r.cursor(), before);
}

#[test]
fn a_child_at_the_bottom_row_does_not_scroll_the_screen() {
    let mut r = rig(20, 3);
    r.feed("\x1b[3;1Hx\x1b[3;1H");
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,V=1,q=2");
    assert_eq!(r.row(2), "x");
    assert_eq!(r.term.history_len(), 0);
}

#[test]
fn a_child_is_a_placement_in_its_own_right_a_picture_may_have_several() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,q=2");
    r.feed(&apc_bare("a=p,i=2,p=2,P=1,Q=1,H=2,z=5"));
    assert_eq!(r.replies(), "\x1b_Gi=2,p=2;OK\x1b\\");
    let cols: Vec<_> = placed(&r).iter().map(|p| (p.image_id, p.placement_id, p.column, p.z)).collect();
    assert_eq!(cols, [(1, 1, 0, 0), (2, 1, 1, 0), (2, 2, 2, 5)], "{cols:?}");
}

#[test]
fn a_parent_placed_again_moves_its_children_with_it() {
    // kitty: "when the parent placement moves, the relative placement moves along with it".
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,V=1,q=2");
    r.feed("\x1b[6;8H");
    r.feed(&apc_bare("a=p,i=1,p=1,C=1,q=2"));
    // (The parent, placed again, is the newest, so it is drawn last.)
    assert_eq!(spots(&r), [(2, 6, 8, 1, 1), (1, 5, 7, 1, 1)]);
}

#[test]
fn a_chain_follows_every_link() {
    let mut r = rig(30, 12);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,V=1,q=2");
    put(&mut r, 3, 1, "P=2,Q=1,H=2,V=2,q=2");
    assert_eq!(spots(&r), [(1, 0, 0, 1, 1), (2, 1, 1, 1, 1), (3, 3, 3, 1, 1)]);
}

#[test]
fn a_chain_may_have_eight_parents_and_no_more() {
    // kitty's limit is 8 (PARENT_DEPTH_LIMIT), and the protocol asks for at least 8.
    let mut r = rig(40, 12);
    put(&mut r, 1, 1, "q=2,C=1");
    for n in 2..=9 {
        assert_eq!(put(&mut r, n, 1, &format!("P={},Q=1,H=1,q=1", n - 1)), "", "{n} has {} parents", n - 1);
    }
    let reply = put(&mut r, 10, 1, "P=9,Q=1,H=1");
    assert!(reply.starts_with("\x1b_Gi=10,p=1;ETOODEEP"), "{reply:?}");
    assert_eq!(placed(&r).len(), 9);
    assert_eq!(placed(&r).last().map(|p| p.column), Some(8));
}

#[test]
fn a_placement_that_would_be_its_own_ancestor_is_refused() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,q=2");
    put(&mut r, 3, 1, "P=2,Q=1,q=2");
    // 1 → 3 → 2 → 1: placing 1 again, relative to 3, closes the loop.
    r.feed(&apc_bare("a=p,i=1,p=1,P=3,Q=1"));
    assert!(r.replies().starts_with("\x1b_Gi=1,p=1;ECYCLE"));
    // A placement relative to itself, existing or not.
    r.feed(&apc_bare("a=p,i=3,p=1,P=3,Q=1"));
    assert!(r.replies().contains("ECYCLE"));
    r.feed(&apc_bare("a=p,i=1,p=9,P=1,Q=9"));
    assert!(r.replies().contains("ECYCLE"));
    assert_eq!(placed(&r).len(), 3, "nothing changed");
}

#[test]
fn a_parent_that_does_not_exist_is_refused() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    for (keys, what) in [
        ("P=9,Q=1", "no such picture"),
        ("P=1,Q=2", "no such placement"),
        ("P=1", "the picture has none without an id"),
    ] {
        let reply = put(&mut r, 2, 1, keys);
        assert!(reply.starts_with("\x1b_Gi=2,p=1;ENOPARENT"), "{what}: {reply:?}");
    }
    assert_eq!(placed(&r).len(), 1);
    // The placement without an id is addressed by Q=0.
    r.feed(&apc("a=T,f=24,s=8,v=16,i=5,q=2", &rgb(8, 16)));
    assert_eq!(put(&mut r, 6, 1, "P=5,q=1"), "");
    assert_eq!(placed(&r).len(), 3);
}

#[test]
fn deleting_the_parent_deletes_its_children_and_a_picture_left_without_placements() {
    // kitty: "If its parent is deleted, it is deleted as well. If the image … has no more
    // placements, the image is deleted as well."
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,q=2");
    put(&mut r, 3, 1, "P=2,Q=1,H=1,q=2");
    let handles = uploads(&mut r);
    assert_eq!(handles.len(), 3);
    r.feed(&apc_bare("a=d,d=i,i=1,p=1"));
    assert!(placed(&r).is_empty());
    assert_eq!(
        freed(&mut r),
        [handles[1].image, handles[2].image],
        "the children's pictures go, the parent's stays"
    );
    r.term.check_invariants().expect("sound");
}

#[test]
fn a_child_whose_picture_has_other_placements_keeps_its_picture() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,q=2");
    r.feed("\x1b[5;5H");
    r.feed(&apc_bare("a=p,i=2,p=7,C=1,q=2"));
    uploads(&mut r);
    r.feed(&apc_bare("a=d,d=i,i=1,p=1"));
    assert_eq!(spots(&r), [(2, 4, 4, 1, 1)]);
    assert!(freed(&mut r).is_empty());
}

#[test]
fn the_capital_delete_frees_the_pictures_of_the_group() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,q=2");
    uploads(&mut r);
    r.feed(&apc_bare("a=d,d=I,i=1"));
    assert_eq!(freed(&mut r).len(), 2);
}

#[test]
fn a_child_goes_when_its_cells_are_erased_and_the_parent_stays() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,V=3,q=2");
    r.feed("\x1b[4;1H\x1b[2K");
    assert_eq!(spots(&r), [(1, 0, 0, 1, 1)]);
    r.term.check_invariants().expect("sound");
}

#[test]
fn a_child_goes_when_its_parent_is_erased() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,V=3,q=2");
    r.feed("\x1b[1;1H\x1b[2K");
    assert!(spots(&r).is_empty());
}

#[test]
fn children_scroll_with_their_parent_into_the_history() {
    let mut r = rig(20, 4);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=2,V=1,q=2");
    r.feed("\x1b[4;1H\n\n");
    assert_eq!(spots(&r), [], "the parent is two rows up, in the history; the view shows the screen");
    r.term.scroll(Scroll::Top);
    assert_eq!(spots(&r), [(1, -2, 0, 1, 1), (2, -1, 2, 1, 1)]);
}

#[test]
fn a_child_left_of_the_screen_is_not_shown() {
    let mut r = rig(20, 10);
    r.feed("\x1b[1;3H");
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=-3,q=2");
    assert_eq!(spots(&r), [(1, 0, 2, 1, 1)]);
    // Placed again further right, it is back.
    r.feed("\x1b[1;5H");
    r.feed(&apc_bare("a=p,i=1,p=1,C=1,q=2"));
    assert_eq!(spots(&r).len(), 2);
}

#[test]
fn a_child_is_cut_at_the_right_edge_like_any_placement() {
    let mut r = rig(10, 5);
    put(&mut r, 1, 1, "q=2,C=1");
    r.feed(&apc("a=p,i=1,p=2,P=1,Q=1,H=9,c=3,r=1", &[]));
    assert_eq!(placed(&r).iter().map(|p| (p.column, p.cols)).collect::<Vec<_>>(), [(0, 1), (9, 1)]);
}

#[test]
fn a_virtual_placement_cannot_be_relative() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    r.feed(&apc("a=T,f=24,s=8,v=16,i=2,U=1,P=1,Q=1,c=1,r=1", &rgb(8, 16)));
    assert!(r.replies().starts_with("\x1b_Gi=2;EINVAL"));
}

/// Print the placeholder for picture `id` (placement `p`), tile (0, 0): the id rides in the
/// foreground colour, the placement in the underline colour.
fn placeholder(id: u32, p: u32) -> String {
    format!(
        "\x1b[38;2;{};{};{}m\x1b[58;2;{};{};{}m\u{10EEEE}\u{305}\u{305}\x1b[0m",
        id >> 16 & 255,
        id >> 8 & 255,
        id & 255,
        p >> 16 & 255,
        p >> 8 & 255,
        p & 255,
    )
}

#[test]
fn a_child_of_a_virtual_placement_follows_its_placeholders() {
    // kitty: the parent's position is the minimum of the placeholders' positions.
    let mut r = rig(20, 10);
    r.feed(&apc("a=t,f=24,s=8,v=16,i=7,q=2", &rgb(8, 16)));
    r.feed(&apc_bare("a=p,i=7,U=1,p=3,c=2,r=1,q=2"));
    put(&mut r, 8, 1, "P=7,Q=3,H=2,V=1,q=2,C=1");
    assert!(spots(&r).iter().all(|s| s.0 != 8), "no placeholders in view yet: not shown");
    r.feed("\x1b[4;6H");
    r.feed(&placeholder(7, 3));
    r.feed("\x1b[3;9H");
    r.feed(&placeholder(7, 3));
    r.feed("\x1b[6;1H");
    r.feed(&placeholder(7, 4)); // another placement's: not ours
    let child: Vec<_> = placed(&r).iter().filter(|p| p.image_id == 8).map(|p| (p.line.0, p.column)).collect();
    // The least line (2) and the least column (5) of the two: (line 2, column 5) plus (1, 2).
    assert_eq!(child, [(3, 7)]);
    r.feed(&apc_bare("a=d,d=i,i=7"));
    assert!(placed(&r).iter().all(|p| p.image_id != 8), "the parent went, the child with it");
}

#[test]
fn a_parent_of_either_kind_is_preferred_real_before_virtual() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    r.feed(&apc_bare("a=p,i=1,U=1,p=1,c=1,r=1,q=2"));
    r.feed("\x1b[5;5H");
    r.feed(&placeholder(1, 1));
    put(&mut r, 2, 1, "P=1,Q=1,q=2,C=1");
    assert_eq!(placed(&r).iter().find(|p| p.image_id == 2).map(|p| (p.line.0, p.column)), Some((0, 0)));
}

#[test]
fn hostile_offsets_do_not_overflow() {
    let mut r = rig(20, 10);
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=-2147483648,V=2147483647,q=2");
    put(&mut r, 3, 1, "P=1,Q=1,H=2147483647,V=-2147483648,q=2");
    assert_eq!(placed(&r).len(), 1);
    r.term.check_invariants().expect("sound");
}

#[test]
fn re_placing_the_root_of_one_chain_under_the_tail_of_another_cannot_make_it_too_deep() {
    // y11..y18 is a chain with 7 parents above its tail; x1 has x2 hanging on it. Putting x1 under y18
    // would give x2 eight parents above x1 and x1 itself... one too many in all: refused, and the
    // placements stay as they were. One link higher it fits.
    let mut r = rig(40, 12);
    put(&mut r, 11, 1, "q=2,C=1");
    for n in 12..=18 {
        assert_eq!(put(&mut r, n, 1, &format!("P={},Q=1,q=1", n - 1)), "");
    }
    put(&mut r, 1, 1, "q=2,C=1");
    put(&mut r, 2, 1, "P=1,Q=1,H=1,q=2");
    let before = spots(&r);
    r.feed(&apc_bare("a=p,i=1,p=1,P=18,Q=1,C=1"));
    assert!(r.replies().starts_with("\x1b_Gi=1,p=1;ETOODEEP"), "{:?}", r.replies());
    assert_eq!(spots(&r), before);
    r.feed(&apc_bare("a=p,i=1,p=1,P=17,Q=1,C=1,q=1"));
    assert_eq!(r.replies(), "");
    // x2 now has eight parents above it (y11 to y17, then x1), which is the most allowed.
    assert_eq!(placed(&r).iter().filter(|p| p.image_id == 2).count(), 1);
    r.term.check_invariants().expect("sound");
}

#[test]
fn a_child_of_a_virtual_placement_is_deleted_by_position_and_by_a_delete_of_the_screens_placements() {
    let mut r = rig(20, 10);
    r.feed(&apc("a=t,f=24,s=8,v=16,i=7,q=2", &rgb(8, 16)));
    r.feed(&apc_bare("a=p,i=7,U=1,p=3,c=1,r=1,q=2"));
    put(&mut r, 8, 1, "P=7,Q=3,H=1,V=1,q=2,C=1");
    r.feed("\x1b[4;6H");
    r.feed(&placeholder(7, 3));
    let drawn = |r: &Rig| placed(r).iter().filter(|p| p.image_id == 8).count();
    assert_eq!(drawn(&r), 1, "the child is where the placeholder says (line 4, column 7)");
    // Deleting what covers its cell (line 5, column 7, counted from 1) deletes it: `d=p`.
    r.feed(&apc_bare("a=d,d=p,x=7,y=5"));
    assert_eq!(drawn(&r), 0);
    // The placeholder still stands: the child is gone, not hidden.
    put(&mut r, 8, 1, "P=7,Q=3,H=1,V=1,q=2,C=1");
    assert_eq!(drawn(&r), 1);
    r.feed(&apc_bare("a=d,d=a"));
    assert_eq!(drawn(&r), 0, "a=d,d=a deletes the placements on the screen, this one too");
    put(&mut r, 8, 1, "P=7,Q=3,H=1,V=1,q=2,C=1");
    assert_eq!(drawn(&r), 1);
    r.feed(&apc_bare("a=d,d=p,x=1,y=1"));
    assert_eq!(drawn(&r), 1, "a cell it does not cover leaves it");
}

#[test]
fn erasing_the_cells_of_a_child_of_a_virtual_placement_deletes_it() {
    let mut r = rig(20, 10);
    r.feed(&apc("a=t,f=24,s=8,v=16,i=7,q=2", &rgb(8, 16)));
    r.feed(&apc_bare("a=p,i=7,U=1,p=3,c=1,r=1,q=2"));
    put(&mut r, 8, 1, "P=7,Q=3,V=2,q=2,C=1");
    r.feed("\x1b[2;3H");
    r.feed(&placeholder(7, 3));
    let drawn = |r: &Rig| placed(r).iter().filter(|p| p.image_id == 8).count();
    assert_eq!(drawn(&r), 1);
    // The child covers line 4 (the placeholder is on line 2, two lines up): erase that line.
    r.feed("\x1b[4;1H\x1b[2K");
    r.feed("\x1b[2;3H");
    r.feed(&placeholder(7, 3));
    assert_eq!(drawn(&r), 0, "it went with its cells, and does not come back with the placeholder");
    r.term.check_invariants().expect("sound");
}
