//! The manga skin: **its own rendering engine**, not a dressing of the chalk stick. The
//! stick figures ([`stick`]) are the game's debug view — the joint-and-line skeleton — and
//! they stay exactly as they are; a manga fighter never draws them. Instead the same posed
//! [`Figure`] skeleton drives a manga sprite: manga proportions (a big head, a small body,
//! thin limbs), cel-shaded surfaces (a base tone, one hard shadow band, one highlight),
//! clean 1-pixel black outlines, huge glossy eyes, spiky hair masses that extend beyond the
//! head, and per-variant signature looks (twintails, a tied braid, flowing mane; dresses,
//! thigh-high stockings for the women, baggy pants and bare arms for the men) — the look of
//! a manga-themed brawler, drawn sprite by sprite.
//!
//! Everything works inside the pen the frame gives ([`stroke`] and [`ring`], only
//! axis-aligned rectangles), so every engine and the headless tests draw it unchanged.
//! The 3D view reads the same palettes ([`Palette`]) for its mannequins.
//!
//! No character copies anyone's protected likeness: every face, haircut and outfit is the
//! game's own; only the broad manga rendering techniques (cel shading, big glossy eyes,
//! clean ink outlines) are borrowed.

use super::scene::{Frame, Rgba};
use super::stick::{Bone, Figure, HEAD_RADIUS, Point, ring, stroke};
use crate::game::{Skin, Variant};

/// The clean outline ink of the manga sprites: pure black, thin.
const INK: Rgba = Rgba::hex(0x100c14);
/// How thick the sprite outlines are.
const OUTLINE: f32 = 1.4;

/// A filled disc, as the frame can draw it (concentric [`ring`]s).
pub(crate) fn disc(frame: &mut Frame, center: Point, radius: f32, color: Rgba) {
    let mut r = radius;
    while r > 1.0 {
        ring(frame, center, r, r, color);
        r -= 1.4;
    }
    ring(frame, center, 0.6, 1.2, color);
}

/// The `t` point between `a` and `b`.
fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

/// A filled leaf shape between `a` and `b` (both `wa` and `wb` wide): the sprite's hair
/// clumps and flowing cloth are leaves — two arcs meeting at the tip.
fn leaf(frame: &mut Frame, a: Point, b: Point, wa: f32, wb: f32, color: Rgba) {
    let span = Point {
        x: b.x - a.x,
        y: b.y - a.y,
    };
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = Point {
        x: -span.y / length,
        y: span.x / length,
    };
    // Two curved edges: bulge out on one side, in on the other, then mirror.
    for side in [1.0_f32, -1.0] {
        for i in 0..8 {
            let t0 = i as f32 / 8.0;
            let t1 = (i + 1) as f32 / 8.0;
            let bulge = |t: f32| {
                // A quarter-circle bulge that peaks in the middle of the leaf.
                let w = (wa + (wb - wa) * t) / 2.0;
                let curve = (1.0 - (2.0 * t - 1.0).powi(2)).sqrt();
                w * (1.0 + side * curve * 0.6)
            };
            let p0 = Point {
                x: a.x + span.x * t0 + across.x * bulge(t0),
                y: a.y + span.y * t0 + across.y * bulge(t0),
            };
            let p1 = Point {
                x: a.x + span.x * t1 + across.x * bulge(t1),
                y: a.y + span.y * t1 + across.y * bulge(t1),
            };
            stroke(frame, p0, p1, 2.2, color);
        }
    }
    // Fill the middle.
    for i in 0..6 {
        let t = i as f32 / 6.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        let p = lerp(a, b, t);
        disc(frame, p, w * 0.9, color);
    }
}

/// A filled quadrilateral with corners `a b c d`.
pub(crate) fn quad(frame: &mut Frame, a: Point, b: Point, c: Point, d: Point, color: Rgba) {
    let steps = 14;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let l0 = lerp(a, d, t0);
        let l1 = lerp(a, d, t1);
        let r0 = lerp(b, c, t0);
        let r1 = lerp(b, c, t1);
        stroke(frame, l0, r0, (l1.y - l0.y).abs() + 2.0, color);
        stroke(frame, l0, l1, 1.5, color);
        stroke(frame, r0, r1, 1.5, color);
    }
}

// -----------------------------------------------------------------------------------------
// The palettes
// -----------------------------------------------------------------------------------------

