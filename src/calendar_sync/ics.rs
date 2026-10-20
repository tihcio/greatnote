//! Lettura di calendari iCalendar (RFC 5545) da un indirizzo di
//! abbonamento: eventi singoli e ricorrenti espansi nella finestra
//! sincronizzata, con eccezioni (`EXDATE`) e occorrenze modificate
//! (`RECURRENCE-ID`). Supporta il sottoinsieme di `RRULE` usato dai
//! calendari reali (Google, Outlook, Apple); le regole non supportate
//! producono solo la prima occorrenza invece di date sbagliate.

use std::collections::{HashMap, HashSet};

use jiff::{
    SignedDuration, Timestamp, ToSpan,
    civil::{Date, DateTime, Weekday},
    tz::TimeZone,
};
use sha2::{Digest, Sha256};

use super::RemoteEvent;
use crate::time;

/// Occorrenze massime generate da un calendario (protezione da file enormi
/// o regole patologiche).
const MAX_OCCURRENCES: usize = 20_000;
/// Periodi esaminati al massimo per una singola regola.
const MAX_PERIODS: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcsCalendar {
    /// Nome del calendario (`X-WR-CALNAME`), se presente.
    pub name: Option<String>,
    pub events: Vec<RemoteEvent>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IcsError {
    #[error("il contenuto non è un calendario iCal")]
    NotCalendar,
}

// ---------------------------------------------------------------------------
// Righe e proprietà

struct Property {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Property {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Righe logiche: le righe che iniziano con spazio o tab continuano la
/// precedente (RFC 5545 §3.1).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !raw.is_empty() {
            lines.push(raw.to_owned());
        }
    }
    lines
}

