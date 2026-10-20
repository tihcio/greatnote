//! Date, orari e fusi orari.
//!
//! Regole:
//! - gli istanti (promemoria, eventi con orario) sono salvati in UTC con
//!   precisione al secondo, nel formato `AAAA-MM-GGTHH:MM:SSZ`, ordinabile
//!   come testo;
//! - le scadenze delle attività e gli eventi di un giorno intero sono date
//!   civili `AAAA-MM-GG`, senza fuso;
//! - un orario locale (`AAAA-MM-GGTHH:MM`) viene convertito con il fuso
//!   indicato o con quello del sistema. Durante il passaggio all'ora legale un
//!   orario inesistente viene spostato in avanti (02:30 → 03:30); durante il
//!   ritorno all'ora solare un orario ripetuto usa la prima occorrenza. È la
//!   strategia "compatible" di RFC 5545/Temporal.

use jiff::{
    Timestamp, ToSpan, Zoned,
    civil::{Date, DateTime, Time},
    tz::TimeZone,
};

use crate::storage::StorageError;

/// Tipi usati nelle API pubbliche, così i chiamanti non dipendono da `jiff`.
pub use jiff::{Timestamp as Instant, civil::Date as CivilDate};

/// Nome usato quando il fuso del sistema non ha un identificatore IANA.
const FALLBACK_TIMEZONE: &str = "UTC";

fn invalid(message: &str) -> StorageError {
    StorageError::InvalidDateTime(message.to_owned())
}

/// Fuso orario risolto, con il nome da salvare accanto agli istanti.
#[derive(Debug, Clone)]
pub struct Zone {
    pub tz: TimeZone,
    pub name: String,
}

impl Zone {
    pub fn system() -> Self {
        let tz = TimeZone::system();
        let name = tz.iana_name().unwrap_or(FALLBACK_TIMEZONE).to_owned();
        Self { tz, name }
    }

    /// Fuso indicato per nome IANA, oppure quello del sistema.
    pub fn resolve(name: Option<&str>) -> Result<Self, StorageError> {
        match name.map(str::trim).filter(|name| !name.is_empty()) {
            None => Ok(Self::system()),
            Some(name) if name.len() > 64 => Err(StorageError::InvalidTimeZone),
            Some(name) => TimeZone::get(name)
                .map(|tz| Self {
                    tz,
                    name: name.to_owned(),
                })
                .map_err(|_| StorageError::InvalidTimeZone),
        }
    }

    /// Fuso salvato con un dato; se non è più disponibile usa quello del
    /// sistema invece di fallire.
    pub fn stored(name: &str) -> Self {
        Self::resolve(Some(name)).unwrap_or_else(|_| Self::system())
    }
}

pub fn now() -> Timestamp {
    Timestamp::now()
}