/// The colours and the look a variant wears: what both views read.
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
    /// The hair.
    pub hair: Rgba,
    /// The shadow tone of the hair.
    pub hair_shadow: Rgba,
    /// The skin of the face and the hands.
    pub skin: Rgba,
    /// The shadow tone of the skin.
    pub skin_shadow: Rgba,
    /// The outfit (the torso, the legs — or the stockings under a dress).
    pub outfit: Rgba,
    /// The shadow tone of the outfit.
    pub outfit_shadow: Rgba,
    /// A second outfit colour: the trims, the shoes, the top.
    pub trim: Rgba,
    /// The eyes.
    pub eye: Rgba,
    /// The shape of the body and the hair, per variant.
    pub look: Look,
}

/// The body and hair proportions of a variant: how the manga sprite differs from the posed
/// skeleton it replaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Half the width of the shoulders.
    pub shoulders: f32,
    /// Half the width of the waist.
    pub waist: f32,
    /// Half the width of the hips (a skirt flares wider).
    pub hips: f32,
    /// Half the width of an arm.
    pub arm: f32,
    /// Half the width of a leg (a stocking-covered leg is thin).
    pub leg: f32,
    /// The length of the hair behind the head, in head radii (0: short).
    pub hair_length: f32,
    /// The hair silhouette: spikes, twintails, a mane.
    pub hair_shape: HairShape,
    /// Whether the build is a woman's (the hourglass silhouette, the stockings, the skirt).
    pub feminine: bool,
}

/// The silhouette of the hair, drawn per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HairShape {
    /// Twintails: two masses flying out from the sides of the head.
    Twintails,
    /// A long tied braid down the back.
    Braid,
    /// A flowing mane down the back.
    Mane,
    /// Short spiky hair.
    Spikes,
    /// A bald head (a shine band instead of hair).
    Bald,
    /// A mohawk.
    Mohawk,
}

impl Look {
    /// The look of a manga heroine: an hourglass body, thin limbs, long hair.
    const WOMAN: Look = Look {
        shoulders: 8.0,
        waist: 5.0,
        hips: 9.0,
        arm: 2.6,
        leg: 3.0,
        hair_length: 2.8,
        hair_shape: HairShape::Mane,
        feminine: true,
    };

    /// The look of a manga hero: broader shoulders, thicker limbs, short hair.
    const MAN: Look = Look {
        shoulders: 9.0,
        waist: 6.0,
        hips: 6.5,
        arm: 3.2,
        leg: 3.4,
        hair_length: 0.9,
        hair_shape: HairShape::Spikes,
        feminine: false,
    };
}