fn parse_property(line: &str) -> Option<Property> {
    // Il valore inizia al primo ':' fuori dalle virgolette dei parametri.
    let mut quoted = false;
    let mut split = None;
    for (index, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                split = Some(index);
                break;
            }
            _ => {}
        }
    }
    let split = split?;
    let (head, value) = (&line[..split], &line[split + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((
                key.trim().to_ascii_uppercase(),
                value.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect();
    Some(Property {
        name,
        params,
        value: value.to_owned(),
    })
}

/// Testo con le sequenze di escape di iCalendar.
fn unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => output.push('\n'),
                Some(other) => output.push(other),
                None => {}
            }
        } else {
            output.push(c);
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Date e fusi

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum IcsTime {
    Date(Date),
    Utc(Timestamp),
    Local(DateTime),
}

/// Nomi di fuso di Windows usati da Outlook/Exchange → IANA (i più comuni,
/// dalla tabella CLDR `windowsZones`).
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("UTC", "UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("Romance Standard Time", "Europe/Paris"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Kiev"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("India Standard Time", "Asia/Kolkata"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Canada Central Standard Time", "America/Regina"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
    ("Pacific SA Standard Time", "America/Santiago"),
];

/// Fuso di un `TZID`: nome IANA, alias dichiarato nel `VTIMEZONE`
/// (`X-LIC-LOCATION`), nome Windows noto, altrimenti il fuso predefinito.
fn resolve_zone(
    tzid: Option<&str>,
    aliases: &HashMap<String, String>,
    default: &TimeZone,
) -> TimeZone {
    let Some(tzid) = tzid.map(|id| id.trim().trim_start_matches('/')) else {
        return default.clone();
    };
    let candidates = [
        Some(tzid),
        aliases.get(tzid).map(String::as_str),
        WINDOWS_ZONES
            .iter()
            .find(|(windows, _)| windows.eq_ignore_ascii_case(tzid))
            .map(|(_, iana)| *iana),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|name| TimeZone::get(name).ok())
        .unwrap_or_else(|| default.clone())
}

fn parse_time(property: &Property) -> Option<IcsTime> {
    let value = property.value.trim();
    // Primo valore se l'elenco ne contiene più d'uno.
    let value = value.split(',').next()?.trim();
    if property
        .param("VALUE")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE"))
        || value.len() == 8
    {
        return Date::strptime("%Y%m%d", value).ok().map(IcsTime::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        return DateTime::strptime("%Y%m%dT%H%M%S", utc)
            .ok()
            .and_then(|dt| TimeZone::UTC.to_ambiguous_timestamp(dt).compatible().ok())
            .map(IcsTime::Utc);
    }
    DateTime::strptime("%Y%m%dT%H%M%S", value)
        .ok()
        .map(IcsTime::Local)
}

/// Tutte le date di un elenco (`EXDATE:20261020T090000Z,20261027T090000Z`).
fn parse_times(property: &Property) -> Vec<IcsTime> {
    property
        .value
        .split(',')
        .filter_map(|value| {
            parse_time(&Property {
                name: property.name.clone(),
                params: property.params.clone(),
                value: value.to_owned(),
            })
        })
        .collect()
}

/// Durata ISO 8601 di iCalendar (`P1D`, `PT1H30M`, `-P1W`).
fn parse_duration(value: &str) -> Option<SignedDuration> {
    let value = value.trim();
    let (negative, value) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let value = value.strip_prefix('P')?;
    let mut seconds: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    for c in value.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let amount: i64 = number.parse().ok()?;
                number.clear();
                seconds += amount
                    * match (unit, in_time) {
                        ('W', false) => 7 * 86_400,
                        ('D', false) => 86_400,
                        ('H', true) => 3_600,
                        ('M', true) => 60,
                        ('S', true) => 1,
                        _ => return None,
                    };
            }
        }
    }
    let duration = SignedDuration::from_secs(seconds);
    Some(if negative { -duration } else { duration })
}

// ---------------------------------------------------------------------------
// Regole di ricorrenza

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    frequency: Frequency,
    interval: i64,
    count: Option<usize>,
    until: Option<IcsTime>,
    by_day: Vec<(Option<i8>, Weekday)>,
    by_month_day: Vec<i8>,
    by_month: Vec<i8>,
    by_set_pos: Vec<i32>,
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// `None` se la regola usa parti non supportate.
fn parse_rule(value: &str) -> Option<Rule> {
    let mut rule = Rule {
        frequency: Frequency::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
    };
    let mut frequency = None;
    for part in value.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let list = || value.split(',').map(str::trim);
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                frequency = Some(match value.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Frequency::Daily,
                    "WEEKLY" => Frequency::Weekly,
                    "MONTHLY" => Frequency::Monthly,
                    "YEARLY" => Frequency::Yearly,
                    _ => return None,
                })
            }
            "INTERVAL" => rule.interval = value.trim().parse().ok().filter(|n| *n >= 1)?,
            "COUNT" => rule.count = Some(value.trim().parse().ok()?),
            "UNTIL" => {
                rule.until = parse_time(&Property {
                    name: "UNTIL".to_owned(),
                    params: Vec::new(),
                    value: value.trim().to_owned(),
                })
            }
            "BYDAY" => {
                for item in list() {
                    let item = item.to_ascii_uppercase();
                    let (ordinal, code) = item.split_at(item.len().checked_sub(2)?);
                    let ordinal = if ordinal.is_empty() {
                        None
                    } else {
                        Some(ordinal.parse::<i8>().ok().filter(|n| *n != 0)?)
                    };
                    rule.by_day.push((ordinal, weekday(code)?));
                }
            }
            "BYMONTHDAY" => {
                for item in list() {
                    rule.by_month_day.push(
                        item.parse()
                            .ok()
                            .filter(|n: &i8| *n != 0 && n.abs() <= 31)?,
                    );
                }
            }
            "BYMONTH" => {
                for item in list() {
                    rule.by_month
                        .push(item.parse().ok().filter(|n: &i8| (1..=12).contains(n))?);
                }
            }
            "BYSETPOS" => {
                for item in list() {
                    rule.by_set_pos
                        .push(item.parse().ok().filter(|n: &i32| *n != 0)?);
                }
            }
            "WKST" => {}
            // BYHOUR, BYMINUTE, BYWEEKNO, BYYEARDAY…: non supportati.
            _ => return None,
        }
    }
    rule.frequency = frequency?;
    // Ordinali nei giorni della settimana solo dove hanno un significato
    // supportato (mese, o anno con BYMONTH).
    let ordinal = rule.by_day.iter().any(|(ordinal, _)| ordinal.is_some());
    if ordinal
        && !matches!(rule.frequency, Frequency::Monthly)
        && !(rule.frequency == Frequency::Yearly && !rule.by_month.is_empty())
    {
        return None;
    }
    Some(rule)
}

