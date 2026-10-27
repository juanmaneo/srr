//! The saved keyboard layout: the bytes of a [`KeyBindings`] as base 64, for the browser's
//! local storage.
//!
//! The encoding itself is plain Rust, so it is unit-tested here on every platform (the rest of
//! the platform layer is only checked by playing): a layout that fails to load after a reload
//! is exactly the bug the tests around [`decode`] guard against.

use crate::game::KeyBindings;

/// `bytes` as base 64, so the layout survives in local storage as a string.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let byte = |i| group.get(i).copied().unwrap_or(0);
        let triple = (u32::from(byte(0)) << 16) | (u32::from(byte(1)) << 8) | u32::from(byte(2));
        let digit = |value: u32| ALPHABET[(value & 63) as usize] as char;
        let mut chars = [
            digit(triple >> 18),
            digit(triple >> 12),
            digit(triple >> 6),
            digit(triple),
        ];
        for pad in group.len()..3 {
            chars[1 + pad] = '=';
        }
        for c in chars {
            out.push(c);
        }
    }
    out
}

/// The bytes [`encode`] wrote, or nothing when the text is not base 64. The `=` padding of the
/// last group ends the number (a 14-byte layout encodes to 20 characters, the last one `=`).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut numbers = Vec::new();
    for byte in text.bytes() {
        match ALPHABET.iter().position(|&a| a == byte) {
            Some(index) => numbers.push(index as u32),
            None if byte == b'=' => break,
            None => return Vec::new(),
        }
    }
    let mut out = Vec::new();
    for group in numbers.chunks(4) {
        let value = |i| group.get(i).copied().unwrap_or(0);
        let quadruple = (value(0) << 18) | (value(1) << 12) | (value(2) << 6) | value(3);
        out.push((quadruple >> 16) as u8);
        if group.len() > 2 {
            out.push((quadruple >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(quadruple as u8);
        }
    }
    out
}

/// The layout `saved` stands for, or `None` when it does not decode (see
/// [`KeyBindings::from_bytes`]).
///
/// Saves are [`versioned`](store) (`"srr1:..."`); one without a prefix is what the first
/// version of the feature wrote, and still loads.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn load(saved: &str) -> Option<KeyBindings> {
    if let Some(rest) = saved.strip_prefix(FORMAT) {
        return KeyBindings::from_bytes(&decode(rest));
    }
    // A save of an unknown format version: ignored, the game keeps its default layout.
    if saved.contains(':') {
        return None;
    }
    KeyBindings::from_bytes(&decode(saved))
}

/// The layout as text for the browser's local storage.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn store(bindings: &KeyBindings) -> String {
    format!("{FORMAT}{}", encode(&bindings.to_bytes()))
}

/// The marker every save starts with, so the format can evolve: a save of another version
/// (`srr2`, ...) is [`ignored`](load) instead of misread, and this one can change shape under
/// a new marker when the layout itself gains actions.
const FORMAT: &str = "srr1:";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Key;

    /// A saved layout loads back: the regression test of the padding bug (a saved layout
    /// never loaded, the game fell back to its default).
    #[test]
    fn saved_layout_loads_back() {
        // Any remapped layout, like grab moved from Space to L.
        let mut mine = KeyBindings::WASD;
        assert!(mine.bind(9, Key::L));
        let stored = store(&mine);
        assert_eq!(load(&stored), Some(mine));
    }

    #[test]
    fn saved_presets_load_back() {
        for (_, preset) in KeyBindings::PRESETS {
            assert_eq!(load(&store(&preset)), Some(preset));
        }
    }

    #[test]
    fn padding_is_written_and_decoded() {
        // A 14-byte layout: 4 full groups of 3 bytes plus 2 bytes, so the last group ends
        // with one `=`.
        let bytes = [0_u8; 14];
        assert_eq!(KeyBindings::WASD.to_bytes().len(), 14);
        assert_eq!(bytes.len(), 14);
        let text = encode(&bytes);
        assert_eq!(text.len(), 20);
        assert!(text.ends_with('='));
        assert_eq!(decode(&text), bytes);
    }

    #[test]
    fn saves_carry_a_version() {
        let stored = store(&KeyBindings::WASD);
        assert!(stored.starts_with("srr1:"));
    }

    #[test]
    fn versioned_saves_load_back() {
        let mut mine = KeyBindings::WASD;
        assert!(mine.bind(9, Key::L));
        let stored = store(&mine);
        assert!(stored.starts_with("srr1:"));
        assert_eq!(load(&stored), Some(mine));
    }

    #[test]
    fn unversioned_saves_still_load_and_are_migrated_on_the_next_save() {
        // What v3 wrote: the bare base 64 of the layout bytes, no "srr1:" prefix.
        let mut mine = KeyBindings::WASD;
        assert!(mine.bind(9, Key::L));
        let v3_save = encode(&mine.to_bytes());
        assert!(!v3_save.contains(':'));
        assert_eq!(load(&v3_save), Some(mine));
        // The next save of the same layout is versioned.
        let migrated = store(&load(&v3_save).unwrap());
        assert!(migrated.starts_with("srr1:"));
        assert_eq!(load(&migrated), Some(mine));
    }

    #[test]
    fn saves_of_an_unknown_version_are_ignored() {
        assert_eq!(load("srr2:AAAA"), None);
        assert_eq!(load("srr999:AAAA"), None);
        assert_eq!(load("srr1"), None);
        assert_eq!(load(":"), None);
        assert_eq!(load("srr1:"), None);
    }

    #[test]
    fn decoding_refuses_garbage() {
        assert!(decode("!@#$").is_empty());
        assert_eq!(load("!!!!"), None);
        assert_eq!(load(""), None);
    }
}
