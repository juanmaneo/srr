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
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn load(saved: &str) -> Option<KeyBindings> {
    KeyBindings::from_bytes(&decode(saved))
}

/// The layout as text for the browser's local storage.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn store(bindings: &KeyBindings) -> String {
    encode(&bindings.to_bytes())
}

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
    fn decoding_refuses_garbage() {
        assert!(decode("!@#$").is_empty());
        assert_eq!(load("!!!!"), None);
        assert_eq!(load(""), None);
    }
}
