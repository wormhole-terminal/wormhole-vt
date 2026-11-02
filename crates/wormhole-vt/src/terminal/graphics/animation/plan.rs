//! The plan of an animation: frames' gaps, state, loops and the current frame; and the stepper
//! a host plays it with. See the [parent module](super) for why it is shaped like this.

use std::time::Duration;

/// Most frames one picture has, the root frame included. Past it a new frame is refused with
/// `EFBIG`: the quota counts a frame's pixels, but a stack of one-pixel frames would cost the
/// host a canvas, an entry and an event each for four bytes of quota.
pub const MAX_FRAMES: usize = 4096;

/// The gap a new frame gets when `z` is 0 or absent: kitty's own default.
pub(super) const DEFAULT_GAP_MS: u32 = 40;

/// How long a frame is shown before the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    /// This many milliseconds. 0 is a frame shown for no time (the root frame starts so).
    Millis(u32),
    /// Skipped over, never shown while the animation runs; useful as the base other frames are
    /// built on.
    Gapless,
}

impl Gap {
    /// The kitty `z` value for a gap: negative is gapless.
    pub(in crate::terminal::graphics) fn from_z(z: i32) -> Gap {
        if z < 0 { Gap::Gapless } else { Gap::Millis(z.unsigned_abs()) }
    }

    fn duration(self) -> Duration {
        match self {
            Gap::Millis(ms) => Duration::from_millis(u64::from(ms)),
            Gap::Gapless => Duration::ZERO,
        }
    }
}

/// kitty's `s` of `a=a`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationState {
    /// `s=1` (and the state of a picture nobody started): the current frame is shown and nothing
    /// moves.
    #[default]
    Stopped,
    /// `s=2`: runs, but at the last frame waits for more instead of looping.
    Loading,
    /// `s=3`: runs and loops.
    Running,
}

/// kitty's `v` of `a=a`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Loops {
    /// Forever: `v=1`, and the default.
    #[default]
    Infinite,
    /// This many passes through the frames in all (`v=n` is `n - 1`).
    Passes(u32),
}

/// Where a host is in an animation: the frame it shows and the pass through the frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// The frame shown, counted from 0 (the root frame).
    pub frame: usize,
    /// The pass through the frames, counted from 0.
    pub pass: u32,
}

/// What follows a frame being shown; see [`Animation::tick`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Nothing follows: the animation is stopped, has played its passes, waits for frames that
    /// have not come, or would take no time to go round.
    Hold,
    /// After `wait` show `next`. A `wait` of zero is a frame to skip.
    After {
        /// How long the frame is shown.
        wait: Duration,
        /// What is shown then.
        next: Position,
    },
}

/// The plan of one picture's animation: read it with [`Terminal::animation`](crate::terminal::Terminal::animation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    /// One gap per frame; never empty (the root frame is the picture itself).
    gaps: Vec<Gap>,
    state: AnimationState,
    loops: Loops,
    current: usize,
    generation: u64,
}

impl Default for Animation {
    fn default() -> Animation {
        Animation {
            gaps: vec![Gap::Millis(0)],
            state: AnimationState::Stopped,
            loops: Loops::Infinite,
            current: 0,
            generation: 0,
        }
    }
}

impl Animation {
    /// How many frames, the root frame included (at least 1).
    pub fn frame_count(&self) -> usize {
        self.gaps.len()
    }

    /// The gap of a frame (counted from 0), `None` past the last.
    pub fn gap(&self, frame: usize) -> Option<Gap> {
        self.gaps.get(frame).copied()
    }

    /// The gaps of all frames, in order.
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// What the program asked for: stopped, running or loading.
    pub fn state(&self) -> AnimationState {
        self.state
    }

    /// How many passes the program asked for.
    pub fn loops(&self) -> Loops {
        self.loops
    }