pub fn format_utc(timestamp: Timestamp) -> String {
    timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn parse_utc(value: &str) -> Result<Timestamp, StorageError> {
    value
        .parse::<Timestamp>()
        .map_err(|_| invalid("istante non valido"))
}

pub fn parse_date(value: &str) -> Result<Date, StorageError> {
    if value.len() != 10 {
        return Err(invalid("usa una data AAAA-MM-GG"));
    }
    value
        .parse::<Date>()
        .map_err(|_| invalid("usa una data AAAA-MM-GG"))
}

/// Orario locale `AAAA-MM-GGTHH:MM` (il formato di `<input type=datetime-local>`).
pub fn parse_local(value: &str) -> Result<DateTime, StorageError> {
    let valid_shape = value.len() == 16 && value.as_bytes()[10] == b'T';
    if !valid_shape {
        return Err(invalid("usa un orario AAAA-MM-GGTHH:MM"));
    }
    value
        .parse::<DateTime>()
        .map_err(|_| invalid("usa un orario AAAA-MM-GGTHH:MM"))
}

pub fn local_to_zoned(local: DateTime, zone: &Zone) -> Result<Zoned, StorageError> {
    zone.tz
        .to_ambiguous_zoned(local)
        .compatible()
        .map_err(|_| invalid("orario fuori dall'intervallo supportato"))
}

pub fn local_to_utc(value: &str, zone: &Zone) -> Result<Timestamp, StorageError> {
    Ok(local_to_zoned(parse_local(value)?, zone)?.timestamp())
}

/// Orario locale effettivo di un istante, nel formato `AAAA-MM-GGTHH:MM`.
pub fn local_wall(timestamp: Timestamp, zone: &Zone) -> String {
    timestamp
        .to_zoned(zone.tz.clone())
        .strftime("%Y-%m-%dT%H:%M")
        .to_string()
}

pub fn today(now: Timestamp, zone: &Zone) -> Date {
    now.to_zoned(zone.tz.clone()).date()
}

/// Primo istante del giorno civile nel fuso (non sempre le 00:00: alcuni
/// fusi saltano la mezzanotte al cambio d'ora).
pub fn day_start(date: Date, zone: &Zone) -> Result<Timestamp, StorageError> {
    date.to_zoned(zone.tz.clone())
        .map(|zoned| zoned.timestamp())
        .map_err(|_| invalid("data fuori dall'intervallo supportato"))
}

pub fn next_day(date: Date) -> Result<Date, StorageError> {
    date.tomorrow()
        .map_err(|_| invalid("data fuori dall'intervallo supportato"))
}

pub fn add_minutes(timestamp: Timestamp, minutes: i64) -> Result<Timestamp, StorageError> {
    timestamp
        .checked_add(minutes.minutes())
        .map_err(|_| invalid("orario fuori dall'intervallo supportato"))
}

/// Domani alla stessa ora locale indicata, anche se nel frattempo cambia
/// l'ora legale.
pub fn tomorrow_at(now: Timestamp, time: Time, zone: &Zone) -> Result<Timestamp, StorageError> {
    let date = next_day(today(now, zone))?;
    Ok(local_to_zoned(date.to_datetime(time), zone)?.timestamp())
}

#[cfg(test)]
mod tests {
    use jiff::civil::time;

    use super::*;

    fn rome() -> Zone {
        Zone::resolve(Some("Europe/Rome")).expect("tzdata available")
    }

    #[test]
    fn local_times_convert_to_utc_in_winter_and_summer() {
        let zone = rome();
        assert_eq!(
            format_utc(local_to_utc("2026-01-15T09:00", &zone).expect("winter")),
            "2026-01-15T08:00:00Z"
        );
        assert_eq!(
            format_utc(local_to_utc("2026-07-15T09:00", &zone).expect("summer")),
            "2026-07-15T07:00:00Z"
        );
    }

    #[test]
    fn daylight_saving_gaps_and_folds_are_resolved_predictably() {
        let zone = rome();
        // 29 marzo 2026: alle 02:00 l'orologio passa alle 03:00.
        let gap = local_to_utc("2026-03-29T02:30", &zone).expect("gap");
        assert_eq!(format_utc(gap), "2026-03-29T01:30:00Z");
        assert_eq!(local_wall(gap, &zone), "2026-03-29T03:30");
        // 25 ottobre 2026: le 02:30 esistono due volte; vale la prima (CEST).
        let fold = local_to_utc("2026-10-25T02:30", &zone).expect("fold");
        assert_eq!(format_utc(fold), "2026-10-25T00:30:00Z");
        // Il giorno del cambio dura 25 ore.
        let start = day_start(parse_date("2026-10-25").expect("date"), &zone).expect("start");
        let end = day_start(parse_date("2026-10-26").expect("date"), &zone).expect("end");
        assert_eq!(end.as_second() - start.as_second(), 25 * 3600);
    }

    #[test]
    fn tomorrow_keeps_the_wall_clock_across_dst() {
        let zone = rome();
        let now = parse_utc("2026-10-24T07:00:00Z").expect("now");
        let next = tomorrow_at(now, time(9, 0, 0, 0), &zone).expect("tomorrow");
        // Il 25 ottobre torna l'ora solare: 09:00 locali sono le 08:00 UTC.
        assert_eq!(format_utc(next), "2026-10-25T08:00:00Z");
        assert_eq!(local_wall(next, &zone), "2026-10-25T09:00");
    }

    #[test]
    fn today_depends_on_the_time_zone() {
        let instant = parse_utc("2026-10-14T23:30:00Z").expect("instant");
        assert_eq!(today(instant, &rome()).to_string(), "2026-10-15");
        let new_york = Zone::resolve(Some("America/New_York")).expect("tz");
        assert_eq!(today(instant, &new_york).to_string(), "2026-10-14");
    }

    #[test]
    fn malformed_input_is_rejected() {
        let zone = rome();
        for value in [
            "",
            "2026-10-14",
            "2026-10-14 09:00",
            "2026-13-01T09:00",
            "2026-10-14T25:00",
        ] {
            assert!(local_to_utc(value, &zone).is_err(), "{value}");
        }
        for value in ["2026-02-30", "26-10-14", "2026-10-14T00:00"] {
            assert!(parse_date(value).is_err(), "{value}");
        }
        assert!(Zone::resolve(Some("Mars/Olympus")).is_err());
        assert!(Zone::resolve(Some("../../etc/passwd")).is_err());
        assert_eq!(Zone::stored("Mars/Olympus").name, Zone::system().name);
    }
}
