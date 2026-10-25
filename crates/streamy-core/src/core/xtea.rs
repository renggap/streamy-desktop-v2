//! XTEA encipher used by the SHOUTcast 2 / Ultravox 2.1 auth handshake (message 0x1001).
//!
//! The server's cipher key (returned by 0x1009) is the obfuscation key; DNAS2 currently returns
//! the fixed string "foobar". The UID + AuthBlob fields are XTEA-enciphered, then emitted as
//! 8-hex-digit (32-bit) words so the ':'-separated auth payload never collides with a colon.
//! Ported verbatim (big-endian, 32 rounds, zero-padded key/data) from Android `UvoxXtea`.

const DELTA: u32 = 0x9E3779B9;

pub fn encipher(data: &[u8], key: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let k = pad_key(key);
    let pad_len = if data.len() % 8 == 0 {
        0
    } else {
        8 - (data.len() % 8)
    };
    let mut padded = Vec::with_capacity(data.len() + pad_len);
    padded.extend_from_slice(data);
    padded.resize(data.len() + pad_len, 0);

    let mut out = String::with_capacity(padded.len() * 2);
    let mut i = 0;
    while i < padded.len() {
        let mut v0 = u32_be(&padded[i..i + 4]);
        let mut v1 = u32_be(&padded[i + 4..i + 8]);
        let mut sum: u32 = 0;
        for _ in 0..32 {
            let part1 = ((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1);
            let part2 = sum.wrapping_add(k[(sum & 3) as usize]);
            v0 = v0.wrapping_add(part1 ^ part2);

            sum = sum.wrapping_add(DELTA);

            let part3 = ((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0);
            let part4 = sum.wrapping_add(k[((sum >> 11) & 3) as usize]);
            v1 = v1.wrapping_add(part3 ^ part4);
        }
        out.push_str(&format!("{:08x}{:08x}", v0, v1));
        i += 8;
    }
    out
}

fn pad_key(key: &[u8]) -> [u32; 4] {
    let mut k = [0u8; 16];
    let copy_len = key.len().min(16);
    k[..copy_len].copy_from_slice(&key[..copy_len]);
    [
        u32_be(&k[0..4]),
        u32_be(&k[4..8]),
        u32_be(&k[8..12]),
        u32_be(&k[12..16]),
    ]
}

#[inline]
fn u32_be(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encipher_matches_reference_vectors() {
        assert_eq!(
            encipher(b"test", b"foobar"),
            "0797c1d44e31a1f3"
        );
        assert_eq!(
            encipher(b"6naZQ3gy", b"foobar"),
            "dcd4cb55b273478d"
        );
    }

    #[test]
    fn encipher_empty_input_is_empty() {
        assert_eq!(encipher(b"", b"foobar"), "");
    }

    #[test]
    fn encipher_pads_short_keys_to_16_bytes() {
        assert_eq!(
            encipher(b"test", b"ab"),
            "1b12ddeaacf76b53"
        );
        assert_eq!(
            encipher(b"test", b"abcdefghijklmno"),
            "cc86e8c176b00346"
        );
        assert_eq!(
            encipher(b"test", b"0123456789abcdef"),
            "6c302719dd265d3f"
        );
    }

    #[test]
    fn encipher_truncates_long_keys_to_16_bytes() {
        let sixteen = encipher(b"test", b"0123456789abcdef");
        let twenty = encipher(b"test", b"0123456789abcdefGHIJ");
        assert_eq!(twenty, sixteen);
        assert_eq!(twenty, "6c302719dd265d3f");
    }

    #[test]
    fn encipher_multiple_blocks_are_concatenated() {
        assert_eq!(
            encipher(b"abcdefghijklmnop", b"foobar"),
            "95b8f749a8590cb6d546ae4022b542e7"
        );
    }
}