impl Palette {
    /// The palette of `variant`.
    pub fn of(variant: Variant) -> Palette {
        let skin = Rgba::hex(0xffe0c2);
        let shadow = |color: Rgba| color.scaled(0.62);
        let palette = |hair, skin, outfit, trim, eye, look| Palette {
            hair,
            hair_shadow: shadow(hair),
            skin,
            skin_shadow: shadow(skin),
            outfit,
            outfit_shadow: shadow(outfit),
            trim,
            eye,
            look,
        };
        match variant {
            Variant::Brunette => palette(
                Rgba::hex(0x7a4a28),
                skin,
                Rgba::hex(0xd83860),
                Rgba::hex(0xfff0e2),
                Rgba::hex(0x3a9e5c),
                Look {
                    hair_shape: HairShape::Twintails,
                    ..Look::WOMAN
                },
            ),
            Variant::Blonde => palette(
                Rgba::hex(0xf7d968),
                skin,
                Rgba::hex(0x4a42b8),
                Rgba::hex(0xf0f0ff),
                Rgba::hex(0x2e7ed0),
                Look {
                    hair_shape: HairShape::Braid,
                    ..Look::WOMAN
                },
            ),
            Variant::Redhead => palette(
                Rgba::hex(0xe85830),
                skin,
                Rgba::hex(0x3a3448),
                Rgba::hex(0xffd24a),
                Rgba::hex(0x44b03c),
                Look {
                    hair_shape: HairShape::Mane,
                    ..Look::WOMAN
                },
            ),
            Variant::Ace => palette(
                Rgba::hex(0x2a2422),
                skin,
                Rgba::hex(0x2e52a0),
                Rgba::hex(0xf0f0f0),
                Rgba::hex(0x282834),
                Look::MAN,
            ),
            Variant::Boulder => palette(
                Rgba::hex(0x4a4a4a),
                Rgba::hex(0xeab27e),
                Rgba::hex(0x7a5a34),
                Rgba::hex(0xe8d8b4),
                Rgba::hex(0x3a2a1a),
                Look {
                    shoulders: 11.5,
                    waist: 8.0,
                    hips: 8.0,
                    arm: 4.0,
                    leg: 4.0,
                    hair_shape: HairShape::Bald,
                    ..Look::MAN
                },
            ),
            Variant::Spark => palette(
                Rgba::hex(0xf0982c),
                skin,
                Rgba::hex(0x38b872),
                Rgba::hex(0xffffff),
                Rgba::hex(0x3aa84c),
                Look {
                    shoulders: 7.0,
                    waist: 5.0,
                    hips: 5.5,
                    arm: 2.6,
                    leg: 2.8,
                    hair_length: 1.3,
                    ..Look::MAN
                },
            ),
            Variant::Elder => palette(
                Rgba::hex(0xdcd8d4),
                Rgba::hex(0xf0c090),
                Rgba::hex(0x6a5678),
                Rgba::hex(0xd0b84c),
                Rgba::hex(0x4a4a56),
                Look {
                    hair_length: 1.6,
                    ..Look::MAN
                },
            ),
            Variant::Trooper => palette(
                Rgba::hex(0x3e3832),
                skin,
                Rgba::hex(0x4c5a42),
                Rgba::hex(0x30382c),
                Rgba::hex(0x3a4a2e),
                Look::MAN,
            ),
            Variant::Punk => palette(
                Rgba::hex(0x3ab860),
                Rgba::hex(0xf8d8b0),
                Rgba::hex(0x24242e),
                Rgba::hex(0xd03a5a),
                Rgba::hex(0x2a6a3a),
                Look {
                    hair_shape: HairShape::Mohawk,
                    hair_length: 0.0,
                    ..Look::MAN
                },
            ),
            Variant::Mystic => palette(
                Rgba::hex(0x9060e8),
                Rgba::hex(0xe0bca0),
                Rgba::hex(0x30304a),
                Rgba::hex(0xa88cf0),
                Rgba::hex(0x6a3ad0),
                Look {
                    hair_length: 2.0,
                    ..Look::MAN
                },
            ),
        }
    }
}

// -----------------------------------------------------------------------------------------
// The sprite parts
// -----------------------------------------------------------------------------------------

/// A cel surface: the base fill, one hard shadow band on the lower third (away from the
/// light), one highlight stroke on the lit side — the three tones of a manga cel.
fn cel(frame: &mut Frame, a: Point, b: Point, wa: f32, wb: f32, color: Rgba, shadow: Rgba) {
    let span = Point {
        x: b.x - a.x,
        y: b.y - a.y,
    };
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = Point {
        x: -span.y / length,
        y: span.x / length,
    };
    // The base fill.
    for i in 0..8 {
        let t = i as f32 / 8.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        let p = lerp(a, b, t);
        stroke(
            frame,
            Point {
                x: p.x + across.x * w,
                y: p.y + across.y * w,
            },
            Point {
                x: p.x - across.x * w,
                y: p.y - across.y * w,
            },
            2.4,
            color,
        );
    }
    // The hard shadow: the third of the surface away from the light (up and left).
    let dark_side = if across.x + across.y > 0.0 { -1.0 } else { 1.0 };
    for i in 0..8 {
        let t = i as f32 / 8.0;
        let w = (wa + (wb - wa) * t) / 2.0;
        let p = lerp(a, b, t);
        // The shadow band, from the dark edge to a third of the way in.
        let edge = Point {
            x: p.x + across.x * w * dark_side,
            y: p.y + across.y * w * dark_side,
        };
        let inner = Point {
            x: p.x + across.x * w * dark_side / 3.0,
            y: p.y + across.y * w * dark_side / 3.0,
        };
        stroke(frame, edge, inner, 2.4, shadow);
    }
    // The highlight: a thin stroke along the lit edge.
    let lit = Point {
        x: -across.x * dark_side,
        y: -across.y * dark_side,
    };
    stroke(
        frame,
        Point {
            x: a.x + lit.x * wa * 0.32,
            y: a.y + lit.y * wa * 0.32,
        },
        Point {
            x: b.x + lit.x * wb * 0.32,
            y: b.y + lit.y * wb * 0.32,
        },
        1.8,
        color.mix(Rgba::hex(0xffffff), 0.28),
    );
}

