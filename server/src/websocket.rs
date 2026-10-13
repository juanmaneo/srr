//! The small part of WebSocket (RFC 6455) the relay needs: the opening handshake key and
//! single-frame messages. Fragmented messages and extensions are not supported; browsers never
//! fragment messages as small as the game's.

use std::io::{self, Read, Write};

/// A continuation frame (only used by fragmented messages, which are rejected).
pub const OP_CONTINUATION: u8 = 0x0;
/// A UTF-8 text message.
pub const OP_TEXT: u8 = 0x1;
/// A binary message.
pub const OP_BINARY: u8 = 0x2;
/// The closing handshake.
pub const OP_CLOSE: u8 = 0x8;
/// A ping, to be answered with a pong carrying the same payload.
pub const OP_PING: u8 = 0x9;
/// The answer to a ping.
pub const OP_PONG: u8 = 0xA;

/// Frames larger than this are refused: the game's messages are a few bytes long.
pub const MAX_PAYLOAD: u64 = 64 * 1024;

const HANDSHAKE_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// One frame received from a browser, already unmasked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Whether this is the last frame of its message.
    pub fin: bool,
    /// The frame type (`OP_*`).
    pub opcode: u8,
    /// The application data.
    pub payload: Vec<u8>,
}

/// The `Sec-WebSocket-Accept` value answering the client's `Sec-WebSocket-Key`.
///
/// ```
/// // The example of RFC 6455, section 1.3.
/// assert_eq!(
///     srr_server::websocket::accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
///     "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
/// );
/// ```
pub fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{key}{HANDSHAKE_GUID}").as_bytes()))
}

/// Reads one frame sent by a client.
///
/// # Errors
///
/// I/O errors of `reader` (end of stream included), and `InvalidData` for unmasked frames
/// (clients must mask) and payloads above [`MAX_PAYLOAD`].
pub fn read_frame(reader: &mut impl Read) -> io::Result<Frame> {
    let [first, second] = read_array::<2>(reader)?;
    if second & 0x80 == 0 {
        return Err(invalid_data("client frames must be masked"));
    }
    let length = match second & 0x7F {
        126 => u64::from(u16::from_be_bytes(read_array(reader)?)),
        127 => u64::from_be_bytes(read_array(reader)?),
        short => u64::from(short),
    };
    if length > MAX_PAYLOAD {
        return Err(invalid_data("frame too large"));
    }
    let mask = read_array::<4>(reader)?;
    // At most MAX_PAYLOAD, so it fits in a usize.
    let mut payload = vec![0; length as usize];
    reader.read_exact(&mut payload)?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    Ok(Frame {
        fin: first & 0x80 != 0,
        opcode: first & 0x0F,
        payload,
    })
}

/// Writes one complete, unmasked server frame.
///
/// # Errors
///
/// The I/O errors of `writer`.
pub fn write_frame(writer: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 10);
    frame.push(0x80 | opcode);
    match payload.len() {
        short @ 0..=125 => frame.push(short as u8),
        medium @ 126..=0xFFFF => {
            frame.push(126);
            frame.extend_from_slice(&(medium as u16).to_be_bytes());
        }
        long => {
            frame.push(127);
            frame.extend_from_slice(&(long as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()
}

fn read_array<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        let symbol = |shift: u32| char::from(BASE64_ALPHABET[(group >> shift & 0x3F) as usize]);
        text.push(symbol(18));
        text.push(symbol(12));
        text.push(if chunk.len() > 1 { symbol(6) } else { '=' });
        text.push(if chunk.len() > 2 { symbol(0) } else { '=' });
    }
    text
}

/// SHA-1 (FIPS 180-4). Only used for the handshake, where the protocol mandates it.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    let bit_length = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    let (blocks, _) = message.as_chunks::<64>();
    for block in blocks {
        let mut words = [0u32; 80];
        let (block_words, _) = block.as_chunks::<4>();
        for (index, word) in block_words.iter().enumerate() {
            words[index] = u32::from_be_bytes(*word);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.into_iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (value, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *value = value.wrapping_add(add);
        }
    }

    let mut digest = [0; 20];
    let (digest_words, _) = digest.as_chunks_mut::<4>();
    for (bytes, value) in digest_words.iter_mut().zip(state) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
    digest
}
