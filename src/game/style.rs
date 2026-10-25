//! How a fighter is drawn: its skin ([`Style`]). The rules of the fight never change — a skin
//! is only looks, picked by each player for their own fighter and shown to everybody, so a
//! match can mix them (a stick figure against a comic-book heroine, a whole brawl of mixed
//! styles; see the [skins](crate::front::skin) the front draws).
//!
//! There are three styles, [`Style::ALL`] in the order of the menus:
//!
//! * [`Style::Stick`], the default: the chalk stick figures of the alley wall (see
//!   [`stick`](crate::front::stick)).
//! * [`Style::Realistic`]: full-colour fighters with faces, hair and physiques, in the
//!   high-fidelity look of modern fighting games.
//! * [`Style::Manga`]: bright cel-shaded fighters with big eyes and bold ink outlines, the
//!   look of a Saturday-morning anime.
//!
//! A skin is one of the ten [`Variant`]s of a style: three women (brunette, blonde,
//! red-haired), an athletic man, a stocky brawler, a wiry youth, an elder, a soldier, a
//! punk, a masked mystic — every style draws its own take of each of them, so player 1 and
//! player 7 can both wear the "brunette" skin and still look nothing alike.
//!
//! [`Style`] and [`Variant`] are both one byte on the wire (their index), like
//! [`Character`](super::Character); [`StyleSet`] is the bitmask of the styles a server allows
//! (see [`ServerMessage::Styles`](crate::net::ServerMessage::Styles)), and a client only
//! offers those to its player.

use std::fmt;

/// How a fighter is drawn: one of the three [`Style::ALL`], each a family of ten
/// [`Variant`] skins.
///
/// ```
/// use srr::game::Style;
///
/// assert_eq!(Style::default(), Style::Stick);
/// for style in Style::ALL {
///     assert_eq!(Style::from_index(style.index()), Some(style));
/// }
/// assert_eq!(Style::from_index(3), None);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Style {
    /// The chalk stick figures on the alley wall (the default).
    #[default]
    Stick,
    /// Full-colour fighters with faces, hair and physiques.
    Realistic,
    /// Bright cel-shaded fighters, big eyes, bold ink outlines.
    Manga,
}

impl Style {
    /// Every style, in the order of the menus.
    pub const ALL: [Style; 3] = [Style::Stick, Style::Realistic, Style::Manga];

    /// Position in [`Style::ALL`]: the byte that stands for the style on the wire.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The style at `index` in [`Style::ALL`], if any.
    pub fn from_index(index: u8) -> Option<Style> {
        Style::ALL.get(usize::from(index)).copied()
    }

    /// The next style of the menus, wrapping around.
    ///
    /// ```
    /// use srr::game::Style;
    ///
    /// assert_eq!(Style::Stick.next(), Style::Realistic);
    /// assert_eq!(Style::Manga.next(), Style::Stick);
    /// ```
    pub const fn next(self) -> Style {
        match self {
            Style::Stick => Style::Realistic,
            Style::Realistic => Style::Manga,
            Style::Manga => Style::Stick,
        }
    }

    /// The previous style of the menus, wrapping around.
    pub const fn previous(self) -> Style {
        match self {
            Style::Stick => Style::Manga,
            Style::Realistic => Style::Stick,
            Style::Manga => Style::Realistic,
        }
    }

    /// The first style of `allowed` from here on, wrapping around; [`Style::default`] when
    /// nothing is allowed (the menus then show no choice).
    pub fn next_allowed(self, allowed: StyleSet) -> Style {
        let mut style = self;
        for _ in 0..Style::ALL.len() {
            style = style.next();
            if allowed.contains(style) {
                return style;
            }
        }
        self
    }

    /// The last style of `allowed` before this one, wrapping around; [`Style::default`] when
    /// nothing is allowed.
    pub fn previous_allowed(self, allowed: StyleSet) -> Style {
        let mut style = self;
        for _ in 0..Style::ALL.len() {
            style = style.previous();
            if allowed.contains(style) {
                return style;
            }
        }
        self
    }