/// The outline of a limb segment.
fn outline_segment(frame: &mut Frame, a: Point, b: Point, w: f32) {
    let span = Point {
        x: b.x - a.x,
        y: b.y - a.y,
    };
    let length = span.x.hypot(span.y).max(f32::EPSILON);
    let across = Point {
        x: -span.y / length,
        y: span.x / length,
    };
    stroke(
        frame,
        Point {
            x: a.x + across.x * w / 2.0,
            y: a.y + across.y * w / 2.0,
        },
        Point {
            x: b.x + across.x * w / 2.0,
            y: b.y + across.y * w / 2.0,
        },
        OUTLINE,
        INK,
    );
    stroke(
        frame,
        Point {
            x: a.x - across.x * w / 2.0,
            y: a.y - across.y * w / 2.0,
        },
        Point {
            x: b.x - across.x * w / 2.0,
            y: b.y - across.y * w / 2.0,
        },
        OUTLINE,
        INK,
    );
}

/// The huge glossy manga eye: a white almond with a bold outline, a vivid iris with a dark
/// gradient band at its top, a big sparkle highlight and a small one.
fn eye(frame: &mut Frame, center: Point, radius: f32, palette: &Palette, facing: f32) {
    // The white, then the iris.
    disc(frame, center, radius, Rgba::hex(0xffffff));
    disc(frame, center, radius * 0.72, palette.eye);
    // The dark gradient band at the top of the iris.
    disc(
        frame,
        Point {
            x: center.x,
            y: center.y - radius * 0.28,
        },
        radius * 0.72,
        palette.eye.scaled(0.4),
    );
    // The pupil.
    disc(
        frame,
        Point {
            x: center.x + facing * radius * 0.08,
            y: center.y + radius * 0.1,
        },
        radius * 0.3,
        INK,
    );
    // The two sparkle highlights.
    disc(
        frame,
        Point {
            x: center.x + facing * radius * 0.2,
            y: center.y + radius * 0.45,
        },
        radius * 0.24,
        Rgba::hex(0xffffff),
    );
    disc(
        frame,
        Point {
            x: center.x - facing * radius * 0.3,
            y: center.y - radius * 0.3,
        },
        radius * 0.13,
        Rgba::hex(0xffffff),
    );
    // The bold clean outline.
    ring(frame, center, radius, OUTLINE, INK);
    // The lash line, thicker above.
    stroke(
        frame,
        Point {
            x: center.x - facing * radius * 0.9,
            y: center.y - radius * 0.85,
        },
        Point {
            x: center.x + facing * radius * 0.9,
            y: center.y - radius * 0.95,
        },
        OUTLINE * 1.6,
        INK,
    );
}

/// The manga head: a round face with a small chin, one huge eye (the other is hidden by
/// the profile), a small mouth, a blush, and the hair silhouette of the variant.
fn head(frame: &mut Frame, head: Point, facing: f32, palette: &Palette, radius: f32) {
    let at = |along: f32, ahead: f32| Point {
        x: head.x + facing * ahead,
        y: head.y - along,
    };
    let r = radius;
    // The face: a round skull, then the small chin.
    disc(frame, head, r, palette.skin);
    disc(frame, at(-r * 0.5, r * 0.05), r * 0.68, palette.skin);
    disc(frame, at(-r * 0.8, facing * r * 0.1), r * 0.4, palette.skin);
    // The cel shadow on the lower part of the face.
    disc(
        frame,
        at(-r * 0.5, -facing * r * 0.25),
        r * 0.62,
        palette.skin_shadow,
    );
    disc(
        frame,
        at(-r * 0.5, facing * r * 0.05),
        r * 0.6,
        palette.skin,
    );
    // The huge eye, on the facing side: half the head's height.
    eye(frame, at(-r * 0.05, r * 0.3), r * 0.42, palette, facing);
    // The brow, a thin arc above the eye.
    stroke(
        frame,
        at(r * 0.35, r * 0.1),
        at(r * 0.4, r * 0.6),
        1.4,
        palette.hair_shadow,
    );
    // The small mouth, a gentle line.
    stroke(
        frame,
        at(-r * 0.55, r * 0.3),
        at(-r * 0.55, r * 0.55),
        1.4,
        Rgba::hex(0xc05a5a),
    );
    // The blush, under the eye.
    disc(
        frame,
        at(-r * 0.28, r * 0.05),
        r * 0.14,
        Rgba {
            r: 1.0,
            g: 0.5,
            b: 0.5,
            a: 0.45,
        },
    );
    // The clean outline of the face.
    ring(frame, head, r, OUTLINE, INK);

    // The hair.
    hair(frame, head, facing, palette, r);
}

