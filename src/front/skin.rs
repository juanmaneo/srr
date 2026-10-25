//! The skins a fighter can wear: how a [`Figure`] is drawn beyond the chalk stick ink
//! (see [`stick`]). A [`SkinStyle`] is a family of looks — one implementation per
//! [`Style`](crate::game::Style) — and each wears one of the ten
//! [`Variant`](crate::game::Variant)s: three women (a brunette, a blonde and a red-haired
//! one), an athletic man, a stocky brawler, a wiry youth, an elder, a soldier, a punk and a
//! masked mystic, so the ten players of a brawl can all look different.
//!
//! The chalk ([`Chalk`](skin::Chalk)) is the default and draws nothing here: the stick ink
//! is the game's own. [`Realistic`] dresses the same skeleton in the full-colour look of a
//! modern fighting game: shaded volumes, faces, hair; [`Manga`] in the bright, bold
//! cel-shaded look of an anime: flat colours, thick ink, big eyes.
//!
//! Both work inside the pen the frame gives them ([`stroke`] and [`ring`], only
//! axis-aligned rectangles), so every engine and the headless tests draw them unchanged.
//! The 3D view reads the same palettes ([`Palette`]) for its mannequins.
//!
//! No style copies anyone's protected likeness: every face, haircut and outfit is the
//! game's own, only the broad look (comic-book realism, Saturday-morning cel shading) is
//! borrowed.

use super::scene::{Frame, Rgba};
use super::stick::{self, Bone, Figure, HEAD_RADIUS, Point, ring, stroke};
use crate::game::{Skin, Style, Variant};

/// A filled disc, as the frame can draw it (concentric [`ring`]s).
pub(crate) fn disc(frame: &mut Frame, center: Point, radius: f32, color: Rgba) {
    let mut r = radius;
    while r > 1.0 {
        ring(frame, center, r, r, color);
        r -= 1.4;
    }
    ring(frame, center, 0.6, 1.2, color);
}

/// A quadrilateral with corners `a b c d` (screen coordinates), filled in scanline runs.
pub(crate) fn quad(frame: &mut Frame, a: Point, b: Point, c: Point, d: Point, color: Rgba) {
    // Interpolate between the two edges (a-b and d-c) and stroke the runs.
    let steps = 14;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let lerp = |p: Point, q: Point, t: f32| Point {
            x: p.x + (q.x - p.x) * t,
            y: p.y + (q.y - p.y) * t,
        };
        let left0 = lerp(a, d, t0);
        let left1 = lerp(a, d, t1);
        let right0 = lerp(b, c, t0);
        let right1 = lerp(b, c, t1);
        stroke(frame, left0, right0, (left1.y - left0.y).abs() + 2.0, color);
        stroke(frame, left0, left1, 1.5, color);
        stroke(frame, right0, right1, 1.5, color);
    }
}

/// The colours a variant wears: what both views read.
///
/// ```
/// use srr::front::skin::Palette;
/// use srr::game::Variant;
///
/// let palette = Palette::of(Variant::Brunette);
/// assert!(palette.hair != palette.skin);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The hair (also the beard of the elder and the soldier's stubble).
    pub hair: Rgba,
    /// The skin of the face and the hands.
    pub skin: Rgba,
    /// The outfit (the torso, the legs).
    pub outfit: Rgba,
    /// A second outfit colour (the trims, the shoes, the top).
    pub trim: Rgba,
}

