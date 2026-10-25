//! Audio configuration and codec validation rules.
//! Pinned to match Android `Models.kt` and `AudioConfigRulesTest.kt`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Codec {
    AAC,
    MP3,
}

impl Codec {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_uppercase().as_str() {
            "AAC" => Ok(Codec::AAC),
            "MP3" => Ok(Codec::MP3),
            other => Err(format!("Unknown codec '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BroadcastMode {
    SOURCE,
    DJ,
}

impl BroadcastMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "DJ" => BroadcastMode::DJ,
            _ => BroadcastMode::SOURCE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DjProtocol {
    ULTRAVOX,
    ICECAST,
}

impl DjProtocol {
    /// Parse a stored value; any unrecognized/blank/legacy-AUTO string -> ULTRAVOX (the default).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "ICECAST" => DjProtocol::ICECAST,
            _ => DjProtocol::ULTRAVOX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioConfig {
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub mono: bool,
}

pub struct AudioConfigRules;

impl AudioConfigRules {
    pub const AAC_BITRATES: &'static [u32] = &[16, 24, 32, 48, 64, 96, 128];
    pub const MP3_BITRATES: &'static [u32] = &[16, 24, 32, 48, 64, 96, 128, 192];
    pub const SAMPLE_RATES: &'static [u32] = &[16_000, 22_050, 44_100, 48_000];
    /// MP3 at 16/24 kbps needs MPEG-2 Layer III (sample rate <= 24 kHz); LAME bumps to 32 otherwise.
    pub const MPEG2_SAMPLE_RATES: &'static [u32] = &[16_000, 22_050];

    /// At 16/24 kbps we want HE-AAC v1 (AAC+) and mono + low rate.
    pub fn needs_he_aac(codec: Codec, bitrate_kbps: u32) -> bool {
        codec == Codec::AAC && bitrate_kbps <= 24
    }

    /// MP3 at 16/24 kbps is only a valid MPEG-2 Layer III bitrate (needs <= 24 kHz).
    pub fn mp3_needs_mpeg2(codec: Codec, bitrate_kbps: u32) -> bool {
        codec == Codec::MP3 && bitrate_kbps <= 24
    }

    /// Sample rates the picker should offer for a codec+bitrate.
    pub fn valid_sample_rates_for(codec: Codec, bitrate_kbps: u32) -> &'static [u32] {
        if Self::mp3_needs_mpeg2(codec, bitrate_kbps) {
            Self::MPEG2_SAMPLE_RATES
        } else {
            Self::SAMPLE_RATES
        }
    }

    /// Auto-correct impossible combos — never errors. MP3 16/24 + >24 kHz -> clamp to nearest <= 24 kHz.
    pub fn clamp(c: AudioConfig) -> AudioConfig {
        if Self::mp3_needs_mpeg2(c.codec, c.bitrate_kbps)
            && !Self::MPEG2_SAMPLE_RATES.contains(&c.sample_rate_hz)
        {
            let nearest = *Self::MPEG2_SAMPLE_RATES
                .iter()
                .min_by_key(|&&rate| (rate as i64 - c.sample_rate_hz as i64).abs())
                .unwrap();
            return AudioConfig {
                sample_rate_hz: nearest,
                ..c
            };
        }
        c
    }

    /// Icecast source mounts don't accept ADTS/AAC — only MP3 is supported there.
    pub fn is_supported(codec: Codec, protocol: DjProtocol) -> bool {
        !(codec == Codec::AAC && protocol == DjProtocol::ICECAST)
    }

    pub fn validate(c: AudioConfig) -> Result<AudioConfig, String> {
        let allowed = if c.codec == Codec::AAC {
            Self::AAC_BITRATES
        } else {
            Self::MP3_BITRATES
        };
        if !allowed.contains(&c.bitrate_kbps) {
            return Err(format!("bitrate {} not in {:?}", c.bitrate_kbps, allowed));
        }
        if !Self::SAMPLE_RATES.contains(&c.sample_rate_hz) {
            return Err(format!("sample rate {}", c.sample_rate_hz));
        }
        if Self::needs_he_aac(c.codec, c.bitrate_kbps) && !c.mono {
            return Err("16/24k must be mono".to_string());
        }
        if Self::mp3_needs_mpeg2(c.codec, c.bitrate_kbps)
            && !Self::MPEG2_SAMPLE_RATES.contains(&c.sample_rate_hz)
        {
            return Err(format!(
                "MP3 {}kbps needs <= 24 kHz (MPEG-2)",
                c.bitrate_kbps
            ));
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aac_low_rate_needs_heaac_and_mono() {
        assert!(AudioConfigRules::needs_he_aac(Codec::AAC, 16));
        assert!(AudioConfigRules::needs_he_aac(Codec::AAC, 24));
        assert!(!AudioConfigRules::needs_he_aac(Codec::AAC, 32));
        assert!(!AudioConfigRules::needs_he_aac(Codec::MP3, 16));
    }

    #[test]
    fn validate_accepts_aac_64_stereo() {
        let cfg = AudioConfig {
            codec: Codec::AAC,
            bitrate_kbps: 64,
            sample_rate_hz: 44_100,
            mono: false,
        };
        assert!(AudioConfigRules::validate(cfg).is_ok());
    }

    #[test]
    fn validate_rejects_16k_stereo() {
        let cfg = AudioConfig {
            codec: Codec::AAC,
            bitrate_kbps: 16,
            sample_rate_hz: 22_050,
            mono: false,
        };
        assert!(AudioConfigRules::validate(cfg).is_err());
    }

    #[test]
    fn validate_rejects_unknown_bitrate() {
        let cfg = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 77,
            sample_rate_hz: 44_100,
            mono: true,
        };
        assert!(AudioConfigRules::validate(cfg).is_err());
    }

    #[test]
    fn validate_rejects_unknown_sample_rate() {
        let cfg = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 128,
            sample_rate_hz: 99_999,
            mono: true,
        };
        assert!(AudioConfigRules::validate(cfg).is_err());
    }

    #[test]
    fn mp3_low_bitrate_needs_mpeg2_sample_rate() {
        assert!(AudioConfigRules::mp3_needs_mpeg2(Codec::MP3, 16));
        assert!(AudioConfigRules::mp3_needs_mpeg2(Codec::MP3, 24));
        assert!(!AudioConfigRules::mp3_needs_mpeg2(Codec::MP3, 48));
        assert!(!AudioConfigRules::mp3_needs_mpeg2(Codec::AAC, 24));
    }

    #[test]
    fn clamp_mp3_low_bitrate_down_to_mpeg2() {
        let c1 = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 24,
            sample_rate_hz: 44_100,
            mono: true,
        };
        assert_eq!(AudioConfigRules::clamp(c1).sample_rate_hz, 22_050);

        let c2 = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 16,
            sample_rate_hz: 48_000,
            mono: true,
        };
        assert_eq!(AudioConfigRules::clamp(c2).sample_rate_hz, 22_050);

        let c3 = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 24,
            sample_rate_hz: 22_050,
            mono: true,
        };
        assert_eq!(AudioConfigRules::clamp(c3).sample_rate_hz, 22_050);

