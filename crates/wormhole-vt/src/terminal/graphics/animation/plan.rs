//! The plan of an animation: frames' gaps, state, loops and the current frame; and the stepper
//! a host plays it with. See the [parent module](super) for why it is shaped like this.

use std::sync::Arc;
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

    fn millis(self) -> u64 {
        match self {
            Gap::Millis(ms) => u64::from(ms),
            Gap::Gapless => 0,
        }
    }

    fn duration(self) -> Duration {
        Duration::from_millis(self.millis())
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

/// Where an animation has got to after a long time; see [`Animation::skip_passes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The position to go on from.
    pub position: Position,
    /// How much of the time is still to be played from `position`: less than one pass.
    pub lag: Duration,
    /// The animation played all its passes in that time and stands on its last frame.
    pub finished: bool,
}

/// The plan of one picture's animation: read it with [`Terminal::animation`](crate::terminal::Terminal::animation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    /// One gap per frame; never empty (the root frame is the picture itself). Shared, so that a
    /// host that reads the plan every frame copies a pointer; a change copies the gaps only if a
    /// reader still holds the old ones.
    gaps: Arc<Vec<Gap>>,
    /// The time one pass through the frames takes, in milliseconds: kept as the gaps change so that
    /// [`tick`](Self::tick) does not add them up.
    pass_ms: u64,
    state: AnimationState,
    loops: Loops,
    current: usize,
    generation: u64,
}