    /// The style as the menus write it.
    ///
    /// ```
    /// use srr::game::Style;
    ///
    /// assert_eq!(Style::Stick.label(), "Stick (chalk)");
    /// assert_eq!(Style::Manga.label(), "Manga");
    /// ```
    pub const fn label(self) -> &'static str {
        match self {
            Style::Stick => "Stick (chalk)",
            Style::Realistic => "Realistic",
            Style::Manga => "Manga",
        }
    }

    /// Whether the style has the ten [`Variant`] skins or is the plain chalk ink.
    ///
    /// ```
    /// use srr::game::Style;
    ///
    /// assert!(!Style::Stick.is_skinned());
    /// assert!(Style::Realistic.is_skinned());
    /// assert!(Style::Manga.is_skinned());
    /// ```
    pub const fn is_skinned(self) -> bool {
        !matches!(self, Style::Stick)
    }
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One of the ten variants every skinned [`Style`] draws its own take of; what a player
/// picks in the menus is really one of these, worn with the style of their choice.
///
/// ```
/// use srr::game::Variant;
///
/// assert_eq!(Variant::ALL.len(), 10);
/// for variant in Variant::ALL {
///     assert_eq!(Variant::from_index(variant.index()), Some(variant));
/// }
/// assert_eq!(Variant::from_index(10), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// A graceful brunette woman.
    Brunette,
    /// A striking blonde woman.
    Blonde,
    /// A fierce red-haired woman.
    Redhead,
    /// An athletic dark-haired man.
    Ace,
    /// A stocky bald brawler.
    Boulder,
    /// A wiry spiky-haired youth.
    Spark,
    /// A silver-haired elder.
    Elder,
    /// A crew-cut soldier.
    Trooper,
    /// A green-mohawked punk.
    Punk,
    /// A masked mystic.
    Mystic,
}

impl Variant {
    /// Every variant, in the order of the menus.
    pub const ALL: [Variant; 10] = [
        Variant::Brunette,
        Variant::Blonde,
        Variant::Redhead,
        Variant::Ace,
        Variant::Boulder,
        Variant::Spark,
        Variant::Elder,
        Variant::Trooper,
        Variant::Punk,
        Variant::Mystic,
    ];

    /// Position in [`Variant::ALL`]: the byte that stands for the variant on the wire.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The variant at `index` in [`Variant::ALL`], if any.
    pub fn from_index(index: u8) -> Option<Variant> {
        Variant::ALL.get(usize::from(index)).copied()
    }

    /// The variant of `player`, so the ten players of a brawl each wear their own: they
    /// cycle in [`Variant::ALL`] order (like [`accessory`](crate::front::stick::accessory)
    /// does for the chalk ink).
    ///
    /// ```
    /// use srr::game::{PlayerId, Variant};
    ///
    /// assert_eq!(Variant::of(PlayerId(0)), Variant::Brunette);
    /// assert_eq!(Variant::of(PlayerId(10)), Variant::Brunette);
    /// assert_eq!(Variant::of(PlayerId(4)), Variant::Boulder);
    /// ```
    pub const fn of(player: crate::game::PlayerId) -> Variant {
        let index = (player.0 % 10) as usize;
        Variant::ALL[index]
    }