/// Giorni candidati di un mese secondo BYMONTHDAY/BYDAY (o il giorno
/// iniziale).
fn month_candidates(year: i16, month: i8, rule: &Rule, start: Date) -> Vec<Date> {
    let Ok(first) = Date::new(year, month, 1) else {
        return Vec::new();
    };
    let days = first.days_in_month();
    let by_month_day: Vec<Date> = rule
        .by_month_day
        .iter()
        .filter_map(|day| {
            let day = if *day > 0 { *day } else { days + 1 + *day };
            Date::new(year, month, day).ok()
        })
        .collect();
    let by_day: Vec<Date> = rule
        .by_day
        .iter()
        .flat_map(|(ordinal, weekday)| match ordinal {
            Some(nth) => first
                .nth_weekday_of_month(*nth, *weekday)
                .ok()
                .into_iter()
                .collect::<Vec<_>>(),
            None => (1..=days)
                .filter_map(|day| Date::new(year, month, day).ok())
                .filter(|date| date.weekday() == *weekday)
                .collect(),
        })
        .collect();
    let mut dates = match (rule.by_month_day.is_empty(), rule.by_day.is_empty()) {
        (true, true) => Date::new(year, month, start.day())
            .ok()
            .into_iter()
            .collect(),
        (false, true) => by_month_day,
        (true, false) => by_day,
        (false, false) => by_month_day
            .into_iter()
            .filter(|date| by_day.contains(date))
            .collect(),
    };
    dates.sort();
    dates.dedup();
    dates
}

fn apply_set_pos(dates: Vec<Date>, positions: &[i32]) -> Vec<Date> {
    if positions.is_empty() {
        return dates;
    }
    let len = dates.len() as i32;
    let mut selected: Vec<Date> = positions
        .iter()
        .filter_map(|position| {
            let index = if *position > 0 {
                position - 1
            } else {
                len + position
            };
            usize::try_from(index)
                .ok()
                .and_then(|index| dates.get(index).copied())
        })
        .collect();
    selected.sort();
    selected.dedup();
    selected
}

/// Date delle occorrenze tra `first` e `last` (inclusi), contando dalla data
/// iniziale per COUNT. Senza COUNT i periodi precedenti a `first` vengono
/// saltati invece di essere generati.
fn occurrence_dates(start: Date, rule: &Rule, first: Date, last: Date) -> Vec<Date> {
    let mut result = Vec::new();
    let in_month = |date: &Date| rule.by_month.is_empty() || rule.by_month.contains(&date.month());
    let week_start = start
        .checked_sub((start.weekday().to_monday_zero_offset() as i64).days())
        .unwrap_or(start);
    let month_index = |date: Date| date.year() as i64 * 12 + date.month() as i64 - 1;
    // Inizio del periodo n (giorno, lunedì, primo del mese o 1° gennaio).
    let period_start = |n: i64| -> Option<Date> {
        let step = n * rule.interval;
        match rule.frequency {
            Frequency::Daily => start.checked_add(step.days()).ok(),
            Frequency::Weekly => week_start.checked_add((step * 7).days()).ok(),
            Frequency::Monthly => {
                let index = month_index(start) + step;
                Date::new(
                    i16::try_from(index.div_euclid(12)).ok()?,
                    (index.rem_euclid(12) + 1) as i8,
                    1,
                )
                .ok()
            }
            Frequency::Yearly => {
                Date::new(i16::try_from(start.year() as i64 + step).ok()?, 1, 1).ok()
            }
        }
    };
    let mut period = 0_i64;
    if rule.count.is_none() && first > start {
        // Salta ai periodi vicini a `first` (uno prima, per sicurezza).
        let units = match rule.frequency {
            Frequency::Daily => first.duration_since(start).as_hours() / 24,
            Frequency::Weekly => first.duration_since(week_start).as_hours() / (24 * 7),
            Frequency::Monthly => month_index(first) - month_index(start),
            Frequency::Yearly => first.year() as i64 - start.year() as i64,
        };
        period = (units / rule.interval - 1).max(0);
    }
    let mut generated = 0_usize;
    for _ in 0..MAX_PERIODS {
        let Some(begin) = period_start(period) else {
            break;
        };
        if begin > last {
            break;
        }
        let mut candidates: Vec<Date> = match rule.frequency {
            Frequency::Daily => {
                let day = begin;
                let weekday_ok = rule.by_day.is_empty()
                    || rule
                        .by_day
                        .iter()
                        .any(|(_, weekday)| *weekday == day.weekday());
                let month_day_ok = rule.by_month_day.is_empty() || {
                    let days = day.days_in_month();
                    rule.by_month_day.iter().any(|value| {
                        if *value > 0 {
                            *value == day.day()
                        } else {
                            days + 1 + *value == day.day()
                        }
                    })
                };
                if weekday_ok && month_day_ok && in_month(&day) {
                    vec![day]
                } else {
                    Vec::new()
                }
            }
            Frequency::Weekly => if rule.by_day.is_empty() {
                vec![start.weekday()]
            } else {
                rule.by_day.iter().map(|(_, weekday)| *weekday).collect()
            }
            .into_iter()
            .filter_map(|weekday| {
                begin
                    .checked_add((weekday.to_monday_zero_offset() as i64).days())
                    .ok()
            })
            .filter(in_month)
            .collect(),
            Frequency::Monthly if in_month(&begin) => apply_set_pos(
                month_candidates(begin.year(), begin.month(), rule, start),
                &rule.by_set_pos,
            ),
            Frequency::Monthly => Vec::new(),
            Frequency::Yearly => {
                let months: Vec<i8> = if rule.by_month.is_empty() {
                    vec![start.month()]
                } else {
                    rule.by_month.clone()
                };
                let mut dates: Vec<Date> = months
                    .iter()
                    .flat_map(|month| month_candidates(begin.year(), *month, rule, start))
                    .collect();
                dates.sort();
                apply_set_pos(dates, &rule.by_set_pos)
            }
        };
        candidates.sort();
        candidates.dedup();
        for date in candidates {
            if date < start {
                continue;
            }
            if date > last || rule.count.is_some_and(|count| generated >= count) {
                return result;
            }
            generated += 1;
            if date >= first {
                result.push(date);
                if result.len() >= MAX_OCCURRENCES {
                    return result;
                }
            }
        }
        period += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Eventi

#[derive(Default)]
struct RawEvent {
    uid: Option<String>,
    summary: String,
    description: String,
    start: Option<(IcsTime, Option<String>)>,
    end: Option<(IcsTime, Option<String>)>,
    duration: Option<SignedDuration>,
    rule: Option<String>,
    exdates: Vec<(IcsTime, Option<String>)>,
    recurrence_id: Option<(IcsTime, Option<String>)>,
    cancelled: bool,
    url: Option<String>,
}

/// Inizio e fine concreti di un evento: date (fine inclusa) o istanti UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    AllDay(Date, Date),
    Timed(Timestamp, Timestamp),
}

struct Resolver<'a> {
    aliases: &'a HashMap<String, String>,
    default: &'a TimeZone,
}