/// The hair silhouette of the variant, drawn as bold leaf clumps with a shine band.
fn hair(frame: &mut Frame, head: Point, facing: f32, palette: &Palette, r: f32) {
    let at = |along: f32, ahead: f32| Point {
        x: head.x + facing * ahead,
        y: head.y - along,
    };
    let hair = palette.hair;
    let dark = palette.hair_shadow;
    let shine = hair.mix(Rgba::hex(0xffffff), 0.35);
    let look = &palette.look;
    // The cap of the head, one bold mass.
    disc(frame, at(r * 0.3, -facing * r * 0.1), r * 0.95, hair);
    disc(frame, at(r * 0.55, -facing * r * 0.35), r * 0.7, hair);
    // The fringe: three sharp clumps over the forehead.
    for (y0, x0, y1, x1, w) in [
        (r * 0.7, r * 0.3, r * 0.35, r * 1.05, r * 0.42),
        (r * 0.6, -r * 0.1, r * 0.1, r * 1.1, r * 0.36),
        (r * 0.5, -r * 0.5, -r * 0.2, r * 1.0, r * 0.3),
    ] {
        leaf(frame, at(y0, x0), at(y1, x1), w, w * 0.5, hair);
    }
    // The shadow tone under the fringe, on the forehead.
    stroke(
        frame,
        at(r * 0.3, r * 0.55),
        at(r * 0.25, r * 0.95),
        r * 0.16,
        dark,
    );
    // The shine band across the crown, the manga signature.
    streak_shine(
        frame,
        at(r * 0.85, -r * 0.3),
        at(r * 0.9, r * 0.35),
        r * 0.18,
        shine,
    );
    // The silhouette per variant.
    match look.hair_shape {
        HairShape::Twintails => {
            // Two masses flying out from the sides, up and behind.
            for side in [1.0, -1.0] {
                let anchor = at(r * 0.4, side * r * 0.9);
                let tip = Point {
                    x: anchor.x - facing * r * 0.4,
                    y: anchor.y - r * 1.1,
                };
                leaf(frame, anchor, tip, r * 0.55, r * 0.3, hair);
                let flow = Point {
                    x: anchor.x - facing * r * 1.2,
                    y: anchor.y + r * 2.2,
                };
                leaf(frame, anchor, flow, r * 0.6, r * 0.35, hair);
            }
            // The back mass.
            disc(frame, at(r * 0.1, -facing * r * 0.8), r * 0.6, dark);
        }
        HairShape::Braid => {
            // A long braid down the back, drawn as overlapping leaves.
            let mut p = at(r * 0.3, -facing * r * 0.9);
            for i in 0..4 {
                let next = Point {
                    x: p.x - facing * r * 0.25,
                    y: p.y + r * 0.6,
                };
                leaf(
                    frame,
                    p,
                    next,
                    r * 0.3 - i as f32 * 0.04,
                    r * 0.26,
                    if i % 2 == 0 { hair } else { dark },
                );
                p = next;
            }
        }
        HairShape::Mane => {
            // A flowing mane down the back, two big leaves.
            let back = at(r * 0.2, -facing * r * 0.85);
            leaf(
                frame,
                back,
                Point {
                    x: back.x - facing * r * 0.5,
                    y: back.y + r * look.hair_length,
                },
                r * 0.6,
                r * 0.3,
                hair,
            );
            leaf(
                frame,
                at(r * -0.1, -facing * r * 0.7),
                Point {
                    x: back.x - facing * r * 0.9,
                    y: back.y + r * look.hair_length * 0.8,
                },
                r * 0.45,
                r * 0.22,
                dark,
            );
        }
        HairShape::Spikes => {
            // Sharp spikes standing up from the cap.
            for (ahead, w) in [(r * 0.5, r * 0.3), (0.0, r * 0.34), (-r * 0.5, r * 0.28)] {
                stroke(frame, at(r * 0.8, ahead), at(r * 1.7, ahead * 1.3), w, hair);
            }
        }
        HairShape::Bald => {
            // The shine band on the scalp itself.
            streak_shine(
                frame,
                at(r * 0.9, -r * 0.25),
                at(r * 0.95, r * 0.25),
                r * 0.14,
                palette.skin.mix(Rgba::hex(0xffffff), 0.3),
            );
        }
        HairShape::Mohawk => {
            // A crest of leaves standing along the top.
            for ahead in [-r * 0.3, 0.0, r * 0.3] {
                leaf(
                    frame,
                    at(r * 0.9, ahead),
                    Point {
                        x: at(r * 0.9, ahead).x,
                        y: at(r * 0.9, ahead).y - r * 0.9,
                    },
                    r * 0.22,
                    r * 0.12,
                    hair,
                );
            }
        }
    }
    // The crown spikes on top of the cap, for the shapes that have them.
    if matches!(
        look.hair_shape,
        HairShape::Twintails | HairShape::Braid | HairShape::Mane
    ) {
        for ahead in [-r * 0.4, 0.0, r * 0.4] {
            stroke(
                frame,
                at(r * 0.85, ahead),
                at(r * 1.5, ahead * 1.4),
                r * 0.26,
                hair,
            );
        }
    }
}

