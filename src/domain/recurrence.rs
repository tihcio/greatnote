//! Ricorrenza delle attività: regola e calcolo della prossima scadenza.
//!
//! Sottoinsieme semplice di RRULE (RFC 5545), salvato come JSON nella
//! colonna `tasks.recurrence`:
//!
//! - `daily`, `weekly`, `monthly`, `yearly` con `interval` (ogni N);
//! - `weekly` con `weekdays` facoltativi (1 = lunedì … 7 = domenica);
//! - `monthly` e `yearly` con `month_day`, il giorno scelto all'inizio, così
//!   un'attività del 31 cade il 30 o il 28 nei mesi più corti e torna al 31
//!   dopo;
//! - `until` facoltativo (data civile, inclusa).
//!
//! Le date sono civili, senza fuso: il calcolo non dipende dall'ora legale.

use jiff::{ToSpan, civil::Date};
use serde::{Deserialize, Serialize};

pub const MAX_INTERVAL: u16 = 99;
/// Limite di sicurezza per regole che non producono più date utili.
const MAX_STEPS: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub frequency: Frequency,
    #[serde(default = "one")]
    pub interval: u16,
    /// Solo per `weekly`: giorni ISO 1–7, ordinati e senza ripetizioni.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weekdays: Vec<u8>,
    /// Solo per `monthly`/`yearly`: giorno del mese 1–31.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month_day: Option<u8>,
    /// Ultima data possibile (`AAAA-MM-GG`, inclusa).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

fn one() -> u16 {
    1
}

fn iso_weekday(date: Date) -> u8 {
    // `to_monday_one_offset` restituisce 1–7: la conversione non perde nulla.
    date.weekday().to_monday_one_offset() as u8
}

/// Stesso giorno del mese, o l'ultimo del mese se più corto.
fn clamp_day(year: i16, month: i8, day: u8) -> Option<Date> {
    let first = Date::new(year, month, 1).ok()?;
    let day = i8::try_from(day).ok()?.min(first.days_in_month());
    Date::new(year, month, day).ok()
}