    /// The current frame, counted from 0: the one a program made current, or the one the host
    /// last moved on to ([`Terminal::set_animation_current`](crate::terminal::Terminal::set_animation_current)).
    /// A stopped animation shows it.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Changes whenever the program starts, stops or re-aims the animation, or deletes frames
    /// (not when frames or gaps are added, nor when the host reports its position): the host
    /// starts over from [`start`](Self::start) when it sees a new number.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Where a host starts playing: the current frame, no pass done.
    pub fn start(&self) -> Position {
        Position { frame: self.current, pass: 0 }
    }

    /// What follows `at`, which the host is showing. Call it again after the wait, with `next`.
    /// Call it with the position it holds when [`Terminal::events`](crate::terminal::Terminal::events)
    /// brings an [`Event::ImageAnimation`](crate::terminal::Event::ImageAnimation) and the
    /// [`generation`](Self::generation) is the same: frames that came meanwhile, or new gaps, show
    /// in the answer.
    pub fn tick(&self, at: Position) -> Tick {
        if self.state == AnimationState::Stopped || at.frame >= self.gaps.len() {
            return Tick::Hold;
        }
        // A pass that takes no time would be a loop the host cannot leave.
        if self.gaps.iter().all(|gap| gap.duration().is_zero()) {
            return Tick::Hold;
        }
        let wait = self.gaps[at.frame].duration();
        let next = if at.frame + 1 < self.gaps.len() {
            Position { frame: at.frame + 1, pass: at.pass }
        } else {
            if self.state == AnimationState::Loading {
                return Tick::Hold;
            }
            let pass = at.pass.saturating_add(1);
            if matches!(self.loops, Loops::Passes(n) if pass >= n) {
                return Tick::Hold;
            }
            Position { frame: 0, pass }
        };
        Tick::After { wait, next }
    }

    // What the commands do.

    pub(in crate::terminal::graphics) fn set_state(&mut self, state: AnimationState) {
        self.state = state;
        self.generation += 1;
    }

    pub(in crate::terminal::graphics) fn set_loops(&mut self, loops: Loops) {
        self.loops = loops;
        self.generation += 1;
    }

    /// The program made `frame` (0-based, must exist) the current frame.
    pub(in crate::terminal::graphics) fn set_current(&mut self, frame: usize) {
        self.current = frame;
        self.generation += 1;
    }

    pub(in crate::terminal::graphics) fn set_current_quietly(&mut self, frame: usize) {
        if frame < self.gaps.len() {
            self.current = frame;
        }
    }

    pub(in crate::terminal::graphics) fn set_gap(&mut self, frame: usize, gap: Gap) {
        if let Some(slot) = self.gaps.get_mut(frame) {
            *slot = gap;
        }
    }

    /// Add a frame at the end.
    pub(in crate::terminal::graphics) fn push_frame(&mut self, gap: Gap) {
        self.gaps.push(gap);
    }

    /// Take the last frame off again (its data could not be decoded). Never the root.
    pub(in crate::terminal::graphics) fn pop_frame(&mut self) {
        if self.gaps.len() > 1 {
            self.gaps.pop();
            if self.current >= self.gaps.len() {
                self.current = self.gaps.len() - 1;
            }
            self.generation += 1;
        }
    }

