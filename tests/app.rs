use antenna_sports::{
    Period, Program, SportsView, merge_adjacent_programs, periods_around, periods_for, render_page,
    render_page_with_watch_links,
};
use chrono::NaiveDate;
use scraper::{ElementRef, Html, Selector};

#[test]
fn fetches_afternoon_and_night_for_a_late_afternoon_refresh() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
    let periods = periods_for(date, 16);

    assert_eq!(
        periods[0],
        Period {
            name: "Afternoon",
            date,
            day_offset: 0
        }
    );
    assert_eq!(
        periods[1],
        Period {
            name: "Night",
            date,
            day_offset: 0
        }
    );
}

#[test]
fn rolls_the_next_period_into_tomorrow_after_dark() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
    let periods = periods_for(date, 21);

    assert_eq!(periods[0].name, "Night");
    assert_eq!(periods[1].name, "Early");
    assert_eq!(
        periods[1].date,
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    );
    assert_eq!(periods[1].day_offset, 1);
}

#[test]
fn includes_the_previous_afternoon_period_just_after_6_pm() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
    let periods = periods_around(date, 18);

    assert_eq!(periods[0].name, "Afternoon");
    assert_eq!(periods[1].name, "Night");
    assert_eq!(periods[2].name, "Early");
    assert_eq!(
        periods[2].date,
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    );
}

#[test]
fn includes_yesterdays_night_period_just_after_midnight() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    let periods = periods_around(date, 0);

    assert_eq!(periods[0].name, "Night");
    assert_eq!(
        periods[0].date,
        NaiveDate::from_ymd_opt(2026, 10, 24).unwrap()
    );
    assert_eq!(periods[0].day_offset, -1);
    assert_eq!(periods[1].name, "Early");
}

#[test]
fn merges_a_broadcast_split_across_schedule_periods() {
    let first = Program {
        network: "ABC".into(),
        station: "KVUE HDTV".into(),
        channel: "24.1".into(),
        title: "College Football".into(),
        start_minutes: 17 * 60 + 30,
        end_minutes: 18 * 60,
        detail_url: None,
        airing_id: None,
        score: None,
    };
    let second = Program {
        start_minutes: 18 * 60,
        end_minutes: 19 * 60,
        ..first.clone()
    };

    let merged = merge_adjacent_programs(vec![second, first]);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].start_minutes, 17 * 60 + 30);
    assert_eq!(merged[0].end_minutes, 19 * 60);
}

#[test]
fn renders_live_and_upcoming_sports_without_allowing_html_in_titles() {
    let view = SportsView {
        live: vec![Program {
            network: "FOX".into(),
            station: "KTBC HDTV".into(),
            channel: "7.1".into(),
            title: "College Football".into(),
            start_minutes: 14 * 60 + 30,
            end_minutes: 18 * 60,
            detail_url: None,
            airing_id: None,
            score: None,
        }],
        recent: vec![],
        upcoming: vec![Program {
            network: "ABC".into(),
            station: "KVUE HDTV".into(),
            channel: "24.1".into(),
            title: "Scoreboard <script>alert(1)</script>".into(),
            start_minutes: 17 * 60 + 30,
            end_minutes: 18 * 60,
            detail_url: None,
            airing_id: None,
            score: None,
        }],
    };

    let page = render_page(&view, 16 * 60 + 30, "4:30 PM");

    assert!(page.contains("1 sports broadcast is on now"));
    assert!(page.contains("College Football"));
    assert!(page.contains("Starts at 5:30 PM"));
    assert!(page.contains("Scoreboard &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!page.contains("Scoreboard <script>"));
}

#[test]
fn watch_links_follow_the_live_cards_station_and_skip_unconfigured_stations() {
    let game = Program {
        network: "FOX".into(),
        station: "KTBC HDTV".into(),
        channel: "7.1".into(),
        title: "College Football \"Special\"".into(),
        start_minutes: 14 * 60 + 30,
        end_minutes: 18 * 60,
        detail_url: None,
        airing_id: None,
        score: None,
    };
    let view = SportsView {
        live: vec![
            game.clone(),
            Program {
                network: "NBC".into(),
                station: "KXAN HDTV".into(),
                channel: "36.1".into(),
                ..game.clone()
            },
        ],
        recent: vec![game.clone()],
        upcoming: vec![game],
    };
    let url = "https://jellyfin.example/web/#/details?id=fox&serverId=server";
    let watch_links = [("KTBC HDTV", url.parse().unwrap())].into();
    let page = render_page_with_watch_links(&view, 16 * 60, "4:00 PM", &watch_links);
    let document = Html::parse_document(&page);
    let selector = Selector::parse("a.watch-live").unwrap();
    let links: Vec<_> = document.select(&selector).collect();

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].attr("href"), Some(url));
    assert_eq!(links[0].text().collect::<String>(), "Watch live");
    assert_eq!(
        links[0].attr("aria-label"),
        Some("Watch live in Jellyfin — College Football \"Special\"")
    );
    let card = links[0].parent().and_then(ElementRef::wrap).unwrap();
    assert!(card.text().any(|text| text.contains("KTBC · 7.1")));
    assert!(page.contains("?id=fox&amp;serverId=server"));
}

#[test]
fn merges_overlapping_period_copies_and_relay_channels() {
    let first = Program {
        network: "FOX".into(),
        station: "KTBC HDTV".into(),
        channel: "7.1".into(),
        title: "NFL Football".into(),
        start_minutes: 12 * 60,
        end_minutes: 15 * 60,
        detail_url: None,
        airing_id: Some("airing-1".into()),
        score: None,
    };
    let duplicate = Program {
        end_minutes: 15 * 60 + 30,
        channel: "99.1".into(),
        ..first.clone()
    };
    let next_game = Program {
        start_minutes: 15 * 60 + 30,
        end_minutes: 18 * 60,
        airing_id: Some("airing-2".into()),
        ..first.clone()
    };
    let merged = merge_adjacent_programs(vec![first, duplicate, next_game]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].end_minutes, 15 * 60 + 30);
    assert_eq!(merged[1].airing_id.as_deref(), Some("airing-2"));
}