impl Recurrence {
    /// Regola normalizzata e controllata per la scadenza `due` (obbligatoria:
    /// è il punto di partenza della serie).
    pub fn validated(mut self, due: Date) -> Result<Self, &'static str> {
        if !(1..=MAX_INTERVAL).contains(&self.interval) {
            return Err("intervallo della ripetizione non valido (da 1 a 99)");
        }
        match self.frequency {
            Frequency::Weekly => {
                if self.weekdays.iter().any(|day| !(1..=7).contains(day)) {
                    return Err("giorno della settimana non valido");
                }
                self.weekdays.sort_unstable();
                self.weekdays.dedup();
                self.month_day = None;
            }
            Frequency::Monthly | Frequency::Yearly => {
                self.weekdays.clear();
                let day = self.month_day.unwrap_or(due.day() as u8);
                if !(1..=31).contains(&day) {
                    return Err("giorno del mese non valido");
                }
                self.month_day = Some(day);
            }
            Frequency::Daily => {
                self.weekdays.clear();
                self.month_day = None;
            }
        }
        if let Some(until) = self.until.as_deref().map(str::trim) {
            if until.is_empty() {
                self.until = None;
            } else {
                let date: Date = until.parse().map_err(|_| "data di fine non valida")?;
                if date < due {
                    return Err("la ripetizione finisce prima della scadenza");
                }
                self.until = Some(date.to_string());
            }
        }
        Ok(self)
    }

    fn until_date(&self) -> Option<Date> {
        self.until.as_deref().and_then(|value| value.parse().ok())
    }

    /// Prima occorrenza dopo `due` (la scadenza attuale) e non prima di
    /// `not_before` (oggi): le scadenze saltate mentre l'attività era in
    /// ritardo non vengono ricreate. `None` se la serie è finita.
    pub fn next_due(&self, due: Date, not_before: Date) -> Option<Date> {
        let mut steps = 0;
        let mut candidate = self.step(due, due)?;
        while candidate < not_before {
            steps += 1;
            if steps > MAX_STEPS {
                return None;
            }
            candidate = self.step(due, candidate)?;
        }
        match self.until_date() {
            Some(until) if candidate > until => None,
            _ => Some(candidate),
        }
    }

    /// Occorrenza successiva a `current` nella serie che parte da `anchor`.
    fn step(&self, anchor: Date, current: Date) -> Option<Date> {
        let interval = i64::from(self.interval);
        match self.frequency {
            Frequency::Daily => current.checked_add(interval.days()).ok(),
            Frequency::Weekly if self.weekdays.is_empty() => {
                current.checked_add((interval * 7).days()).ok()
            }
            Frequency::Weekly => {
                // Settimane (da lunedì) contate dalla settimana di `anchor`:
                // solo quelle multiple dell'intervallo sono attive.
                let anchor_monday = anchor
                    .checked_sub(i64::from(iso_weekday(anchor) - 1).days())
                    .ok()?;
                let mut day = current;
                for _ in 0..(7 * interval + 7) {
                    day = day.checked_add(1.day()).ok()?;
                    let weeks = anchor_monday.until(day).ok()?.get_days() / 7;
                    if i64::from(weeks) % interval == 0 && self.weekdays.contains(&iso_weekday(day))
                    {
                        return Some(day);
                    }
                }
                None
            }
            Frequency::Monthly | Frequency::Yearly => {
                let months = if self.frequency == Frequency::Monthly {
                    interval
                } else {
                    interval * 12
                };
                let day = self.month_day.unwrap_or(anchor.day() as u8);
                // Mesi da `anchor` alla data corrente, poi il passo successivo:
                // il giorno resta quello della regola, non quello ridotto.
                let elapsed = i64::from(current.year() - anchor.year()) * 12
                    + i64::from(current.month() - anchor.month());
                let target = elapsed - elapsed.rem_euclid(months) + months;
                let total = i64::from(anchor.month() - 1) + target;
                let year = i16::try_from(i64::from(anchor.year()) + total.div_euclid(12)).ok()?;
                let month = i8::try_from(total.rem_euclid(12) + 1).ok()?;
                clamp_day(year, month, day)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> Date {
        value.parse().expect("date")
    }

    fn rule(frequency: Frequency, interval: u16) -> Recurrence {
        Recurrence {
            frequency,
            interval,
            weekdays: Vec::new(),
            month_day: None,
            until: None,
        }
    }

    #[test]
    fn daily_and_weekly_steps_skip_missed_dates() {
        let daily = rule(Frequency::Daily, 2);
        assert_eq!(
            daily.next_due(date("2026-10-20"), date("2026-10-20")),
            Some(date("2026-10-22"))
        );
        // In ritardo di una settimana: la prossima è da oggi in poi.
        assert_eq!(
            daily.next_due(date("2026-10-10"), date("2026-10-22")),
            Some(date("2026-10-22"))
        );
        let weekly = rule(Frequency::Weekly, 1);
        assert_eq!(
            weekly.next_due(date("2026-10-19"), date("2026-10-19")),
            Some(date("2026-10-26"))
        );
        assert_eq!(
            weekly.next_due(date("2026-10-05"), date("2026-10-22")),
            Some(date("2026-10-26"))
        );
    }

    #[test]
    fn weekly_on_chosen_days_every_other_week() {
        let mut every = rule(Frequency::Weekly, 1);
        every.weekdays = vec![1, 3, 5];
        // Mercoledì 21 ottobre 2026 → venerdì 23 → lunedì 26.
        assert_eq!(
            every.next_due(date("2026-10-21"), date("2026-10-21")),
            Some(date("2026-10-23"))
        );
        assert_eq!(
            every.next_due(date("2026-10-23"), date("2026-10-23")),
            Some(date("2026-10-26"))
        );
        let mut other = rule(Frequency::Weekly, 2);
        other.weekdays = vec![2];
        // Martedì 20 ottobre, ogni 2 settimane: 3 novembre.
        assert_eq!(
            other.next_due(date("2026-10-20"), date("2026-10-20")),
            Some(date("2026-11-03"))
        );
        let mut weekend = rule(Frequency::Weekly, 2);
        weekend.weekdays = vec![6, 7];
        // Da sabato 24: domenica 25 (stessa settimana), poi sabato 7 novembre.
        assert_eq!(
            weekend.next_due(date("2026-10-24"), date("2026-10-24")),
            Some(date("2026-10-25"))
        );
        assert_eq!(
            weekend.next_due(date("2026-10-25"), date("2026-10-25")),
            Some(date("2026-11-07"))
        );
    }

    #[test]
    fn monthly_keeps_the_chosen_day_across_short_months() {
        let monthly = rule(Frequency::Monthly, 1)
            .validated(date("2026-01-31"))
            .expect("valid");
        assert_eq!(monthly.month_day, Some(31));
        let february = monthly
            .next_due(date("2026-01-31"), date("2026-01-31"))
            .expect("next");
        assert_eq!(february, date("2026-02-28"));
        // Dal 28 febbraio si torna al 31 a marzo, non al 28.
        assert_eq!(
            monthly.next_due(february, february),
            Some(date("2026-03-31"))
        );
        assert_eq!(
            monthly.next_due(date("2026-03-31"), date("2026-03-31")),
            Some(date("2026-04-30"))
        );
        let quarterly = rule(Frequency::Monthly, 3)
            .validated(date("2026-11-15"))
            .expect("valid");
        assert_eq!(
            quarterly.next_due(date("2026-11-15"), date("2026-11-15")),
            Some(date("2027-02-15"))
        );
    }

    #[test]
    fn yearly_on_leap_day_and_until() {
        let mut yearly = rule(Frequency::Yearly, 1)
            .validated(date("2028-02-29"))
            .expect("valid");
        assert_eq!(
            yearly.next_due(date("2028-02-29"), date("2028-02-29")),
            Some(date("2029-02-28"))
        );
        assert_eq!(
            yearly.next_due(date("2031-02-28"), date("2031-02-28")),
            Some(date("2032-02-29"))
        );
        yearly.until = Some("2030-12-31".to_owned());
        assert_eq!(
            yearly.next_due(date("2030-02-28"), date("2030-02-28")),
            None
        );
    }

    #[test]
    fn invalid_rules_are_rejected_and_json_is_strict() {
        let due = date("2026-10-22");
        assert!(rule(Frequency::Daily, 0).validated(due).is_err());
        assert!(rule(Frequency::Daily, 100).validated(due).is_err());
        let mut weekly = rule(Frequency::Weekly, 1);
        weekly.weekdays = vec![8];
        assert!(weekly.validated(due).is_err());
        let mut ended = rule(Frequency::Daily, 1);
        ended.until = Some("2026-10-01".to_owned());
        assert!(ended.validated(due).is_err());
        let mut normalized = rule(Frequency::Weekly, 1);
        normalized.weekdays = vec![5, 1, 5];
        assert_eq!(
            normalized.validated(due).expect("valid").weekdays,
            vec![1, 5]
        );
        assert!(serde_json::from_str::<Recurrence>(r#"{"frequency":"daily","sql":"x"}"#).is_err());
        let parsed: Recurrence = serde_json::from_str(r#"{"frequency":"monthly"}"#).expect("json");
        assert_eq!(parsed.interval, 1);
    }
}
