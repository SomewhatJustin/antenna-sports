use antenna_sports::{Program, classify_sports, periods_around, render_page};
use chrono::NaiveDate;

#[test]
fn fetches_through_the_same_period_tomorrow() {
    let day = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
    for hour in 0..24 {
        let periods = periods_around(day, hour);
        assert_eq!(periods.len(), 6);
        assert_eq!(periods.last().unwrap().date, day.succ_opt().unwrap());
        assert_eq!(periods.last().unwrap().name, periods[1].name);
    }
}

#[test]
fn lists_every_start_within_24_hours_across_midnight() {
    let now = 23 * 60 + 30;
    let programs = (0..10)
        .chain([1440, 1441])
        .map(|offset| Program {
            network: "ABC".into(),
            station: "KVUE".into(),
            channel: "24.1".into(),
            title: format!("Game {offset}"),
            start_minutes: now + offset,
            end_minutes: now + offset + 60,
            airing_id: None,
            detail_url: None,
            score: None,
        })
        .collect();
    let view = classify_sports(programs, now);
    assert_eq!(view.upcoming.len(), 10);
    let html = render_page(&view, now, "11:30 PM");
    assert_eq!(html.matches("class=\"card upcoming-card\"").count(), 10);
    assert!(html.contains("Game 1440</h3>"));
    assert!(html.contains("Next 24 hours"));
}