/// A shine stroke: a bright band over the hair, the manga highlight.
fn streak_shine(frame: &mut Frame, a: Point, b: Point, width: f32, shine: Rgba) {
    stroke(frame, a, b, width, shine);
}

// -----------------------------------------------------------------------------------------
// The sprite
// -----------------------------------------------------------------------------------------

/// The manga sprite: **its own rendering** of the posed skeleton. The chalk ink is never
/// drawn under it. Proportions follow the manga look: the head is drawn big relative to
/// the body, the limbs thin, and every surface is a cel (base, shadow band, highlight)
/// with a clean 1-pixel outline.
///
/// ```
/// use srr::game::{Skin, Style, Variant};
///
/// assert!(!Skin::STICK.style.is_skinned());
/// assert!(Skin::styled(Style::Manga, Variant::Brunette).style.is_skinned());
/// ```
pub fn draw_inked(frame: &mut Frame, figure: &Figure, skin: &Skin) {
    if !skin.style.is_skinned() {
        figure.draw(frame);
        return;
    }
    let palette = palette_of(skin);
    let look = &palette.look;
    let facing = figure.facing;

    // The manga proportions: the head drawn at 1.5x the skeleton's, the body kept slender.
    let head_r = HEAD_RADIUS * 1.5;
    // The limbs are thin: the sprite's own widths.
    let arm_w = look.arm * 2.0;
    let leg_w = look.leg * 2.0;

    // A cel limb: the two segments, each a cel surface with its outline; a glove or a
    // stocking edge at the joint; the fist or the shoe at the end.
    let limb = |frame: &mut Frame, from: Point, bone: &Bone, front: bool, arm: bool| {
        let (base, shadow, w) = if arm {
            (palette.skin, palette.skin_shadow, arm_w)
        } else {
            (palette.trim, palette.trim.scaled(0.62), leg_w)
        };
        // The women's arms are bare; the men's are in sleeves.
        let (upper, upper_shadow) = if arm && !look.feminine {
            (palette.outfit, palette.outfit_shadow)
        } else {
            (base, shadow)
        };
        let shade = if front { 1.0 } else { 0.85 };
        cel(
            frame,
            from,
            bone.joint,
            w,
            w * 0.9,
            upper.scaled(shade),
            upper_shadow.scaled(shade),
        );
        outline_segment(frame, from, bone.joint, w);
        cel(
            frame,
            bone.joint,
            bone.end,
            w * 0.9,
            w * 0.8,
            base.scaled(shade),
            shadow.scaled(shade),
        );
        outline_segment(frame, bone.joint, bone.end, w * 0.9);
        // The end: a fist in a glove for the arms, a shoe for the legs.
        if arm {
            disc(frame, bone.end, w * 0.62, base.scaled(shade));
            ring(frame, bone.end, w * 0.62, OUTLINE, INK);
        } else {
            // The shoe: a flat disc ahead of the ankle, with its outline and a sole.
            let toe = Point {
                x: bone.end.x + facing * w * 0.55,
                y: bone.end.y + w * 0.08,
            };
            disc(frame, toe, w * 0.6, base.scaled(shade));
            ring(frame, toe, w * 0.6, OUTLINE, INK);
            stroke(
                frame,
                Point {
                    x: bone.end.x - facing * w * 0.3,
                    y: bone.end.y + w * 0.42,
                },
                Point {
                    x: toe.x + facing * w * 0.4,
                    y: toe.y + w * 0.42,
                },
                1.8,
                INK,
            );
        }
    };

    // The painter's order: back limbs, torso, front leg, head, front arm.
    limb(frame, figure.hip, &figure.legs[1], false, false);
    limb(frame, figure.shoulder, &figure.arms[1], false, true);

    // The torso: the look's silhouette. A woman's is an hourglass (a dress flaring at the
    // hips, a trim top, a bare midriff); a man's a straight jacket.
    let neck = figure.neck;
    let hip = figure.hip;
    let up = Point {
        x: hip.x - neck.x,
        y: hip.y - neck.y,
    };
    let length = up.x.hypot(up.y).max(f32::EPSILON);
    let across = Point {
        x: -up.y / length,
        y: up.x / length,
    };
    let at = |along: f32, side: f32| Point {
        x: neck.x + up.x * along + across.x * side,
        y: neck.y + up.y * along + across.y * side,
    };
    if look.feminine {
        // The top: a trim crop top over the chest, with a neckline.
        cel(
            frame,
            neck,
            at(0.42, 0.0),
            look.shoulders * 1.6,
            look.shoulders * 1.5,
            palette.trim,
            palette.trim.scaled(0.62),
        );
        outline_segment(frame, neck, at(0.42, 0.0), look.shoulders * 1.6);
        // The bare midriff: the skin of the waist, narrow.
        cel(
            frame,
            at(0.42, 0.0),
            at(0.72, 0.0),
            look.waist * 1.2,
            look.waist,
            palette.skin,
            palette.skin_shadow,
        );
        outline_segment(frame, at(0.42, 0.0), at(0.72, 0.0), look.waist * 1.2);
        // The skirt: flaring from the waist past the hips, a bold leaf shape.
        for side in [1.0, -1.0] {
            let flare = Point {
                x: at(0.72, 0.0).x + across.x * side * look.hips * 1.4,
                y: at(0.72, 0.0).y + across.y * side * look.hips * 1.4,
            };
            let hem = Point {
                x: hip.x + across.x * side * look.hips * 1.15 + up.x * 0.3,
                y: hip.y + across.y * side * look.hips * 1.15 + up.y * 0.3,
            };
            quad(
                frame,
                at(0.72, side * look.waist),
                flare,
                hem,
                at(1.0, side * look.hips * 0.4),
                palette.outfit,
            );
        }
        // The shadow band on the skirt.
        quad(
            frame,
            at(0.72, -look.waist),
            at(0.72, -look.hips * 1.4),
            at(1.0, -look.hips * 1.15),
            at(1.0, -look.hips * 0.4),
            palette.outfit_shadow,
        );
        // The skirt's outline.
        stroke(
            frame,
            at(0.72, look.waist),
            at(1.0, look.hips * 1.15),
            OUTLINE,
            INK,
        );
        stroke(
            frame,
            at(0.72, -look.waist),
            at(1.0, -look.hips * 1.15),
            OUTLINE,
            INK,
        );
    } else {
        // The jacket: a straight torso in the outfit colour, wide at the shoulders.
        cel(
            frame,
            neck,
            at(0.55, 0.0),
            look.shoulders * 1.8,
            look.waist * 1.5,
            palette.outfit,
            palette.outfit_shadow,
        );
        outline_segment(frame, neck, at(0.55, 0.0), look.shoulders * 1.8);
        cel(
            frame,
            at(0.55, 0.0),
            hip,
            look.waist * 1.5,
            look.hips * 1.4,
            palette.outfit.scaled(0.9),
            palette.outfit_shadow.scaled(0.9),
        );
        outline_segment(frame, at(0.55, 0.0), hip, look.hips * 1.4);
        // The collar, in the trim colour.
        stroke(
            frame,
            neck,
            at(0.12, look.shoulders * 0.6),
            2.6,
            palette.trim,
        );
        stroke(
            frame,
            neck,
            at(0.12, -look.shoulders * 0.6),
            2.6,
            palette.trim,
        );
    }

    limb(frame, figure.hip, &figure.legs[0], true, false);

    // The head, drawn big and last so the hair lands over the torso.
    head(frame, figure.head, facing, &palette, head_r);

    limb(frame, figure.shoulder, &figure.arms[0], true, true);

    // Only the marks (a skin never hides the state of the fighter).
    figure.draw_marks_only(frame);
}

