//! Sets of players, one bit each: the seats of the room, who is ready, who plays a session.

use crate::game::PlayerId;

/// The most players an online room seats (and an online match holds): from a 1v1 up to a
/// brawl of ten, everybody for themselves.
pub const MAX_PLAYERS: usize = 10;

/// A set of players, as the two bytes that carry it on the wire: bit `i` is player `i`.
/// Only players 0 to 15 fit; larger ids are never members.
///
/// ```
/// use srr::game::PlayerId;
/// use srr::net::PlayerSet;
///
/// let trio = PlayerSet::first(3);
/// assert_eq!(trio.len(), 3);
/// assert!(trio.contains(PlayerId(2)) && !trio.contains(PlayerId(3)));
/// assert_eq!(trio.without(PlayerId(1)).iter().collect::<Vec<_>>(), [PlayerId(0), PlayerId(2)]);
/// assert_eq!(PlayerSet::from_bits(trio.bits()), trio);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PlayerSet(u16);

impl PlayerSet {
    /// Nobody.
    pub const EMPTY: PlayerSet = PlayerSet(0);
    /// The number of distinct players a set can hold.
    pub const CAPACITY: usize = u16::BITS as usize;

    /// The set encoded by `bits`.
    pub const fn from_bits(bits: u16) -> Self {
        PlayerSet(bits)
    }

    /// The bits sent on the wire.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Players `0..count` (all of them from [`PlayerSet::CAPACITY`] on).
    pub fn first(count: usize) -> Self {
        (0..count.min(Self::CAPACITY))
            .fold(Self::EMPTY, |set, index| set.with(PlayerId(index as u8)))
    }

    /// Whether `player` is in the set.
    pub fn contains(self, player: PlayerId) -> bool {
        Self::bit(player) & self.0 != 0
    }

    /// The set plus `player` (unchanged when the id does not fit).
    pub fn with(self, player: PlayerId) -> Self {
        PlayerSet(self.0 | Self::bit(player))
    }

    /// The set without `player`.
    pub fn without(self, player: PlayerId) -> Self {
        PlayerSet(self.0 & !Self::bit(player))
    }

    /// The number of players in the set.
    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether nobody is in the set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The players, by increasing id.
    pub fn iter(self) -> impl Iterator<Item = PlayerId> {
        (0..Self::CAPACITY as u8)
            .map(PlayerId)
            .filter(move |&player| self.contains(player))
    }

    fn bit(player: PlayerId) -> u16 {
        1u16.checked_shl(u32::from(player.0)).unwrap_or(0)
    }
}

impl FromIterator<PlayerId> for PlayerSet {
    fn from_iter<T: IntoIterator<Item = PlayerId>>(players: T) -> Self {
        players
            .into_iter()
            .fold(PlayerSet::EMPTY, |set, player| set.with(player))
    }
}
