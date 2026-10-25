//! Ultravox 2.1 binary framing and AAC ADTS wrapping.

const AAC_LC_PROFILE: u8 = 1;

/// Map a sample rate in Hz to its ADTS frequency index.
pub fn adts_freq_index(sample_rate_hz: u32) -> u8 {
    match sample_rate_hz {
        96_000 => 0,
        88_200 => 1,
        64_000 => 2,
        48_000 => 3,
        44_100 => 4,
        32_000 => 5,
        24_000 => 6,
        22_050 => 7,
        16_000 => 8,
        12_000 => 9,
        11_025 => 10,
        8_000 => 11,
        7_350 => 12,
        _ => 4,
    }
}

/// Build a 7-byte (no-CRC) ADTS header wrapping one AAC access unit.
/// Profile field = AAC-LC base layer (carries HE-AAC backward-compatibly — implicit SBR).
/// Channel configuration low 2 bits live in byte 3 bits 7-6.
pub fn adts_header(au: &[u8], sample_rate_hz: u32, channels: u8) -> Vec<u8> {
    let freq_idx = adts_freq_index(sample_rate_hz);
    let frame_len = (au.len() + 7) as u32;
    let mut out = vec![0u8; 7 + au.len()];

    out[0] = 0xFF;
    out[1] = 0xF1; // MPEG-4, layer 0, protection_absent (no CRC)
    out[2] = ((AAC_LC_PROFILE & 0x3) << 6) | (freq_idx << 2) | ((channels >> 2) & 0x1);
    out[3] = ((channels & 0x3) << 6) | (((frame_len >> 11) & 0x3) as u8);
    out[4] = ((frame_len >> 3) & 0xFF) as u8;
    out[5] = (((frame_len & 0x7) << 5) | 0x1F) as u8;
    out[6] = 0xFC;
    out[7..].copy_from_slice(au);
    out
}

/// Build a full Ultravox wire message (header + payload + 0x00 trailer).
/// Header format:
///   [0..1] = 0x5A, 0x00
///   [2..3] = (class << 12) | type (Big Endian u16)
///   [4..5] = payload length (Big Endian u16)
///   [6..6+payload.len()] = payload
///   [last] = 0x00 trailer byte
pub fn message(cls: u16, msg_type: u16, payload: &[u8]) -> Vec<u8> {
    let ct = ((cls & 0xF) << 12) | (msg_type & 0xFFF);
    let mut out = vec![0u8; 6 + payload.len() + 1];
    out[0] = 0x5A;
    out[1] = 0x00;
    out[2] = ((ct >> 8) & 0xFF) as u8;
    out[3] = (ct & 0xFF) as u8;
    out[4] = ((payload.len() >> 8) & 0xFF) as u8;
    out[5] = (payload.len() & 0xFF) as u8;
    out[6..6 + payload.len()].copy_from_slice(payload);
    out[6 + payload.len()] = 0x00;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_config(hdr: &[u8]) -> u8 {
        let hi = hdr[2] & 0x1;
        let lo = (hdr[3] & 0xC0) >> 6;
        (hi << 2) | lo
    }

    fn frame_length(hdr: &[u8]) -> usize {
        let b3 = (hdr[3] & 0x3) as usize;
        let b4 = hdr[4] as usize;
        let b5 = ((hdr[5] & 0xE0) >> 5) as usize;
        (b3 << 11) | (b4 << 3) | b5
    }

    #[test]
    fn adts_header_carries_correct_channel_configuration() {
        let au = vec![0u8; 64];
        assert_eq!(channel_config(&adts_header(&au, 44_100, 1)), 1);
        assert_eq!(channel_config(&adts_header(&au, 44_100, 2)), 2);
    }

    #[test]
    fn adts_frame_length_is_au_plus_header() {
        let au = vec![0u8; 100];
        assert_eq!(frame_length(&adts_header(&au, 22_050, 1)), 107);
    }

    #[test]
    fn adts_sync_and_protection_absent() {
        let h = adts_header(&vec![0u8; 32], 44_100, 1);
        assert_eq!(h[0], 0xFF);
        assert_eq!(h[1], 0xF1);
    }

    #[test]
    fn ultravox_message_framing() {
        let msg = message(1, 0x009, b"2.1\0");
        assert_eq!(msg[0], 0x5A);
        assert_eq!(msg[1], 0x00);
        assert_eq!(msg[2], 0x10);
        assert_eq!(msg[3], 0x09);
        assert_eq!(msg[4], 0x00);
        assert_eq!(msg[5], 0x04);
        assert_eq!(&msg[6..10], b"2.1\0");
        assert_eq!(msg[10], 0x00); // trailer
    }
}