    /// The variant as the menus write it.
    ///
    /// ```
    /// use srr::game::Variant;
    ///
    /// assert_eq!(Variant::Brunette.label(), "the brunette");
    /// assert_eq!(Variant::Ace.label(), "the ace");
    /// ```
    pub const fn label(self) -> &'static str {
        match self {
            Variant::Brunette => "the brunette",
            Variant::Blonde => "the blonde",
            Variant::Redhead => "the redhead",
            Variant::Ace => "the ace",
            Variant::Boulder => "the boulder",
            Variant::Spark => "the spark",
            Variant::Elder => "the elder",
            Variant::Trooper => "the trooper",
            Variant::Punk => "the punk",
            Variant::Mystic => "the mystic",
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A skin: the [`Style`] a fighter wears, and (for the skinned styles) which of the ten
/// [`Variant`]s of that style it is. One byte each on the wire; [`Skin::DEFAULT`] is the
/// plain chalk.
///
/// ```
/// use srr::game::{PlayerId, Skin, Style, Variant};
///
/// // Player 3 picks the manga take of their own variant: the red-haired woman.
/// let skin = Skin::styled(Style::Manga, Variant::of(PlayerId(2)));
/// assert_eq!(skin.style, Style::Manga);
/// assert_eq!(skin.variant, Some(Variant::Redhead));
/// assert_eq!(Skin::from_index(skin.index()), Some(skin));
/// // The chalk is the default and wears no variant.
/// assert_eq!(Skin::default(), Skin::STICK);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Skin {
    /// The style worn.
    pub style: Style,
    /// The variant of the style, `None` for the chalk ink.
    pub variant: Option<Variant>,
}

impl Skin {
    /// The plain chalk figure.
    pub const STICK: Skin = Skin {
        style: Style::Stick,
        variant: None,
    };

    /// The skin of `style` wearing `variant`.
    pub const fn styled(style: Style, variant: Variant) -> Skin {
        Skin {
            style,
            variant: if style.is_skinned() {
                Some(variant)
            } else {
                None
            },
        }
    }

    /// The skin of `style` wearing `player`'s own variant (see [`Variant::of`]).
    pub const fn of_player(style: Style, player: crate::game::PlayerId) -> Skin {
        Skin::styled(style, Variant::of(player))
    }

    /// Position among the skins: the byte that stands for the skin on the wire — `0` for the
    /// chalk, then ten variants per skinned style, [`Style::Realistic`] first.
    pub const fn index(self) -> u8 {
        match self.variant {
            None => 0,
            Some(variant) => 1 + (self.style.index() - 1) * 10 + variant.index(),
        }
    }

    /// The skin at `index`, if any: `0` for the chalk, `1..=10` the realistic variants,
    /// `11..=20` the manga ones.
    ///
    /// ```
    /// use srr::game::{Skin, Style, Variant};
    ///
    /// for style in Style::ALL {
    ///     for variant in Variant::ALL {
    ///         let skin = Skin::styled(style, variant);
    ///         assert_eq!(Skin::from_index(skin.index()), Some(skin));
    ///     }
    /// }
    /// assert_eq!(Skin::from_index(0), Some(Skin::STICK));
    /// assert_eq!(Skin::from_index(21), None);
    /// ```
    pub fn from_index(index: u8) -> Option<Skin> {
        if index == 0 {
            return Some(Skin::STICK);
        }
        let variant = Variant::from_index((index - 1) % 10)?;
        let style = match (index - 1) / 10 {
            0 => Style::Realistic,
            1 => Style::Manga,
            _ => return None,
        };
        Some(Skin::styled(style, variant))
    }

    /// Every skin worn with `variant`: the chalk, then each skinned style's take of it.
    pub fn choices(variant: Variant) -> Vec<Skin> {
        std::iter::once(Skin::STICK)
            .chain(
                Style::ALL
                    .iter()
                    .filter(|style| style.is_skinned())
                    .map(|&style| Skin::styled(style, variant)),
            )
            .collect()
    }

    /// The skin as the menus write it: the style, and the variant between parentheses.
    ///
    /// ```
    /// use srr::game::{Skin, Style, Variant};
    ///
    /// assert_eq!(Skin::STICK.label(), "Stick (chalk)");
    /// assert_eq!(
    ///     Skin::styled(Style::Manga, Variant::Blonde).label(),
    ///     "Manga (the blonde)"
    /// );
    /// ```
    pub fn label(self) -> String {
        match self.variant {
            None => self.style.label().to_string(),
            Some(variant) => format!("{} ({})", self.style.label(), variant.label()),
        }
    }

