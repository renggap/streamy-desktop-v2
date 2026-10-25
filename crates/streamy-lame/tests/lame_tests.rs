use streamy_lame::{lame_version, LameError, Mp3Encoder};

fn is_valid_mp3_sync(frame: &[u8]) -> bool {
    if frame.len() < 4 {
        return false;
    }
    // MP3 frame sync word: 11 bits set (0xFF, bits 7-5 of next byte)
    // Layer III: bits 2-1 of byte 1 is 01 (Layer 3)
    let b0 = frame[0];
    let b1 = frame[1];
    let sync = (b0 == 0xFF) && ((b1 & 0xE0) == 0xE0);
    let layer3 = ((b1 >> 1) & 0x03) == 0x01; // Layer III
    sync && layer3
}

fn find_first_mp3_frame(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    for i in 0..data.len() - 3 {
        if is_valid_mp3_sync(&data[i..]) {
            return Some(i);
        }
    }
    None
}

#[test]
fn test_lame_version_string() {
    let v = lame_version();
    assert!(!v.is_empty());
    println!("LAME version: {}", v);
}

#[test]
fn test_fixture_silence_mp3_has_valid_header() {
    let bytes = include_bytes!("fixtures/silence.mp3");
    assert!(!bytes.is_empty());
    // silence.mp3 begins with an ID3v2 header and has its first audio frame at offset 0x2C (ff fb 40 c0)
    let frame_offset = find_first_mp3_frame(bytes).expect("Expected MP3 audio frame in silence.mp3");
    assert!(is_valid_mp3_sync(&bytes[frame_offset..]));
}

#[test]
fn test_every_legal_audio_config_encodes_and_flushes() {
    let mp3_bitrates = [16, 24, 32, 48, 64, 96, 128, 192];
    let mpeg2_rates = [16_000, 22_050];
    let all_rates = [16_000, 22_050, 44_100, 48_000];
    let channel_options = [true, false]; // mono, stereo

    let mut combinations_tested = 0;

    for &bitrate in &mp3_bitrates {
        let valid_rates: &[u32] = if bitrate <= 24 {
            &mpeg2_rates
        } else {
            &all_rates
        };

        for &rate in valid_rates {
            for &mono in &channel_options {
                let mut encoder = Mp3Encoder::new(rate, bitrate, mono)
                    .unwrap_or_else(|e| panic!("Failed to init encoder for {}kbps {}Hz mono={}: {:?}", bitrate, rate, mono, e));

                assert_eq!(encoder.out_sample_rate_hz(), rate);
                assert_eq!(encoder.channels(), if mono { 1 } else { 2 });

                // Synthesize 0.1s of 440Hz sine wave PCM
                let channels = if mono { 1 } else { 2 };
                let sample_count = (rate as usize / 10) * channels;
                let mut pcm = Vec::with_capacity(sample_count);
                for i in 0..sample_count {
                    let t = (i / channels) as f32 / rate as f32;
                    let val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
                    pcm.push((val * 16000.0) as i16);
                }

                let mut mp3_bytes = Vec::new();
                let encoded = encoder.encode(&pcm).expect("encode failed");
                for frame in encoded {
                    mp3_bytes.extend(frame);
                }

                let flushed = encoder.flush().expect("flush failed");
                for frame in flushed {
                    mp3_bytes.extend(frame);
                }

                assert!(
                    !mp3_bytes.is_empty(),
                    "Expected encoded MP3 data for {}kbps {}Hz mono={}",
                    bitrate,
                    rate,
                    mono
                );

                assert!(
                    is_valid_mp3_sync(&mp3_bytes),
                    "Output data does not start with valid MP3 frame header for {}kbps {}Hz mono={}",
                    bitrate,
                    rate,
                    mono
                );

                encoder.close();
                assert!(encoder.is_closed());
                combinations_tested += 1;
            }
        }
    }

    // 2 (low bitrates 16, 24) * 2 rates * 2 channels = 8
    // 6 (high bitrates 32, 48, 64, 96, 128, 192) * 4 rates * 2 channels = 48
    // Total = 56 legal configurations
    assert_eq!(combinations_tested, 56);
    println!("Successfully tested all {} legal MP3 configurations!", combinations_tested);
}

#[test]
fn test_encode_after_close_refused() {
    let mut encoder = Mp3Encoder::new(44_100, 128, true).unwrap();
    encoder.close();
    let res = encoder.encode(&[0i16; 1024]);
    assert!(matches!(res, Err(LameError::AlreadyClosed)));
}

#[test]
fn test_idempotent_close() {
    let mut encoder = Mp3Encoder::new(44_100, 128, true).unwrap();
    encoder.close();
    encoder.close(); // second call should be no-op, no crash or double free
    assert!(encoder.is_closed());
}