impl Default for Animation {
    fn default() -> Animation {
        Animation {
            gaps: Arc::new(vec![Gap::Millis(0)]),
            pass_ms: 0,
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

    /// How long one pass through the frames takes; zero when no frame has a gap, which is an
    /// animation with nothing to show over time.
    pub fn pass_duration(&self) -> Duration {
        Duration::from_millis(self.pass_ms)
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
        if self.pass_ms == 0 {
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

    /// A host that was away (a hidden window) comes back `lag` after the frame at `at` came on:
    /// skip the whole passes it missed at once, so that catching up costs the frames of one pass
    /// and not of the time away. Only a running animation has passes to skip; any other, and one
    /// that takes no time, is returned as it is. A [`Loops::Passes`] animation that would have
    /// played out in `lag` is `finished`, on its last frame.
    pub fn skip_passes(&self, at: Position, lag: Duration) -> Skipped {
        let unchanged = Skipped { position: at, lag, finished: false };
        let total = u128::from(self.pass_ms) * 1_000_000;
        if self.state != AnimationState::Running || total == 0 {
            return unchanged;
        }
        let lag_ns = lag.as_nanos();
        let passes = lag_ns / total;
        if passes == 0 {
            return unchanged;
        }
        if let Loops::Passes(n) = self.loops
            && passes >= u128::from(n.saturating_sub(at.pass))
        {
            let last = Position { frame: self.gaps.len() - 1, pass: n.saturating_sub(1) };
            return Skipped { position: last, lag: Duration::ZERO, finished: true };
        }
        let rest = lag_ns % total;
        let pass = at.pass.saturating_add(u32::try_from(passes).unwrap_or(u32::MAX));
        Skipped {
            position: Position { frame: at.frame, pass },
            lag: Duration::new((rest / 1_000_000_000) as u64, (rest % 1_000_000_000) as u32),
            finished: false,
        }
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
        let gaps = Arc::make_mut(&mut self.gaps);
        if let Some(slot) = gaps.get_mut(frame) {
            self.pass_ms = self.pass_ms - slot.millis() + gap.millis();
            *slot = gap;
        }
    }

    /// Add a frame at the end.
    pub(in crate::terminal::graphics) fn push_frame(&mut self, gap: Gap) {
        self.pass_ms += gap.millis();
        Arc::make_mut(&mut self.gaps).push(gap);
    }

    /// Delete frame `frame` (counted from 0), root included, as kitty does
    /// (`handle_delete_frame_command` in its `graphics.c`): the later frames move up one, and the
    /// current frame follows the one it was on, or the next one when that was the frame deleted.
    /// The last frame left is never deleted. Returns whether one was.
    pub(in crate::terminal::graphics) fn remove_frame(&mut self, frame: usize) -> bool {
        if self.gaps.len() < 2 || frame >= self.gaps.len() {
            return false;
        }
        let removed = Arc::make_mut(&mut self.gaps).remove(frame);
        self.pass_ms -= removed.millis();
        if self.current > self.gaps.len() - 1 {
            self.current = self.gaps.len() - 1;
        } else if self.current > frame {
            self.current -= 1;
        }
        self.generation += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Animation, AnimationState, Duration, Gap, Loops, Position, Tick};

    fn plan(gaps: &[Gap], state: AnimationState, loops: Loops) -> Animation {
        let pass_ms = gaps.iter().map(|gap| gap.millis()).sum();
        Animation { gaps: Arc::new(gaps.to_vec()), pass_ms, state, loops, ..Animation::default() }
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
    fn deleting_a_frame_moves_the_later_ones_up_and_the_current_frame_with_its_picture() {
        let gaps = [Gap::Millis(1), Gap::Millis(2), Gap::Millis(4), Gap::Millis(8)];
        let mut a = plan(&gaps, AnimationState::Running, Loops::Infinite);
        a.set_current(2);
        assert!(a.remove_frame(0), "the root can go too");
        assert_eq!((a.gaps(), a.current(), a.pass_duration()), (&gaps[1..], 1, Duration::from_millis(14)));
        a.set_current(1);
        assert!(a.remove_frame(1));
        assert_eq!((a.current(), a.frame_count()), (1, 2), "the next frame is the current one now");
        a.set_current(1);
        assert!(a.remove_frame(0));
        assert_eq!((a.current(), a.frame_count()), (0, 1));
        assert!(!a.remove_frame(0), "the last frame stays");
        assert!(!a.remove_frame(5));
    }

    #[test]
    fn whole_passes_that_were_missed_are_skipped_at_once() {
        let a = plan(&[Gap::Millis(10), Gap::Millis(20)], AnimationState::Running, Loops::Infinite);
        let at = Position { frame: 1, pass: 3 };
        let ms = Duration::from_millis;
        let skipped = a.skip_passes(at, ms(95));
        assert_eq!(skipped.position, Position { frame: 1, pass: 6 }, "three passes of 30 ms");
        assert_eq!((skipped.lag, skipped.finished), (ms(5), false));
        assert_eq!(a.skip_passes(at, ms(29)).position, at, "less than a pass: nothing to skip");
        let huge = a.skip_passes(at, Duration::from_secs(u64::MAX / 2));
        assert!(huge.lag < ms(30) && huge.position.pass >= 3 && !huge.finished);
        let once = plan(&[Gap::Millis(10), Gap::Millis(20)], AnimationState::Running, Loops::Passes(2));
        let done = once.skip_passes(Position { frame: 0, pass: 0 }, ms(60));
        assert_eq!((done.position, done.finished), (Position { frame: 1, pass: 1 }, true));
        let more = once.skip_passes(Position { frame: 0, pass: 0 }, ms(30));
        assert_eq!((more.position.pass, more.finished), (1, false), "one of two passes");
        let stopped = plan(&[Gap::Millis(10)], AnimationState::Stopped, Loops::Infinite);
        assert_eq!(stopped.skip_passes(at, ms(1000)).position, at);
        let instant = plan(&[Gap::Gapless], AnimationState::Running, Loops::Infinite);
        assert_eq!(instant.skip_passes(at, ms(1000)).position, at, "a pass of no time has nothing to skip");
    }

    #[test]
    fn the_pass_duration_follows_every_change_of_a_gap() {
        let mut a = plan(&[Gap::Millis(5), Gap::Gapless], AnimationState::Running, Loops::Infinite);
        assert_eq!(a.pass_duration(), Duration::from_millis(5));
        a.set_gap(1, Gap::Millis(7));
        a.push_frame(Gap::Millis(3));
        assert_eq!(a.pass_duration(), Duration::from_millis(15));
        a.set_gap(0, Gap::Gapless);
        assert_eq!(a.pass_duration(), Duration::from_millis(10));
        a.set_gap(9, Gap::Millis(1_000));
        assert_eq!(a.pass_duration(), Duration::from_millis(10), "no such frame");
    }

    #[test]
    fn a_plan_is_shared_not_copied_until_it_changes() {
        let mut a = plan(&[Gap::Millis(5)], AnimationState::Running, Loops::Infinite);
        let seen = a.clone();
        assert!(Arc::ptr_eq(&a.gaps, &seen.gaps));
        a.push_frame(Gap::Millis(1));
        assert!(!Arc::ptr_eq(&a.gaps, &seen.gaps), "a change leaves the reader's copy alone");
        assert_eq!(seen.frame_count(), 1);
    }
}