/// The palette `skin` wears: the variant's own, or the plain chalk ink.
pub fn palette_of(skin: &Skin) -> Palette {
    skin.variant.map_or_else(
        || Palette {
            hair: super::stick::INK,
            hair_shadow: super::stick::INK,
            skin: super::stick::INK,
            skin_shadow: super::stick::INK,
            outfit: super::stick::INK,
            outfit_shadow: super::stick::INK,
            trim: super::stick::INK,
            eye: super::stick::INK,
            look: Look::MAN,
        },
        Palette::of,
    )
}

// -----------------------------------------------------------------------------------------
// The sprite engine
// -----------------------------------------------------------------------------------------

/// The sprite bytes of a variant: a little-endian `u32` width, then height, then the RGBA
/// pixels, row by row from the top-left corner — generated once by the art pipeline and
/// embedded, so every engine draws the same art.
const SPRITES: [&[u8]; 10] = [
    include_bytes!("../../assets/skins/brunette.rgba"),
    include_bytes!("../../assets/skins/blonde.rgba"),
    include_bytes!("../../assets/skins/redhead.rgba"),
    include_bytes!("../../assets/skins/ace.rgba"),
    include_bytes!("../../assets/skins/boulder.rgba"),
    include_bytes!("../../assets/skins/spark.rgba"),
    include_bytes!("../../assets/skins/elder.rgba"),
    include_bytes!("../../assets/skins/trooper.rgba"),
    include_bytes!("../../assets/skins/punk.rgba"),
    include_bytes!("../../assets/skins/mystic.rgba"),
];