impl Palette {
    /// The palette of `variant` (the same for every style; the style decides how it is
    /// used).
    pub fn of(variant: Variant) -> Palette {
        let skin = Rgba::hex(0xf2c79a);
        match variant {
            Variant::Brunette => Palette {
                hair: Rgba::hex(0x5a3825),
                skin,
                outfit: Rgba::hex(0x8a2d4e),
                trim: Rgba::hex(0xf0e6d2),
            },
            Variant::Blonde => Palette {
                hair: Rgba::hex(0xe8c26a),
                skin,
                outfit: Rgba::hex(0x4b3a86),
                trim: Rgba::hex(0xd8d8e8),
            },
            Variant::Redhead => Palette {
                hair: Rgba::hex(0xc74a2a),
                skin,
                outfit: Rgba::hex(0x22242e),
                trim: Rgba::hex(0xc7a04a),
            },
            Variant::Ace => Palette {
                hair: Rgba::hex(0x27221f),
                skin,
                outfit: Rgba::hex(0x2b4a8c),
                trim: Rgba::hex(0xe8e8e8),
            },
            Variant::Boulder => Palette {
                hair: Rgba::hex(0x4a4a4a),
                skin: Rgba::hex(0xe0aa76),
                outfit: Rgba::hex(0x6a5030),
                trim: Rgba::hex(0xd8c8a8),
            },
            Variant::Spark => Palette {
                hair: Rgba::hex(0xd8862c),
                skin,
                outfit: Rgba::hex(0x2ea86a),
                trim: Rgba::hex(0xf4f4f4),
            },
            Variant::Elder => Palette {
                hair: Rgba::hex(0xd8d4d0),
                skin: Rgba::hex(0xe6b686),
                outfit: Rgba::hex(0x5c4a6a),
                trim: Rgba::hex(0xc8b048),
            },
            Variant::Trooper => Palette {
                hair: Rgba::hex(0x3d3630),
                skin,
                outfit: Rgba::hex(0x44523e),
                trim: Rgba::hex(0x2e362a),
            },
            Variant::Punk => Palette {
                hair: Rgba::hex(0x3fae5c),
                skin: Rgba::hex(0xf6d2a8),
                outfit: Rgba::hex(0x1e1e26),
                trim: Rgba::hex(0xc73a5a),
            },
            Variant::Mystic => Palette {
                hair: Rgba::hex(0x8a5adf),
                skin: Rgba::hex(0xdcb890),
                outfit: Rgba::hex(0x2a2a3e),
                trim: Rgba::hex(0x9a7ae0),
            },
        }
    }
}

/// How one [`Style`] dresses a [`Figure`]: the side-view ink, and the palette its variant
/// wears. The chalk needs none of this (see [`Style::is_skinned`]); a style that is not
/// in the server's set is never drawn either (see [`StyleSet`](crate::game::StyleSet)).
pub trait SkinStyle {
    /// The style this draws ([`Style::Stick`] for the chalk ink).
    fn style(&self) -> Style;

    /// How the fighter's head is drawn over the skeleton: the face, the hair, whatever the
    /// style does with `palette` around `head` looking `facing`, scaled by `scale` (the
    /// chalk draws nothing: the stick ink already has its head).
    fn head(&self, frame: &mut Frame, head: Point, facing: f32, palette: &Palette, scale: f32);

    /// How the torso between `neck` and `hip` is dressed (the chalk draws its single line).
    fn torso(&self, frame: &mut Frame, neck: Point, hip: Point, palette: &Palette, scale: f32);

    /// The width and colour of a limb: `(width, color)` for `palette`, or `None` to let the
    /// stick ink draw the limb (front limbs are passed `front = true`).
    fn limb(&self, palette: &Palette, bone: &Bone, front: bool) -> Option<(f32, Rgba)>;
}

/// The chalk ink itself: draws nothing over the stick figure.
pub struct Chalk;

impl SkinStyle for Chalk {
    fn style(&self) -> Style {
        Style::Stick
    }

    fn head(&self, _: &mut Frame, _: Point, _: f32, _: &Palette, _: f32) {}

    fn torso(&self, _: &mut Frame, _: Point, _: Point, _: &Palette, _: f32) {}

    fn limb(&self, _: &Palette, _: &Bone, _: bool) -> Option<(f32, Rgba)> {
        None
    }
}

/// The full-colour look of a modern fighting game: shaded volumes, faces, hair.
pub struct Realistic;

impl SkinStyle for Realistic {
    fn style(&self) -> Style {
        Style::Realistic
    }