    /// Delete every frame but the root, and stop.
    pub(in crate::terminal::graphics) fn clear_frames(&mut self) {
        self.gaps.truncate(1);
        self.current = 0;
        self.state = AnimationState::Stopped;
        self.loops = Loops::Infinite;
        self.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{Animation, AnimationState, Duration, Gap, Loops, Position, Tick};

    fn plan(gaps: &[Gap], state: AnimationState, loops: Loops) -> Animation {
        Animation { gaps: gaps.to_vec(), state, loops, ..Animation::default() }
    }

    fn after(ms: u64, frame: usize, pass: u32) -> Tick {
        Tick::After { wait: Duration::from_millis(ms), next: Position { frame, pass } }
    }

    #[test]
    fn a_stopped_animation_holds() {
        let a = plan(&[Gap::Millis(10), Gap::Millis(10)], AnimationState::Stopped, Loops::Infinite);
        assert_eq!(a.tick(a.start()), Tick::Hold);
    }

    #[test]
    fn a_running_animation_goes_round_for_ever_by_default() {
        let a = plan(&[Gap::Millis(10), Gap::Millis(20)], AnimationState::Running, Loops::Infinite);
        assert_eq!(a.tick(Position { frame: 0, pass: 0 }), after(10, 1, 0));
        assert_eq!(a.tick(Position { frame: 1, pass: 0 }), after(20, 0, 1));
        assert_eq!(a.tick(Position { frame: 1, pass: u32::MAX }), after(20, 0, u32::MAX), "saturates");
    }

    #[test]
    fn passes_are_counted_and_the_last_frame_stays_when_they_are_done() {
        let a = plan(&[Gap::Millis(5), Gap::Millis(5)], AnimationState::Running, Loops::Passes(2));
        assert_eq!(a.tick(Position { frame: 1, pass: 0 }), after(5, 0, 1));
        assert_eq!(a.tick(Position { frame: 1, pass: 1 }), Tick::Hold);
        let once = plan(&[Gap::Millis(5), Gap::Millis(5)], AnimationState::Running, Loops::Passes(1));
        assert_eq!(once.tick(Position { frame: 1, pass: 0 }), Tick::Hold);
        assert_eq!(once.tick(Position { frame: 0, pass: 0 }), after(5, 1, 0));
    }

    #[test]
    fn loading_waits_at_the_last_frame_whatever_the_loops() {
        let a = plan(&[Gap::Millis(5), Gap::Millis(5)], AnimationState::Loading, Loops::Infinite);
        assert_eq!(a.tick(Position { frame: 0, pass: 0 }), after(5, 1, 0));
        assert_eq!(a.tick(Position { frame: 1, pass: 0 }), Tick::Hold);
    }

    #[test]
    fn gapless_frames_come_out_as_a_wait_of_zero() {
        let a =
            plan(&[Gap::Millis(5), Gap::Gapless, Gap::Millis(5)], AnimationState::Running, Loops::Infinite);
        assert_eq!(a.tick(Position { frame: 0, pass: 0 }), after(5, 1, 0));
        assert_eq!(a.tick(Position { frame: 1, pass: 0 }), after(0, 2, 0));
    }

    #[test]
    fn a_pass_that_takes_no_time_is_a_hold_not_a_busy_loop() {
        let a = plan(&[Gap::Gapless, Gap::Millis(0)], AnimationState::Running, Loops::Infinite);
        assert_eq!(a.tick(a.start()), Tick::Hold);
    }

    #[test]
    fn a_position_past_the_frames_holds() {
        let a = plan(&[Gap::Millis(5)], AnimationState::Running, Loops::Infinite);
        assert_eq!(a.tick(Position { frame: 9, pass: 0 }), Tick::Hold);
    }

    #[test]
    fn gaps_come_from_z() {
        assert_eq!(Gap::from_z(48), Gap::Millis(48));
        assert_eq!(Gap::from_z(-1), Gap::Gapless);
        assert_eq!(Gap::from_z(i32::MIN), Gap::Gapless);
        assert_eq!(Gap::from_z(0), Gap::Millis(0));
    }

    #[test]
    fn the_hosts_position_is_remembered_without_a_new_generation() {
        let mut a = plan(&[Gap::Millis(5), Gap::Millis(5)], AnimationState::Running, Loops::Infinite);
        a.set_current_quietly(1);
        assert_eq!((a.current(), a.generation()), (1, 0));
        a.set_current_quietly(7);
        assert_eq!(a.current(), 1, "no such frame");
        a.set_current(0);
        assert_eq!((a.current(), a.generation()), (0, 1));
    }

    #[test]
    fn popping_the_last_frame_never_takes_the_root_and_moves_current_in() {
        let mut a = plan(&[Gap::Millis(5), Gap::Millis(5)], AnimationState::Stopped, Loops::Infinite);
        a.set_current(1);
        a.pop_frame();
        assert_eq!((a.frame_count(), a.current()), (1, 0));
        a.pop_frame();
        assert_eq!(a.frame_count(), 1);
    }
}