/// A decoded sprite: the RGBA bytes, row by row, and its size.
#[derive(Debug)]
pub struct Sprite<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
}

/// The sprite of `variant`: the AI-art character of the manga style.
///
/// ```
/// use srr::front::skin::sprite;
/// use srr::game::Variant;
///
/// let sprite = sprite(Variant::Brunette);
/// assert!(sprite.width > 0 && sprite.height > 0);
/// assert_eq!(sprite.pixels.len(), sprite.width * sprite.height * 4);
/// ```
pub fn sprite(variant: Variant) -> Sprite<'static> {
    let bytes = SPRITES[variant.index() as usize];
    let width = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    Sprite {
        pixels: &bytes[8..8 + width * height * 4],
        width,
        height,
    }
}

/// The sprite named `name`, if any (the names are the variants' own).
pub fn sprite_by_name(name: &str) -> Option<Sprite<'static>> {
    Variant::ALL
        .iter()
        .find_map(|variant| (sprite_name_of_variant(*variant) == name).then(|| sprite(*variant)))
}

/// The name a variant's sprite is registered under.
fn sprite_name_of_variant(variant: Variant) -> &'static str {
    match variant {
        Variant::Brunette => "brunette",
        Variant::Blonde => "blonde",
        Variant::Redhead => "redhead",
        Variant::Ace => "ace",
        Variant::Boulder => "boulder",
        Variant::Spark => "spark",
        Variant::Elder => "elder",
        Variant::Trooper => "trooper",
        Variant::Punk => "punk",
        Variant::Mystic => "mystic",
    }
}

/// The name of the sprite `skin` wears: what [`Frame::image`](crate::front::Frame::image)
/// draws for a manga fighter (`None` for the chalk).
pub fn sprite_name_of(skin: &Skin) -> Option<&'static str> {
    skin.variant.map(|variant| match variant {
        Variant::Brunette => "brunette",
        Variant::Blonde => "blonde",
        Variant::Redhead => "redhead",
        Variant::Ace => "ace",
        Variant::Boulder => "boulder",
        Variant::Spark => "spark",
        Variant::Elder => "elder",
        Variant::Trooper => "trooper",
        Variant::Punk => "punk",
        Variant::Mystic => "mystic",
    })
}