    /// The skin of the next allowed style, worn with `player`'s own variant, wrapping around;
    /// `None` when `allowed` offers nothing (the chalk always is, but a lone chalk is no
    /// choice either — the menus then show no skins).
    ///
    /// ```
    /// use srr::game::{PlayerId, Skin, Style, StyleSet, Variant};
    ///
    /// let all = StyleSet::all();
    /// let redhead = Skin::styled(Style::Realistic, Variant::Redhead);
    /// assert_eq!(Skin::STICK.next_allowed(all, PlayerId(2)), Some(redhead));
    /// let manga_redhead = Skin::styled(Style::Manga, Variant::Redhead);
    /// assert_eq!(redhead.next_allowed(all, PlayerId(2)), Some(manga_redhead));
    /// assert_eq!(manga_redhead.next_allowed(all, PlayerId(2)), Some(Skin::STICK));
    /// // A set without any skinned style offers nothing.
    /// assert_eq!(Skin::STICK.next_allowed(StyleSet::none(), PlayerId(0)), None);
    /// ```
    pub fn next_allowed(self, allowed: StyleSet, player: crate::game::PlayerId) -> Option<Skin> {
        let next = self.style.next_allowed(allowed);
        (next != self.style).then(|| Skin::of_player(next, player))
    }

    /// The skin of the previous allowed style, worn with `player`'s own variant, wrapping
    /// around; `None` when `allowed` offers nothing.
    pub fn previous_allowed(
        self,
        allowed: StyleSet,
        player: crate::game::PlayerId,
    ) -> Option<Skin> {
        let previous = self.style.previous_allowed(allowed);
        (previous != self.style).then(|| Skin::of_player(previous, player))
    }
}

/// The bitmask of the [`Style`]s a server allows its players to wear
/// ([`ServerMessage::Styles`](crate::net::ServerMessage::Styles)); a bit per style, like
/// [`GlitchSet`](super::GlitchSet).
///
/// ```
/// use srr::game::{Style, StyleSet};
///
/// let set = StyleSet::default().with(Style::Manga);
/// assert!(set.contains(Style::Manga));
/// assert!(!set.contains(Style::Realistic));
/// assert_eq!(set.without(Style::Manga), StyleSet::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StyleSet(u8);

impl StyleSet {
    /// The set with only the chalk (the default: nobody wears a skin).
    pub const fn none() -> Self {
        StyleSet(0)
    }

    /// Every style in [`Style::ALL`].
    pub fn all() -> Self {
        Style::ALL.into_iter().fold(Self::none(), Self::with)
    }

    /// This set plus `style`.
    pub const fn with(self, style: Style) -> Self {
        StyleSet(self.0 | 1 << style.index())
    }

    /// This set minus `style`.
    pub const fn without(self, style: Style) -> Self {
        StyleSet(self.0 & !(1 << style.index()))
    }

    /// Whether `style` is in this set. The chalk is always allowed (it needs no server
    /// blessing; it is also what a skin outside the set falls back to).
    pub const fn contains(self, style: Style) -> bool {
        match style {
            Style::Stick => true,
            Style::Realistic => self.0 & (1 << Style::Realistic.index()) != 0,
            Style::Manga => self.0 & (1 << Style::Manga.index()) != 0,
        }
    }

    /// The bits of the set, one per [`Style::ALL`]: what goes on the wire.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The set of `bits`, ignoring the bits that name no style.
    pub const fn from_bits(bits: u8) -> Self {
        StyleSet(bits & ((1 << Style::ALL.len() as u8) - 1))
    }

    /// The styles of this set, in [`Style::ALL`] order, chalk first.
    pub fn styles(self) -> impl Iterator<Item = Style> {
        Style::ALL
            .into_iter()
            .filter(move |style| self.contains(*style))
    }
}

impl fmt::Display for StyleSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let styles: Vec<&str> = self.styles().map(Style::label).collect();
        f.write_str(&styles.join(", "))
    }
}
