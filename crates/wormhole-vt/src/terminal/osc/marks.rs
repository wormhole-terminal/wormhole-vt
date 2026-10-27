//! OSC 133: FinalTerm's shell marks, which divide the screen into blocks of prompt, command and
//! output.

use crate::grid::{BlockPart, RowMark};

use crate::terminal::Terminal;
use crate::terminal::event::{Event, PromptKind, ShellMark};
use crate::types::Line;

impl Terminal {
    /// OSC 133: `A` prompt, `B` command line, `C` output, `D` finished. The pen carries the mark
    /// so every cell printed afterwards belongs to the block; the rows learn their block from it.
    pub(in crate::terminal) fn osc_shell_mark(&mut self, rest: &[u8]) {
        let mut parts = rest.split(|&b| b == b';');
        let mark = match parts.next() {
            Some(b"A") => {
                let kind = match parts.find_map(|p| p.strip_prefix(b"k=")) {
                    Some(b"s") => PromptKind::Secondary,
                    Some(b"r") => PromptKind::Right,
                    Some(b"c") => PromptKind::Continuation,
                    _ => PromptKind::Initial,
                };
                if kind == PromptKind::Initial {
                    self.block = self.block.wrapping_add(1).max(1);
                    self.pen.block = self.block;
                    self.prompt_lines = 0;
                    if self.block_row_is_blank() {
                        self.grid.set_row_block(self.cursor.y, self.block);
                    }
                }
                if self.pen.block != 0 {
                    self.pen.content = BlockPart::Prompt;
                }
                ShellMark::PromptStart(kind)
            }
            Some(b"B") => {
                match self.pen.content {
                    BlockPart::Command => self.reclaim_redrawn_prompt(),
                    BlockPart::Prompt if self.prompt_lines == 0 => self.measure_prompt(),
                    _ => {}
                }
                self.set_content(BlockPart::Command);
                ShellMark::CommandStart
            }
            Some(b"C") => {
                self.set_content(BlockPart::Output);
                ShellMark::OutputStart
            }
            Some(b"D") => {
                self.pen.content = BlockPart::None;
                let code =
                    parts.next().and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse().ok());
                ShellMark::Finished(code)
            }
            _ => return,
        };
        self.push_event(Event::Mark(mark));
    }

    /// Whether the cursor's row has nothing on it yet.
    fn block_row_is_blank(&self) -> bool {
        self.grid.content_len(Line(self.cursor.y as i32)) == 0
    }

    /// The first `B` after a prompt's `A` says how big the prompt is: its height in logical lines (a
    /// line that wraps is one), counted from the rows above the cursor that hold the block's prompt,
    /// and how many cells its last line takes. A redraw is held to both (see
    /// [`reclaim_redrawn_prompt`](Self::reclaim_redrawn_prompt)); they are not rows because a resize
    /// wraps the same prompt onto more or fewer of them.
    fn measure_prompt(&mut self) {
        let block = self.pen.block;
        let cols = self.grid.cols();
        let mut lines = 1u16;
        let mut tail = self.cursor.x + usize::from(self.cursor.pending);
        let mut y = self.cursor.y;
        while y > 0 && self.row_is(y - 1, |mark| mark.block == block && mark.prompt) {
            if !self.grid.is_wrapped(Line(y as i32 - 1)) {
                lines = lines.saturating_add(1);
            } else if lines == 1 {
                tail += cols;
            }
            y -= 1;
        }
        self.prompt_lines = lines;
        self.prompt_tail = tail;
    }

    /// A `B` that comes while the pen is still in the command part (no `A` and no `C` since the last
    /// `B`) ends a prompt that was drawn again: a line editor redraws its prompt after a resize or a
    /// clear screen, and a shell that sends `A` once per prompt (bash's `PROMPT_COMMAND`) puts only the
    /// prompt text and the `B` into that redraw. The text went down in the command part, since only
    /// `A` leaves it, and the row would lose its prompt: no block start, no gutter mark. Terminals
    /// that keep a row's prompt apart (kitty, Ghostty, WezTerm, iTerm2) get this right for shells
    /// that put `A` into the prompt string, where each redraw says it; here the `B` is the only word
    /// there is, so what lies before it on the cursor's row is the prompt, and so are the rows above
    /// that lost their prompt too. It goes no further than that on purpose:
    /// - the cursor's own line takes as many rows above the cursor as the prompt's last line needs
    ///   (`measure_prompt`): a prompt that wraps after a narrowing resize is longer than the row,
    ///   while a command that wrapped has its prompt at the top and the redraw on the last row
    ///   (readline goes to the bottom line of the input), so the rows between are what was typed;
    /// - above that, no more logical lines than the prompt had: a shell that skips `C` leaves its
    ///   output in the command part, and a prompt of one line drawn under it must not take it;
    /// - a line that still has prompt cells is left alone, and so is everything above it, which keeps
    ///   a continuation line's `>` redrawn alone from turning the command line above it into prompt;
    /// - other blocks' rows end the walk, and only command cells change, so output (after `C` the
    ///   pen is not in the command part) is never touched;
    /// - a shell that sends `A` with each redraw (zsh, fish) never gets here: its pen is in the prompt.
    ///
    /// Known limit: with a prompt of two lines or more, up to one row fewer than its height of a
    /// shell's output (one that sends no `C`) or of lines typed after a B-only prompt, right above the
    /// cursor, can be taken as well; a prompt of one line takes none.
    fn reclaim_redrawn_prompt(&mut self) {
        let block = self.pen.block;
        let lost = |mark: RowMark| mark.block == block && !mark.prompt && !mark.output;
        let cols = self.grid.cols();
        let mut top = self.cursor.y;
        if self.grid.row_view(Line(top as i32)).is_none_or(|row| row.block() != block) {
            return;
        }
        // A cursor with a wrap pending sits on the last cell that was written.
        let end = self.cursor.x + usize::from(self.cursor.pending);
        self.grid.retag_content(top, 0..end, BlockPart::Command, BlockPart::Prompt);
        // The rest of the prompt's last line, when it wrapped: the redraw's own rows, which wrap.
        for _ in 0..self.prompt_tail.saturating_sub(end).div_ceil(cols) {
            if top == 0 || !self.grid.is_wrapped(Line(top as i32 - 1)) || !self.row_is(top - 1, lost) {
                return;
            }
            top -= 1;
            self.grid.retag_content(top, 0..cols, BlockPart::Command, BlockPart::Prompt);
        }
        // The lines above it, taken whole.
        for _ in 1..self.prompt_lines {
            if top == 0 {
                return;
            }
            let bottom = top - 1;
            top = bottom;
            while top > 0 && self.grid.is_wrapped(Line(top as i32 - 1)) {
                top -= 1;
            }
            if !(top..=bottom).all(|row| self.row_is(row, lost)) {
                return;
            }
            for row in top..=bottom {
                self.grid.retag_content(row, 0..cols, BlockPart::Command, BlockPart::Prompt);
            }
        }
    }

    /// Whether live row `y` carries a block mark that `test` accepts.
    fn row_is(&self, y: usize, test: impl Fn(RowMark) -> bool) -> bool {
        self.grid.row_mark(Line(y as i32)).is_some_and(test)
    }

    fn set_content(&mut self, content: BlockPart) {
        if self.pen.block != 0 && !self.on_alt_screen() {
            self.pen.content = content;
        }
    }
}
