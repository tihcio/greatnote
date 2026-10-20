//! Prova reale di un abbonamento iCal con un calendario pubblico (festività
//! italiane di Google, nessun dato personale). Ignorata di default:
//! `cargo test --test calendar_live -- --ignored --nocapture`
//! (facoltativo `GREATNOTE_ICS_URL` per un altro calendario).

use greatnote_core::{
    calendar_sync::subscriptions::{apply_ics, calendar_name, fetch_ics},
    storage::Database,
};

#[test]
#[ignore = "richiede la rete"]
fn public_holiday_calendar() {
    let url = std::env::var("GREATNOTE_ICS_URL").unwrap_or_else(|_| {
        "https://calendar.google.com/calendar/ical/it.italian%23holiday%40group.v.calendar.google.com/public/basic.ics"
            .to_owned()
    });
    let text = fetch_ics(&url).expect("fetch");
    println!("nome: {:?}, {} byte", calendar_name(&text), text.len());
    let database = Database::in_memory().expect("db");
    let id = database
        .add_calendar_subscription("Festività", None)
        .expect("add");
    let zone = jiff::tz::TimeZone::get("Europe/Rome").expect("zone");
    let report = apply_ics(&database, &id, &text, jiff::Timestamp::now(), &zone).expect("apply");
    println!("eventi: {report:?}");
    assert!(report.events > 0);
    let zone = greatnote_core::time::Zone::resolve(Some("Europe/Rome")).expect("zone");
    let today = jiff::Zoned::now().date();
    let events = database
        .list_events(
            today,
            today.checked_add(jiff::ToSpan::days(365)).expect("date"),
            &zone,
        )
        .expect("events");
    for event in events.iter().take(8) {
        println!(
            "  {} {}",
            event.start_date.as_deref().unwrap_or("?"),
            event.title
        );
    }
    assert!(events.iter().any(|event| event.title.contains("Natale")));
}
