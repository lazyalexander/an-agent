#![allow(clippy::disallowed_methods)] // the seam is the licensed exception; see clippy.toml

use std::time::Duration;

use chrono::{TimeZone, Utc};

/// Unix milliseconds. Wall is production; Frozen is tests/replay.
/// Do not feed this into Entropy — pin clock and seed separately.
#[derive(Clone, Copy, Debug)]
pub enum Clock {
    Wall,
    Frozen(u64),
}

impl Clock {
    pub fn wall() -> Self {
        Self::Wall
    }

    pub fn frozen(ms: u64) -> Self {
        Self::Frozen(ms)
    }

    pub fn now_ms(self) -> u64 {
        match self {
            Self::Wall => Utc::now().timestamp_millis() as u64,
            Self::Frozen(ms) => ms,
        }
    }

    pub fn now_iso(self) -> String {
        Self::format_ms(self.now_ms())
    }

    pub fn format_ms(ms: u64) -> String {
        let secs = (ms / 1000) as i64;
        let nsec = ((ms % 1000) * 1_000_000) as u32;
        Utc.timestamp_opt(secs, nsec)
            .single()
            .unwrap_or(chrono::DateTime::UNIX_EPOCH)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    pub fn from_duration(d: Duration) -> u64 {
        d.as_millis() as u64
    }

    pub fn to_duration(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_iso_is_epoch_and_units_round_trip() {
        assert_eq!(Clock::frozen(0).now_iso(), "1970-01-01T00:00:00.000Z");
        assert_eq!(Clock::format_ms(1_000), "1970-01-01T00:00:01.000Z");
        assert_eq!(Clock::from_duration(Duration::from_secs(2)), 2_000);
        assert_eq!(Clock::to_duration(2_000), Duration::from_secs(2));
    }
}