        let c4 = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 128,
            sample_rate_hz: 44_100,
            mono: true,
        };
        assert_eq!(AudioConfigRules::clamp(c4).sample_rate_hz, 44_100);
    }

    #[test]
    fn validate_rejects_mp3_24k_at_44khz() {
        let cfg = AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: 24,
            sample_rate_hz: 44_100,
            mono: true,
        };
        assert!(AudioConfigRules::validate(cfg).is_err());
    }

    #[test]
    fn icecast_rejects_aac() {
        assert!(!AudioConfigRules::is_supported(Codec::AAC, DjProtocol::ICECAST));
        assert!(AudioConfigRules::is_supported(Codec::MP3, DjProtocol::ICECAST));
        assert!(AudioConfigRules::is_supported(Codec::AAC, DjProtocol::ULTRAVOX));
    }

    #[test]
    fn dj_protocol_parse() {
        assert_eq!(DjProtocol::parse("ULTRAVOX"), DjProtocol::ULTRAVOX);
        assert_eq!(DjProtocol::parse("ICECAST"), DjProtocol::ICECAST);
        assert_eq!(DjProtocol::parse("icecast"), DjProtocol::ICECAST);
        assert_eq!(DjProtocol::parse("AUTO"), DjProtocol::ULTRAVOX);
        assert_eq!(DjProtocol::parse(""), DjProtocol::ULTRAVOX);
        assert_eq!(DjProtocol::parse("nope"), DjProtocol::ULTRAVOX);
    }
}