impl Resolver<'_> {
    fn zone(&self, tzid: Option<&str>) -> TimeZone {
        resolve_zone(tzid, self.aliases, self.default)
    }

    fn instant(&self, time: IcsTime, tzid: Option<&str>) -> Option<Timestamp> {
        match time {
            IcsTime::Utc(timestamp) => Some(timestamp),
            IcsTime::Local(dt) => self.zone(tzid).to_ambiguous_timestamp(dt).compatible().ok(),
            IcsTime::Date(date) => self
                .default
                .to_ambiguous_timestamp(date.at(0, 0, 0, 0))
                .compatible()
                .ok(),
        }
    }

    /// Chiave di un'occorrenza, uguale per `RECURRENCE-ID`, `EXDATE` e
    /// occorrenze generate.
    fn key(&self, time: IcsTime, tzid: Option<&str>) -> Option<String> {
        match time {
            IcsTime::Date(date) => Some(date.strftime("%Y%m%d").to_string()),
            other => self
                .instant(other, tzid)
                .map(|t| t.strftime("%Y%m%dT%H%M%SZ").to_string()),
        }
    }
}

fn stable_id(uid: &str, key: Option<&str>) -> String {
    let base = if uid.len() > 300 {
        format!("{:x}", Sha256::digest(uid.as_bytes()))
    } else {
        uid.to_owned()
    };
    match key {
        Some(key) => format!("{base}@{key}"),
        None => base,
    }
}

fn remote_event(
    raw: &RawEvent,
    uid: &str,
    key: Option<&str>,
    span: Span,
    recurring: bool,
    zone_name: Option<String>,
) -> RemoteEvent {
    let (all_day, start_at, end_at, start_date, end_date) = match span {
        Span::AllDay(start, end) => (
            true,
            None,
            None,
            Some(start.to_string()),
            Some(end.to_string()),
        ),
        Span::Timed(start, end) => (
            false,
            Some(time::format_utc(start)),
            Some(time::format_utc(end)),
            None,
            None,
        ),
    };
    RemoteEvent {
        remote_id: stable_id(uid, key),
        etag: None,
        title: raw.summary.clone(),
        description: raw.description.clone(),
        all_day,
        start_at,
        end_at,
        start_date,
        end_date,
        timezone: zone_name,
        recurring,
        web_link: raw.url.clone().filter(|url| url.starts_with("https://")),
    }
}

