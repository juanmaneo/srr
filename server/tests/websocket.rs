use std::io::{self, Cursor, Write};

use srr_server::websocket::{
    Frame, MAX_PAYLOAD, OP_BINARY, OP_CLOSE, OP_CONTINUATION, OP_PING, OP_PONG, OP_TEXT,
    accept_key, base64, read_frame, sha1, write_frame,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A frame as a browser sends it: masked, with the shortest length encoding.
fn client_frame(first: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut frame = vec![first];
    match payload.len() {
        short @ 0..=125 => frame.push(0x80 | short as u8),
        medium @ 126..=0xFFFF => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(medium as u16).to_be_bytes());
        }
        long => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(long as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    frame
}

fn read(bytes: &[u8]) -> io::Result<Frame> {
    read_frame(&mut Cursor::new(bytes))
}

fn written(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_frame(&mut out, opcode, payload).unwrap();
    out
}

#[test]
fn opcodes_match_rfc_6455() {
    assert_eq!(
        [
            OP_CONTINUATION,
            OP_TEXT,
            OP_BINARY,
            OP_CLOSE,
            OP_PING,
            OP_PONG
        ],
        [0x0, 0x1, 0x2, 0x8, 0x9, 0xA]
    );
    assert_eq!(MAX_PAYLOAD, 65536);
}

#[test]
fn sha1_test_vectors() {
    assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(
        hex(&sha1(b"abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    // 56 bytes: the padding needs a second block.
    assert_eq!(
        hex(&sha1(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    // Several full blocks.
    assert_eq!(
        hex(&sha1(&[b'a'; 1000])),
        "291e9a6c66994949b57ba5e650361e98fc36b1ba"
    );
}

#[test]
fn base64_test_vectors() {
    let cases = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];
    for (plain, encoded) in cases {
        assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
    }
    assert_eq!(base64(&[0xFB, 0xFF, 0xBF]), "+/+/");
}

#[test]
fn accept_key_of_rfc_6455() {
    assert_eq!(
        accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

#[test]
fn reads_masked_frames_of_every_length_encoding() {
    let mask = [0x37, 0xFA, 0x21, 0x3D];
    for length in [0, 1, 125, 126, 1000, 0xFFFF, 0x1_0000] {
        let payload: Vec<u8> = (0..length).map(|index| index as u8).collect();
        let frame = read(&client_frame(0x80 | OP_BINARY, &payload, mask)).unwrap();
        assert_eq!(
            frame,
            Frame {
                fin: true,
                opcode: OP_BINARY,
                payload
            },
            "length {length}"
        );
    }
}

#[test]
fn reads_the_rfc_6455_masked_hello() {
    let bytes = [
        0x81, 0x85, 0x37, 0xFA, 0x21, 0x3D, 0x7F, 0x9F, 0x4D, 0x51, 0x58,
    ];
    let frame = read(&bytes).unwrap();
    assert!(frame.fin);
    assert_eq!(frame.opcode, OP_TEXT);
    assert_eq!(frame.payload, b"Hello");
}

#[test]
fn reads_non_final_frames() {
    let frame = read(&client_frame(OP_TEXT, b"Hel", [1, 2, 3, 4])).unwrap();
    assert!(!frame.fin);
    assert_eq!(frame.opcode, OP_TEXT);
    assert_eq!(frame.payload, b"Hel");
}

#[test]
fn a_64_bit_length_may_encode_a_small_payload() {
    let mut bytes = vec![0x80 | OP_PING, 0x80 | 127];
    bytes.extend_from_slice(&3u64.to_be_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"abc");
    assert_eq!(read(&bytes).unwrap().payload, b"abc");
}

#[test]
fn rejects_unmasked_frames() {
    let error = read(&[0x80 | OP_BINARY, 0x01, 0x42]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "client frames must be masked");
}

#[test]
fn rejects_frames_above_the_limit() {
    let mut bytes = vec![0x80 | OP_BINARY, 0x80 | 127];
    bytes.extend_from_slice(&(MAX_PAYLOAD + 1).to_be_bytes());
    let error = read(&bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "frame too large");

    let mut bytes = vec![0x80 | OP_BINARY, 0x80 | 127];
    bytes.extend_from_slice(&u64::MAX.to_be_bytes());
    assert_eq!(read(&bytes).unwrap_err().kind(), io::ErrorKind::InvalidData);
}

#[test]
fn truncated_frames_are_unexpected_eof() {
    let full = client_frame(0x80 | OP_BINARY, &[7; 200], [9, 8, 7, 6]);
    // Header, 16-bit length, mask and payload all cut short.
    for cut in [0, 1, 2, 3, 5, 7, 8, 100] {
        let error = read(&full[..cut]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
    }
    // A 64-bit length cut short.
    let error = read(&[0x82, 0x80 | 127, 0, 0, 0]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn writes_unmasked_frames_of_every_length_encoding() {
    assert_eq!(written(OP_CLOSE, &[]), vec![0x88, 0]);
    assert_eq!(written(OP_BINARY, &[1, 2]), vec![0x82, 2, 1, 2]);

    let payload = [0xAB; 125];
    let out = written(OP_BINARY, &payload);
    assert_eq!(out[..2], [0x82, 125]);
    assert_eq!(out[2..], payload);

    for length in [126, 0xFFFF] {
        let payload = vec![0xCD; length];
        let out = written(OP_PONG, &payload);
        assert_eq!(out[..2], [0x8A, 126]);
        assert_eq!(out[2..4], (length as u16).to_be_bytes());
        assert_eq!(out[4..], payload[..]);
    }

    for length in [0x1_0000, 0x1_0005] {
        let payload = vec![0xEF; length];
        let out = written(OP_TEXT, &payload);
        assert_eq!(out[..2], [0x81, 127]);
        assert_eq!(out[2..10], (length as u64).to_be_bytes());
        assert_eq!(out[10..], payload[..]);
    }
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn write_errors_are_returned() {
    // Exercise the test writer's flush so it is not dead code.
    Broken.flush().unwrap();
    let error = write_frame(&mut Broken, OP_BINARY, &[1]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}