    fn head(&self, frame: &mut Frame, head: Point, facing: f32, palette: &Palette, scale: f32) {
        let r = HEAD_RADIUS * scale;
        let at = |along: f32, ahead: f32| Point {
            x: head.x + facing * ahead,
            y: head.y - along,
        };
        // The skull, the jaw, then the hair over them, the face last.
        disc(frame, head, r, palette.skin);
        disc(frame, at(-r * 0.4, 0.0), r * 0.8, palette.skin);
        // The eyes and the mouth, on the facing side.
        let eye = at(-1.0, facing * r * 0.35);
        disc(frame, eye, 1.4 * scale, Rgba::hex(0x241a12));
        let mouth = at(-r * 0.5, facing * r * 0.4);
        stroke(
            frame,
            mouth,
            at(-r * 0.5, facing * r * 0.75),
            1.6 * scale,
            Rgba::hex(0x8a4a3a),
        );
        // The hair: a cap on top, longer behind for the women and the elder.
        let back = at(r * 0.2, -facing * r);
        disc(frame, at(r * 0.35, 0.0), r * 0.8, palette.hair);
        disc(frame, back, r * 0.55, palette.hair);
        stroke(
            frame,
            back,
            at(-r * 0.9, -facing * r * 1.2),
            r * 0.5,
            palette.hair,
        );
        // The brow.
        stroke(
            frame,
            at(2.5 * scale, facing * r * 0.15),
            at(2.5 * scale, facing * r * 0.6),
            1.8 * scale,
            palette.hair,
        );
    }

    fn torso(&self, frame: &mut Frame, neck: Point, hip: Point, palette: &Palette, scale: f32) {
        let width = 9.0 * scale;
        let across = |point: Point, w: f32| {
            (
                Point {
                    x: point.x - w,
                    y: point.y,
                },
                Point {
                    x: point.x + w,
                    y: point.y,
                },
            )
        };
        let (nl, nr) = across(neck, width * 0.6);
        let (hl, hr) = across(hip, width);
        quad(frame, nl, nr, hr, hl, palette.outfit);
        // The trims at the waist.
        stroke(
            frame,
            hl,
            Point {
                x: hl.x,
                y: hl.y - 2.0 * scale,
            },
            2.0 * scale,
            palette.trim,
        );
        stroke(
            frame,
            hr,
            Point {
                x: hr.x,
                y: hr.y - 2.0 * scale,
            },
            2.0 * scale,
            palette.trim,
        );
    }

    fn limb(&self, palette: &Palette, _bone: &Bone, front: bool) -> Option<(f32, Rgba)> {
        let shade = if front { 1.0 } else { 0.8 };
        Some((7.0, palette.outfit.scaled(shade)))
    }
}

/// The bright cel-shaded look of an anime: flat colours, thick ink, big eyes.
pub struct Manga;

impl SkinStyle for Manga {
    fn style(&self) -> Style {
        Style::Manga
    }

    fn head(&self, frame: &mut Frame, head: Point, facing: f32, palette: &Palette, scale: f32) {
        let r = HEAD_RADIUS * scale;
        let at = |along: f32, ahead: f32| Point {
            x: head.x + facing * ahead,
            y: head.y - along,
        };
        // A rounder face, a huge eye, the hair in bold flat shapes.
        disc(frame, head, r, palette.skin);
        let eye = at(0.0, facing * r * 0.3);
        disc(frame, eye, 3.2 * scale, Rgba::hex(0xffffff));
        disc(frame, eye, 2.0 * scale, Rgba::hex(0x2a2330));
        disc(
            frame,
            at(0.0 + 0.4, facing * r * 0.3 + 0.6),
            0.8 * scale,
            Rgba::hex(0xffffff),
        );
        // The mouth, a simple line.
        let mouth = at(-r * 0.45, facing * r * 0.35);
        stroke(
            frame,
            mouth,
            at(-r * 0.45, facing * r * 0.6),
            1.4 * scale,
            Rgba::hex(0x9a4a4a),
        );
        // The hair: big spikes on top, a sweep behind.
        let top = at(r * 0.9, 0.0);
        disc(frame, top, r * 0.85, palette.hair);
        for spike in 0..3 {
            let ahead = facing * r * (0.5 - spike as f32 * 0.5);
            stroke(
                frame,
                at(r * 0.7, ahead),
                at(r * 1.6, ahead),
                r * 0.4,
                palette.hair,
            );
        }
        stroke(
            frame,
            at(0.0, -facing * r),
            at(-r * 0.8, -facing * r * 1.1),
            r * 0.7,
            palette.hair,
        );
    }