/// Legge un calendario ed espande gli eventi che intersecano `from..to`.
/// `default_zone` vale per gli orari senza fuso e per i giorni interi.
pub fn parse_calendar(
    text: &str,
    from: Timestamp,
    to: Timestamp,
    default_zone: &TimeZone,
) -> Result<IcsCalendar, IcsError> {
    let lines = unfold(text);
    if !lines
        .iter()
        .any(|line| line.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR"))
    {
        return Err(IcsError::NotCalendar);
    }
    let mut name = None;
    let mut aliases = HashMap::new();
    let mut raw_events: Vec<RawEvent> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<RawEvent> = None;
    let mut current_tzid: Option<String> = None;
    for line in &lines {
        let Some(property) = parse_property(line) else {
            continue;
        };
        match property.name.as_str() {
            "BEGIN" => {
                let component = property.value.trim().to_ascii_uppercase();
                if component == "VEVENT" && stack.last().map(String::as_str) == Some("VCALENDAR") {
                    current = Some(RawEvent::default());
                }
                stack.push(component);
                continue;
            }
            "END" => {
                let component = property.value.trim().to_ascii_uppercase();
                if component == "VEVENT"
                    && let Some(event) = current.take()
                {
                    raw_events.push(event);
                }
                if component == "VTIMEZONE" {
                    current_tzid = None;
                }
                stack.pop();
                continue;
            }
            _ => {}
        }
        let inside = stack.last().map(String::as_str);
        match (inside, property.name.as_str()) {
            (Some("VCALENDAR"), "X-WR-CALNAME") => {
                name = Some(unescape(&property.value).trim().to_owned())
            }
            (Some("VTIMEZONE"), "TZID") => current_tzid = Some(property.value.trim().to_owned()),
            (Some("VTIMEZONE"), "X-LIC-LOCATION") => {
                if let Some(tzid) = &current_tzid {
                    aliases.insert(tzid.clone(), property.value.trim().to_owned());
                }
            }
            (Some("VEVENT"), field) => {
                let Some(event) = current.as_mut() else {
                    continue;
                };
                let tzid = property.param("TZID").map(str::to_owned);
                match field {
                    "UID" => event.uid = Some(property.value.trim().to_owned()),
                    "SUMMARY" => event.summary = unescape(&property.value),
                    "DESCRIPTION" => event.description = unescape(&property.value),
                    "DTSTART" => event.start = parse_time(&property).map(|time| (time, tzid)),
                    "DTEND" => event.end = parse_time(&property).map(|time| (time, tzid)),
                    "DURATION" => event.duration = parse_duration(&property.value),
                    "RRULE" => event.rule = Some(property.value.trim().to_owned()),
                    "EXDATE" => event.exdates.extend(
                        parse_times(&property)
                            .into_iter()
                            .map(|time| (time, tzid.clone())),
                    ),
                    "RECURRENCE-ID" => {
                        event.recurrence_id = parse_time(&property).map(|time| (time, tzid))
                    }
                    "STATUS" => {
                        event.cancelled = property.value.trim().eq_ignore_ascii_case("CANCELLED")
                    }
                    "URL" => event.url = Some(property.value.trim().to_owned()),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let resolver = Resolver {
        aliases: &aliases,
        default: default_zone,
    };
    let from_date = default_zone.to_datetime(from).date();
    let to_date = default_zone.to_datetime(to).date();
    let intersects = |span: Span| match span {
        Span::AllDay(start, end) => start <= to_date && end >= from_date,
        Span::Timed(start, end) => start < to && (end > from || start >= from),
    };

    // Occorrenze modificate: sostituiscono quella generata con la stessa chiave.
    let mut overridden: HashSet<(String, String)> = HashSet::new();
    for event in &raw_events {
        if let (Some(uid), Some((time, tzid))) = (&event.uid, &event.recurrence_id)
            && let Some(key) = resolver.key(*time, tzid.as_deref())
        {
            overridden.insert((uid.clone(), key));
        }
    }

    let mut events = Vec::new();
    for raw in &raw_events {
        if events.len() >= MAX_OCCURRENCES {
            break;
        }
        let (Some(uid), Some((start, start_tz))) = (raw.uid.as_deref(), raw.start.as_ref()) else {
            continue;
        };
        let start_tz = start_tz.as_deref();
        let zone = resolver.zone(start_tz);
        let zone_name = zone.iana_name().map(str::to_owned);
        // Durata: da DTEND o DURATION; giorno intero senza fine = 1 giorno.
        let span_of = |begin: IcsTime| -> Option<Span> {
            match begin {
                IcsTime::Date(date) => {
                    let days = match (&raw.end, raw.duration) {
                        (Some((IcsTime::Date(end), _)), _) => {
                            let IcsTime::Date(original) = *start else {
                                return None;
                            };
                            (end.duration_since(original).as_hours() / 24).max(1)
                        }
                        (_, Some(duration)) => (duration.as_hours() / 24).max(1),
                        _ => 1,
                    };
                    let last = date.checked_add((days - 1).days()).ok()?;
                    Some(Span::AllDay(date, last))
                }
                other => {
                    let begin_instant = resolver.instant(other, start_tz)?;
                    let length = match (&raw.end, raw.duration) {
                        (Some((end, end_tz)), _) => {
                            let original = resolver.instant(*start, start_tz)?;
                            resolver
                                .instant(*end, end_tz.as_deref())?
                                .duration_since(original)
                        }
                        (None, Some(duration)) => duration,
                        (None, None) => SignedDuration::ZERO,
                    };
                    let end_instant = begin_instant
                        .checked_add(length.max(SignedDuration::ZERO))
                        .ok()?;
                    Some(Span::Timed(begin_instant, end_instant))
                }
            }
        };

        // Occorrenza modificata di una serie.
        if let Some((time, tzid)) = &raw.recurrence_id {
            if raw.cancelled {
                continue;
            }
            let key = resolver.key(*time, tzid.as_deref());
            if let Some(span) = span_of(*start).filter(|span| intersects(*span)) {
                events.push(remote_event(
                    raw,
                    uid,
                    key.as_deref(),
                    span,
                    true,
                    zone_name.clone(),
                ));
            }
            continue;
        }
        if raw.cancelled {
            continue;
        }
        let rule = raw.rule.as_deref().and_then(parse_rule);
        let Some(rule) = rule else {
            // Evento singolo (o regola non supportata: solo la prima volta).
            if let Some(span) = span_of(*start).filter(|span| intersects(*span)) {
                events.push(remote_event(
                    raw,
                    uid,
                    None,
                    span,
                    raw.rule.is_some(),
                    zone_name.clone(),
                ));
            }
            continue;
        };
        let excluded: HashSet<String> = raw
            .exdates
            .iter()
            .filter_map(|(time, tzid)| resolver.key(*time, tzid.as_deref()))
            .collect();
        let until = rule.until.and_then(|until| match until {
            IcsTime::Date(date) => Some(Span::AllDay(date, date)),
            other => resolver.instant(other, start_tz).map(|t| Span::Timed(t, t)),
        });
        let (start_date, time_of_day) = match start {
            IcsTime::Date(date) => (*date, None),
            IcsTime::Utc(timestamp) => {
                let local = TimeZone::UTC.to_datetime(*timestamp);
                (local.date(), Some((local.time(), true)))
            }
            IcsTime::Local(dt) => (dt.date(), Some((dt.time(), false))),
        };
        let mut last = to_date.checked_add(1.day()).unwrap_or(to_date);
        if let Some(Span::AllDay(date, _)) = until {
            last = last.min(date);
        }
        // Un giorno di margine: le occorrenze lunghe iniziate prima della
        // finestra vengono filtrate da `intersects`.
        let first = from_date.checked_sub(1.day()).unwrap_or(from_date);
        for date in occurrence_dates(start_date, &rule, first, last) {
            let begin = match time_of_day {
                None => IcsTime::Date(date),
                Some((clock, true)) => match TimeZone::UTC
                    .to_ambiguous_timestamp(date.to_datetime(clock))
                    .compatible()
                {
                    Ok(timestamp) => IcsTime::Utc(timestamp),
                    Err(_) => continue,
                },
                Some((clock, false)) => IcsTime::Local(date.to_datetime(clock)),
            };
            let Some(key) = resolver.key(begin, start_tz) else {
                continue;
            };
            if excluded.contains(&key) || overridden.contains(&(uid.to_owned(), key.clone())) {
                continue;
            }
            let Some(span) = span_of(begin) else { continue };
            if let (Some(Span::Timed(limit, _)), Span::Timed(occurrence, _)) = (until, span)
                && occurrence > limit
            {
                break;
            }
            if intersects(span) {
                events.push(remote_event(
                    raw,
                    uid,
                    Some(&key),
                    span,
                    true,
                    zone_name.clone(),
                ));
                if events.len() >= MAX_OCCURRENCES {
                    break;
                }
            }
        }
    }
    Ok(IcsCalendar {
        name: name.filter(|name| !name.is_empty()),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rome() -> TimeZone {
        TimeZone::get("Europe/Rome").expect("zone")
    }

    fn window() -> (Timestamp, Timestamp) {
        (
            "2026-01-01T00:00:00Z".parse().expect("from"),
            "2027-01-01T00:00:00Z".parse().expect("to"),
        )
    }

    fn calendar(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Lavoro\r\n{body}END:VCALENDAR\r\n")
    }

    fn parse(body: &str) -> IcsCalendar {
        let (from, to) = window();
        parse_calendar(&calendar(body), from, to, &rome()).expect("parse")
    }

    fn starts(calendar: &IcsCalendar, title: &str) -> Vec<String> {
        calendar
            .events
            .iter()
            .filter(|event| event.title == title)
            .map(|event| {
                event
                    .start_at
                    .clone()
                    .or(event.start_date.clone())
                    .unwrap_or_default()
            })
            .collect()
    }

    #[test]
    fn single_events_with_folding_escapes_and_zones() {
        let parsed = parse(
            "BEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Riunione\\, budget\r\nDESCRIPTION:Riga uno\\nriga \r\n due\r\nDTSTART;TZID=Europe/Rome:20260329T093000\r\nDTEND;TZID=Europe/Rome:20260329T103000\r\nURL:https://example.com/e\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:b@x\r\nSUMMARY:Ferie\r\nDTSTART;VALUE=DATE:20260810\r\nDTEND;VALUE=DATE:20260815\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:c@x\r\nSUMMARY:Outlook\r\nDTSTART;TZID=W. Europe Standard Time:20261025T090000\r\nDURATION:PT1H30M\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:d@x\r\nSUMMARY:Annullato\r\nSTATUS:CANCELLED\r\nDTSTART:20260601T080000Z\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:e@x\r\nSUMMARY:Vecchio\r\nDTSTART:20200101T080000Z\r\nEND:VEVENT\r\n",
        );
        assert_eq!(parsed.name.as_deref(), Some("Lavoro"));
        assert_eq!(parsed.events.len(), 3);
        let meeting = &parsed.events[0];
        assert_eq!(meeting.title, "Riunione, budget");
        assert_eq!(meeting.description, "Riga uno\nriga due");
        // 29 marzo 2026: già ora legale (UTC+2).
        assert_eq!(meeting.start_at.as_deref(), Some("2026-03-29T07:30:00Z"));
        assert_eq!(meeting.end_at.as_deref(), Some("2026-03-29T08:30:00Z"));
        assert_eq!(meeting.web_link.as_deref(), Some("https://example.com/e"));
        assert_eq!(meeting.remote_id, "a@x");
        let holiday = &parsed.events[1];
        assert!(holiday.all_day);
        assert_eq!(
            (holiday.start_date.as_deref(), holiday.end_date.as_deref()),
            (Some("2026-08-10"), Some("2026-08-14"))
        );
        // 25 ottobre 2026: torna l'ora solare (UTC+1).
        let outlook = &parsed.events[2];
        assert_eq!(outlook.start_at.as_deref(), Some("2026-10-25T08:00:00Z"));
        assert_eq!(outlook.end_at.as_deref(), Some("2026-10-25T09:30:00Z"));
        assert_eq!(outlook.timezone.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn weekly_rule_keeps_local_time_across_dst_with_exceptions_and_overrides() {
        let parsed = parse(
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Rome:20261019T090000\r\nDTEND;TZID=Europe/Rome:20261019T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5\r\nEXDATE;TZID=Europe/Rome:20261021T090000\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:standup\r\nRECURRENCE-ID;TZID=Europe/Rome:20261026T090000\r\nSUMMARY:Standup spostato\r\nDTSTART;TZID=Europe/Rome:20261026T110000\r\nDTEND;TZID=Europe/Rome:20261026T111500\r\nEND:VEVENT\r\n",
        );
        // 19 (lun), 21 (mer, escluso), 26 (lun, spostato), 28 (mer), 2/11 (lun).
        assert_eq!(
            starts(&parsed, "Standup"),
            [
                "2026-10-19T07:00:00Z",
                "2026-10-28T08:00:00Z",
                "2026-11-02T08:00:00Z"
            ]
        );
        assert_eq!(
            starts(&parsed, "Standup spostato"),
            ["2026-10-26T10:00:00Z"]
        );
        assert!(parsed.events.iter().all(|event| event.recurring));
        let ids: HashSet<&str> = parsed
            .events
            .iter()
            .map(|event| event.remote_id.as_str())
            .collect();
        assert_eq!(ids.len(), parsed.events.len());
        assert!(ids.contains("standup@20261026T080000Z"));
    }

    #[test]
    fn monthly_yearly_and_daily_rules() {
        let parsed = parse(
            "BEGIN:VEVENT\r\nUID:m\r\nSUMMARY:Ultimo venerdì\r\nDTSTART;VALUE=DATE:20260130\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;UNTIL=20260430\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:y\r\nSUMMARY:Compleanno\r\nDTSTART;VALUE=DATE:20000229\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Ogni due giorni\r\nDTSTART:20261228T080000Z\r\nRRULE:FREQ=DAILY;INTERVAL=2\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:p\r\nSUMMARY:Ultimo giorno lavorativo\r\nDTSTART;VALUE=DATE:20260130\r\nRRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=3\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:Non supportata\r\nDTSTART:20260105T080000Z\r\nRRULE:FREQ=HOURLY;COUNT=3\r\nEND:VEVENT\r\n",
        );
        assert_eq!(
            starts(&parsed, "Ultimo venerdì"),
            ["2026-01-30", "2026-02-27", "2026-03-27", "2026-04-24"]
        );
        // Il 29 febbraio non esiste nel 2026: nessuna occorrenza.
        assert!(starts(&parsed, "Compleanno").is_empty());
        assert_eq!(
            starts(&parsed, "Ogni due giorni"),
            ["2026-12-28T08:00:00Z", "2026-12-30T08:00:00Z"]
        );
        assert_eq!(
            starts(&parsed, "Ultimo giorno lavorativo"),
            ["2026-01-30", "2026-02-27", "2026-03-31"]
        );
        assert_eq!(starts(&parsed, "Non supportata"), ["2026-01-05T08:00:00Z"]);
    }

    #[test]
    fn google_vtimezone_alias_and_rejections() {
        let parsed = parse(
            "BEGIN:VTIMEZONE\r\nTZID:Ora di Roma\r\nX-LIC-LOCATION:Europe/Rome\r\nEND:VTIMEZONE\r\n\
             BEGIN:VEVENT\r\nUID:z\r\nSUMMARY:Alias\r\nDTSTART;TZID=Ora di Roma:20260715T100000\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nSUMMARY:Senza UID\r\nDTSTART:20260715T100000Z\r\nEND:VEVENT\r\n",
        );
        assert_eq!(starts(&parsed, "Alias"), ["2026-07-15T08:00:00Z"]);
        assert!(starts(&parsed, "Senza UID").is_empty());
        let (from, to) = window();
        assert_eq!(
            parse_calendar("<html>login</html>", from, to, &rome()),
            Err(IcsError::NotCalendar)
        );
        assert_eq!(
            parse_duration("P1W2DT3H"),
            Some(SignedDuration::from_secs(9 * 86_400 + 3 * 3_600))
        );
        assert_eq!(
            parse_duration("-PT15M"),
            Some(SignedDuration::from_secs(-900))
        );
        assert_eq!(parse_duration("1H"), None);
        assert!(parse_rule("FREQ=WEEKLY;BYHOUR=9").is_none());
        assert!(parse_rule("FREQ=WEEKLY;BYDAY=2MO").is_none());
    }

    #[test]
    fn huge_rules_are_bounded() {
        let (from, _) = window();
        let to: Timestamp = "2100-01-01T00:00:00Z".parse().expect("to");
        let text = calendar(
            "BEGIN:VEVENT\r\nUID:h\r\nSUMMARY:Sempre\r\nDTSTART:19000101T000000Z\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\n",
        );
        let parsed = parse_calendar(&text, from, to, &rome()).expect("parse");
        assert!(!parsed.events.is_empty() && parsed.events.len() <= MAX_OCCURRENCES);
        // Le occorrenze partono dalla finestra, non dal 1900.
        assert!(
            parsed.events[0]
                .start_at
                .as_deref()
                .is_some_and(|start| start >= "2025-12-31")
        );
    }
}
