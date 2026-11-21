//! Deterministic sprite-sequence timing shared by the 2D renderer.
//!
//! A frame names a pose cell and how long it stays visible. The timeline is independent of wall
//! time, so the same game tick always selects the same frame in local play, replays and rollback.

use super::skin::Pose;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Playback {
    Loop,
    #[allow(dead_code)]
    Once,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    pose: Pose,
    ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sequence<'a> {
    frames: &'a [Frame],
    playback: Playback,
}

impl Sequence<'_> {
    fn pose_at(self, tick: u32, fallback: Pose) -> Pose {
        let duration: u64 = self.frames.iter().map(|frame| u64::from(frame.ticks)).sum();
        if duration == 0 {
            return fallback;
        }

        let tick = match self.playback {
            Playback::Loop => u64::from(tick) % duration,
            Playback::Once => u64::from(tick).min(duration - 1),
        };
        let mut elapsed = 0;
        self.frames
            .iter()
            .find_map(|frame| {
                elapsed += u64::from(frame.ticks);
                (tick < elapsed).then_some(frame.pose)
            })
            .unwrap_or(fallback)
    }
}

const WALK_FRAMES: [Frame; 4] = [
    Frame {
        pose: Pose::WalkA,
        ticks: 4,
    },
    Frame {
        pose: Pose::WalkC,
        ticks: 4,
    },
    Frame {
        pose: Pose::WalkB,
        ticks: 4,
    },
    Frame {
        pose: Pose::WalkD,
        ticks: 4,
    },
];

const WALK_CYCLE: Sequence<'static> = Sequence {
    frames: &WALK_FRAMES,
    playback: Playback::Loop,
};

/// The walk pose at a deterministic game tick. The four-frame loop keeps the existing sixteen-tick
/// stride cadence while showing a generated passing pose between each opposing stride.
pub(super) fn walking_pose(tick: u32) -> Pose {
    WALK_CYCLE.pose_at(tick, Pose::WalkA)
}

#[cfg(test)]
mod tests {
    use super::{Frame, Playback, Sequence, WALK_CYCLE, walking_pose};
    use crate::front::skin::Pose;

    #[test]
    fn walking_cycle_advances_through_inbetweens_and_loops_at_frame_boundaries() {
        assert_eq!(walking_pose(0), Pose::WalkA);
        assert_eq!(walking_pose(3), Pose::WalkA);
        assert_eq!(walking_pose(4), Pose::WalkC);
        assert_eq!(walking_pose(7), Pose::WalkC);
        assert_eq!(walking_pose(8), Pose::WalkB);
        assert_eq!(walking_pose(11), Pose::WalkB);
        assert_eq!(walking_pose(12), Pose::WalkD);
        assert_eq!(walking_pose(15), Pose::WalkD);
        assert_eq!(walking_pose(16), Pose::WalkA);
        assert_eq!(walking_pose(u32::MAX), Pose::WalkD);
    }

    #[test]
    fn once_sequences_hold_the_last_frame() {
        let frames = [
            Frame {
                pose: Pose::Stand,
                ticks: 2,
            },
            Frame {
                pose: Pose::PunchMid,
                ticks: 3,
            },
        ];
        let sequence = Sequence {
            frames: &frames,
            playback: Playback::Once,
        };
        assert_eq!(sequence.pose_at(0, Pose::Ko), Pose::Stand);
        assert_eq!(sequence.pose_at(2, Pose::Ko), Pose::PunchMid);
        assert_eq!(sequence.pose_at(4, Pose::Ko), Pose::PunchMid);
        assert_eq!(sequence.pose_at(u32::MAX, Pose::Ko), Pose::PunchMid);
    }

    #[test]
    fn empty_and_zero_duration_sequences_use_the_fallback() {
        let empty = Sequence {
            frames: &[],
            playback: Playback::Loop,
        };
        let zero_duration_frames = [Frame {
            pose: Pose::Hit,
            ticks: 0,
        }];
        let zero_duration = Sequence {
            frames: &zero_duration_frames,
            playback: Playback::Once,
        };
        assert_eq!(empty.pose_at(0, Pose::Ko), Pose::Ko);
        assert_eq!(zero_duration.pose_at(5, Pose::Ko), Pose::Ko);
    }

    #[test]
    fn walking_cycle_uses_the_shared_timeline_definition() {
        assert_eq!(WALK_CYCLE.frames.len(), 4);
        assert_eq!(WALK_CYCLE.playback, Playback::Loop);
        assert_eq!(
            WALK_CYCLE
                .frames
                .iter()
                .map(|frame| frame.ticks)
                .sum::<u32>(),
            16
        );
        assert_eq!(
            WALK_CYCLE
                .frames
                .iter()
                .map(|frame| frame.pose)
                .collect::<Vec<_>>(),
            [Pose::WalkA, Pose::WalkC, Pose::WalkB, Pose::WalkD]
        );
    }
}