    fn torso(&self, frame: &mut Frame, neck: Point, hip: Point, palette: &Palette, scale: f32) {
        let width = 10.0 * scale;
        let across = |point: Point, w: f32| {
            (
                Point {
                    x: point.x - w,
                    y: point.y,
                },
                Point {
                    x: point.x + w,
                    y: point.y,
                },
            )
        };
        let (nl, nr) = across(neck, width * 0.55);
        let (hl, hr) = across(hip, width);
        quad(frame, nl, nr, hr, hl, palette.outfit);
        // A bold trim band, straight across.
        let band = hip.y - 6.0 * scale;
        stroke(
            frame,
            Point { x: hl.x, y: band },
            Point { x: hr.x, y: band },
            3.5 * scale,
            palette.trim,
        );
    }

    fn limb(&self, palette: &Palette, _bone: &Bone, front: bool) -> Option<(f32, Rgba)> {
        let shade = if front { 1.0 } else { 0.72 };
        Some((8.5, palette.outfit.scaled(shade)))
    }
}

/// The [`SkinStyle`] of a [`Skin`]: the chalk (drawing nothing), or the style the skin
/// wears, shared. Only the styles a server allows reach the menus; unknown styles are
/// never asked for.
///
/// ```
/// use srr::front::skin::{Chalk, Manga, Realistic, SkinStyle, skinner};
/// use srr::game::{Skin, Style, Variant};
///
/// assert_eq!(skinner(&Skin::STICK).style(), Style::Stick);
/// assert_eq!(skinner(&Skin::styled(Style::Manga, Variant::Ace)).style(), Style::Manga);
/// ```
pub fn skinner(skin: &Skin) -> &'static dyn SkinStyle {
    match skin.style {
        Style::Stick => &Chalk,
        Style::Realistic => &Realistic,
        Style::Manga => &Manga,
    }
}

/// The palette `skin` wears: the variant's own, or the plain ink for the chalk.
pub fn palette_of(skin: &Skin) -> Palette {
    skin.variant.map_or_else(
        || Palette {
            hair: stick::INK,
            skin: stick::INK,
            outfit: stick::INK,
            trim: stick::INK,
        },
        Palette::of,
    )
}

/// Draws `figure` in the side view, dressed by the [`SkinStyle`] of `skin`: the chalk draws
/// the plain stick ink; a skinned style draws the dressed torso, the thick limbs, then the
/// head with its face and hair over them. The skins layer *over* the ink, never replace
/// it: the game keeps its poses, its marks and its effects, and the chalk outline stays
/// under the volumes so a skin never hides the state of the fight (the stars of a stun,
/// the cross of a knockout, the striking limb in the strength's colour).
///
/// ```
/// use srr::front::skin::draw_inked;
/// use srr::game::{MatchConfig, Skin, Style, Variant};
///
/// // The chalk keeps its plain stick ink; a skinned figure is dressed by its style.
/// assert!(!Skin::styled(Style::Manga, Variant::Brunette).style.is_skinned() == false);
/// assert!(Skin::STICK.style.is_skinned() == false);
/// ```
pub fn draw_inked(frame: &mut Frame, figure: &Figure, skin: &Skin) {
    if !skin.style.is_skinned() {
        figure.draw(frame);
        return;
    }
    let style = skinner(skin);
    let palette = palette_of(skin);
    let scale = 1.0;
    // The limbs, thick and shaded, then the torso between them, the head last so the
    // hair and the face land over everything.
    let draw_limb = |frame: &mut Frame, from: Point, bone: &Bone, front: bool| {
        if let Some((width, color)) = style.limb(&palette, bone, front) {
            stroke(frame, from, bone.joint, width, color);
            stroke(frame, bone.joint, bone.end, width, color);
            // A hand or a foot, in the skin's colour.
            if front {
                disc(frame, bone.end, width * 0.45, palette.skin);
            }
        }
    };
    draw_limb(frame, figure.hip, &figure.legs[1], false);
    draw_limb(frame, figure.shoulder, &figure.arms[1], false);
    style.torso(frame, figure.neck, figure.hip, &palette, scale);
    draw_limb(frame, figure.hip, &figure.legs[0], true);
    style.head(frame, figure.head, figure.facing * scale, &palette, scale);
    draw_limb(frame, figure.shoulder, &figure.arms[0], true);
    // The chalk outline over the volumes, so the pose stays readable whatever the skin:
    // the head ring, then the marks (a skin never hides the state of the fighter).
    ring(frame, figure.head, HEAD_RADIUS, 1.6, stick::INK.scaled(0.9));
    figure.draw_marks_only(frame);
}
