//! A prompt a line editor draws again keeps its block (OSC 133) when the shell sends `A` only once.

use super::Rig;
use crate::grid::{BlockPart, RowMark};
use crate::types::Line;

const A: &str = "\x1b]133;A\x07";
const B: &str = "\x1b]133;B\x07";
const C: &str = "\x1b]133;C\x07";

fn mark(rig: &Rig, y: i32) -> Option<RowMark> {
    rig.term.row_mark(Line(y))
}

fn part(rig: &Rig, y: i32, part: BlockPart) -> String {
    rig.term.block_text(Line(y), 1, part).trim_end().to_string()
}

#[test]
fn a_prompt_redrawn_after_a_resize_without_a_new_a_is_still_the_prompt() {
    // bash with PROMPT_COMMAND sending A and the prompt string only B: readline redraws the string.
    let mut rig = Rig::new(10, 4);
    rig.feed(&format!("{A}$ {B}"));
    rig.term.resize(12, 4);
    rig.feed(&format!("\r\x1b[K$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(part(&rig, 0, BlockPart::Prompt), "$");
    // What is typed after the redraw is the command, as before.
    rig.feed("ls");
    assert_eq!(mark(&rig, 0).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(part(&rig, 0, BlockPart::Command), "ls");
}

#[test]
fn the_typed_text_stays_the_command_when_the_redraw_prints_it_again() {
    let mut rig = Rig::new(20, 4);
    rig.feed(&format!("{A}$ {B}ls -l"));
    rig.feed(&format!("\r\x1b[K$ {B}ls -l"));
    assert_eq!(part(&rig, 0, BlockPart::Prompt), "$");
    assert_eq!(part(&rig, 0, BlockPart::Command), "ls -l");
}

#[test]
fn a_multi_line_prompt_redrawn_without_a_new_a_is_the_prompt_on_every_row() {
    let mut rig = Rig::new(20, 5);
    rig.feed(&format!("{A}~/work main\r\n$ {B}"));
    rig.feed(&format!("\x1b[A\r\x1b[J~/work main\r\n$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_continuation_prompt_redrawn_alone_leaves_the_command_line_above_it_alone() {
    let mut rig = Rig::new(20, 5);
    rig.feed(&format!("{A}$ {B}echo \\\r\n\x1b]133;A;k=s\x07> {B}foo"));
    rig.feed(&format!("\r\x1b[K> {B}foo"));
    assert_eq!(mark(&rig, 0).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(part(&rig, 0, BlockPart::Command), "echo \\");
    assert_eq!(mark(&rig, 1).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(part(&rig, 1, BlockPart::Command), "foo");
}

#[test]
fn a_command_that_wraps_is_not_turned_into_prompt() {
    let mut rig = Rig::new(10, 5);
    rig.feed(&format!("{A}$ {B}aaaaaaaaaabbbbb"));
    // The redraw starts at the first row, as a shell moves up before it draws the prompt again.
    rig.feed(&format!("\x1b[A\r$ {B}"));
    assert_eq!(mark(&rig, 0).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(part(&rig, 0, BlockPart::Prompt), "$");
    assert_eq!(mark(&rig, 1).map(|m| (m.prompt, m.command)), Some((false, true)));
}

#[test]
fn output_is_never_turned_into_prompt() {
    let mut rig = Rig::new(20, 5);
    rig.feed(&format!("{A}$ {B}ls\r\n{C}out\r\n"));
    // A B after the output began finds the pen in the output part, not the command's.
    rig.feed(&format!("more{B}"));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: false, command: false, output: true }));
    assert_eq!(mark(&rig, 2), Some(RowMark { block: 1, prompt: false, command: false, output: true }));
    assert_eq!(part(&rig, 2, BlockPart::Output), "more");
}

#[test]
fn a_shell_that_sends_a_with_every_redraw_starts_the_block_again_as_before() {
    let mut rig = Rig::new(10, 4);
    rig.feed(&format!("{A}$ {B}"));
    rig.feed(&format!("\r\x1b[K{A}$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 2, prompt: true, command: false, output: false }));
}

#[test]
fn what_a_full_screen_program_prints_before_a_b_is_not_prompt() {
    let mut rig = Rig::new(20, 4);
    rig.feed(&format!("{A}$ {B}vim\r\n{C}\x1b[?1049h"));
    rig.feed(&format!("\x1b[Hhello{B}"));
    assert_eq!(mark(&rig, 0), None);
}

#[test]
fn a_redraw_on_the_last_row_of_a_wrapped_command_leaves_the_typed_rows_alone() {
    // readline goes to the bottom line of the input, clears it and draws the prompt there.
    let mut rig = Rig::new(10, 5);
    rig.feed(&format!("{A}$ {B}aaaaaaaaaabbbbbbbbbbcc"));
    rig.feed(&format!("\r\x1b[K$ {B}"));
    assert_eq!(mark(&rig, 0).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 2).map(|m| (m.prompt, m.command)), Some((true, false)));
}

#[test]
fn a_prompt_without_an_a_of_its_own_takes_only_its_own_row() {
    // A secondary prompt that sends B but no A (PS2 in bash): the command line above stays command.
    let mut rig = Rig::new(20, 5);
    rig.feed(&format!("{A}$ {B}echo \\\r\n> {B}foo"));
    assert_eq!(mark(&rig, 0).map(|m| (m.prompt, m.command)), Some((true, true)));
    assert_eq!(part(&rig, 0, BlockPart::Command), "echo \\");
    assert_eq!(part(&rig, 1, BlockPart::Prompt), ">");
    assert_eq!(part(&rig, 1, BlockPart::Command), "foo");
}

#[test]
fn the_output_of_a_shell_that_sends_no_c_is_not_taken_by_a_prompt_drawn_under_it() {
    let mut rig = Rig::new(20, 8);
    // An empty line before it: its prompt row sits right above the next block, which counts only its own.
    rig.feed(&format!("{A}$ {B}\r\n\x1b]133;D;0\x07{A}$ {B}ls\r\nfile one\r\nfile two\r\n$ {B}"));
    assert_eq!(mark(&rig, 2), Some(RowMark { block: 2, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 3), Some(RowMark { block: 2, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 4).map(|m| (m.prompt, m.command)), Some((true, false)));
}

#[test]
fn a_prompt_that_fills_its_row_is_all_prompt_when_it_is_redrawn() {
    // The cursor waits in the last column for the next character: the cell under it is written.
    let mut rig = Rig::new(10, 4);
    rig.feed(&format!("{A}$ 1234567 {B}"));
    rig.feed(&format!("\r\x1b[K$ 1234567 {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(part(&rig, 0, BlockPart::Prompt), "$ 1234567");
}

#[test]
fn a_prompt_that_wraps_after_a_resize_is_prompt_on_both_rows() {
    // Narrower than before: the redraw is longer than the row, a line of the prompt wraps.
    let mut rig = Rig::new(20, 4);
    rig.feed(&format!("{A}abcdefghijkl$ {B}"));
    rig.term.resize(10, 4);
    rig.feed(&format!("\x1b[H\x1b[Jabcdefghijkl$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_b_while_the_pen_is_in_the_prompt_changes_no_cells() {
    let mut rig = Rig::new(20, 4);
    rig.feed(&format!("{A}$ {B}ls\x1b]133;A;k=s\x07> {B}"));
    assert_eq!(part(&rig, 0, BlockPart::Command), "ls");
    assert_eq!(part(&rig, 0, BlockPart::Prompt), "$ >");
}

#[test]
fn a_b_on_a_row_of_an_older_block_leaves_its_command_alone() {
    let mut rig = Rig::new(20, 6);
    rig.feed(&format!("{A}$ {B}ls\r\n{C}x\r\n\x1b]133;D;0\x07{A}$ {B}"));
    // The program moves the cursor back over the first block's command and sends B there.
    rig.feed(&format!("\x1b[3A\x1b[5G{B}"));
    assert_eq!(rig.term.block_text(Line(0), 1, BlockPart::Command).trim_end(), "ls");
}

#[test]
fn a_prompt_of_three_lines_is_prompt_on_all_three_rows_when_it_is_redrawn() {
    let mut rig = Rig::new(20, 6);
    rig.feed(&format!("{A}one\r\ntwo\r\n$ {B}"));
    rig.feed(&format!("\x1b[2A\r\x1b[Jone\r\ntwo\r\n$ {B}"));
    for y in 0..3 {
        assert_eq!(mark(&rig, y), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    }
}

#[test]
fn the_rows_of_a_command_that_ran_are_not_taken_through_its_output() {
    // A prompt of three lines allows a walk of three; output (after C) ends it all the same.
    let mut rig = Rig::new(10, 9);
    rig.feed(&format!("{A}one\r\ntwo\r\n$ {B}xxxxxxxxxxxxx\r\n{C}out\r\n{B}$ {B}"));
    assert_eq!(mark(&rig, 3), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 4).map(|m| m.output), Some(true));
    assert_eq!(mark(&rig, 5).map(|m| (m.prompt, m.command)), Some((true, false)));
}

#[test]
fn a_prompt_at_the_top_of_the_screen_does_not_look_into_the_history_for_its_rows() {
    let mut rig = Rig::with_scrollback(10, 3, 10);
    // A line that wraps ends in the history, right above the screen's first row.
    rig.feed("xxxxxxxxxxxxxxx\x1b[S\x1b[H\x1b[J");
    rig.feed(&format!("{A}$ {B}"));
    rig.feed(&format!("\r\x1b[K$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_secondary_prompt_does_not_change_how_tall_the_prompt_was() {
    // The prompt is one line; the rows above the nested prompt are output of a shell that sends no C.
    let mut rig = Rig::new(20, 8);
    rig.feed(&format!("{A}$ {B}x\r\n\x1b]133;A;k=s\x07> {B}y\r\nout1\r\nout2\r\n$ {B}"));
    assert_eq!(mark(&rig, 3), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 4).map(|m| (m.prompt, m.command)), Some((true, false)));
}

#[test]
fn a_prompt_that_wraps_after_a_narrowing_resize_is_prompt_on_its_rows_with_readlines_redraw() {
    // The old prompt is reflowed over rows 0 and 1; readline clears the cursor's row and draws it again.
    let mut rig = Rig::new(20, 4);
    rig.feed(&format!("{A}abcdefghijkl$ {B}"));
    rig.term.resize(10, 4);
    rig.feed(&format!("\r\x1b[Kabcdefghijkl$ {B}"));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(mark(&rig, 2), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(rig.term.block_text(Line(1), 1, BlockPart::Command), "");
}

#[test]
fn a_prompt_whose_first_line_wraps_is_redrawn_whole_and_leaves_the_output_above_it_alone() {
    // Two logical lines, the first on two rows; drawn again (B only) under output of a shell that
    // sends no C. The walk takes the first line's two rows and stops at the output.
    let mut rig = Rig::new(10, 10);
    rig.feed(&format!("{A}0123456789abc\r\n$ {B}ls\r\nfile two\r\n0123456789abc\r\n$ {B}"));
    assert_eq!(mark(&rig, 3), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    for y in 4..=6 {
        assert_eq!(mark(&rig, y), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    }
}

#[test]
fn a_wrapped_prompt_line_at_the_top_of_the_screen_does_not_look_into_the_history() {
    let mut rig = Rig::with_scrollback(10, 3, 10);
    // A line that wraps ends in the history, right above the screen's first row.
    rig.feed("xxxxxxxxxxxxxxx\x1b[S\x1b[H\x1b[J");
    rig.feed(&format!("{A}one\r\n$ {B}"));
    rig.feed(&format!("\x1b[A\r\x1b[Jone\r\n$ {B}"));
    assert_eq!(mark(&rig, 0), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    assert_eq!(mark(&rig, 1), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_shorter_prompt_drawn_under_output_takes_no_row_that_does_not_wrap_into_it() {
    // After a `cd` the prompt is shorter than the one measured, which wrapped; the shell sends no C.
    let mut rig = Rig::new(12, 6);
    rig.feed(&format!("{A}~/src/wormhole$ {B}cd; ls\r\nnotes.md\r\n~$ {B}"));
    assert_eq!(mark(&rig, 2), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    assert_eq!(mark(&rig, 3), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_prompt_drawn_right_after_output_takes_no_more_rows_of_it_than_its_last_line_had() {
    // The measured prompt's last line wrapped onto two rows. The output (no C) ends without a
    // newline, so the prompt goes on in its line, and that line's rows above the prompt's stay.
    let mut rig = Rig::new(10, 10);
    rig.feed(&format!("{A}0123456789ab$ {B}x\r\n{}0123456789ab$ {B}", "a".repeat(25)));
    for y in 2..=3 {
        assert_eq!(mark(&rig, y), Some(RowMark { block: 1, prompt: false, command: true, output: false }));
    }
    assert_eq!(mark(&rig, 5), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
}

#[test]
fn a_prompt_that_filled_its_row_is_prompt_on_every_row_after_a_narrowing_to_two_columns() {
    // Measured with the cursor waiting in the last column: that cell is the prompt's too.
    let mut rig = Rig::new(10, 8);
    rig.feed(&format!("{A}abcdefghij{B}"));
    rig.term.resize(2, 8);
    rig.feed(&format!("\x1b[H\x1b[Jabcdefghij{B}"));
    for y in 0..5 {
        assert_eq!(mark(&rig, y), Some(RowMark { block: 1, prompt: true, command: false, output: false }));
    }
}
