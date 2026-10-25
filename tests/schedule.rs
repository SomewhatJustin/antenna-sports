use antenna_sports::{classify_sports, episode_title_for_airing, parse_schedule};

const FIXTURE: &str = include_str!("fixtures/afternoon.html");
const DETAIL_FIXTURE: &str = include_str!("fixtures/detail.html");

#[test]
fn parses_only_the_four_major_austin_broadcast_stations() {
    let programs = parse_schedule(FIXTURE).expect("fixture should parse");

    assert_eq!(programs.len(), 6);
    assert!(
        programs
            .iter()
            .all(|program| ["ABC", "CBS", "FOX", "NBC"].contains(&program.network.as_str()))
    );
    assert!(
        !programs
            .iter()
            .any(|program| program.title == "A Sport We Ignore")
    );
}

#[test]
fn identifies_live_and_upcoming_sports_at_430_pm() {
    let programs = parse_schedule(FIXTURE).expect("fixture should parse");
    let view = classify_sports(programs, 16 * 60 + 30);

    assert_eq!(view.live.len(), 4);
    assert_eq!(view.upcoming.len(), 1);
    assert_eq!(view.upcoming[0].title, "College Football Scoreboard");
    assert_eq!(view.upcoming[0].start_minutes, 17 * 60 + 30);
    let abc = view
        .live
        .iter()
        .find(|program| program.network == "ABC")
        .unwrap();
    assert_eq!(abc.title, "College Football");
    assert_eq!(abc.start_minutes, 14 * 60 + 40);
}

#[test]
fn computes_the_end_time_from_half_hour_slots() {
    let programs = parse_schedule(FIXTURE).expect("fixture should parse");
    let fox = programs
        .iter()
        .find(|program| program.network == "FOX")
        .unwrap();

    assert_eq!(fox.start_minutes, 14 * 60 + 30);
    assert_eq!(fox.end_minutes, 18 * 60);
    assert_eq!(fox.airing_id.as_deref(), Some("airing-276532"));
    assert_eq!(
        fox.detail_url.as_deref(),
        Some("https://www.ontvtonight.com/guide/listings/4334117515/college-football-2/")
    );
}

#[test]
fn finds_the_specific_matchup_for_an_airing_anchor() {
    assert_eq!(
        episode_title_for_airing(DETAIL_FIXTURE, "airing-276532").as_deref(),
        Some("Ohio State at Penn State")
    );
    assert_eq!(
        episode_title_for_airing(DETAIL_FIXTURE, "airing-date"),
        None
    );
}

#[test]
fn excludes_non_sports_programs() {
    let programs = parse_schedule(FIXTURE).expect("fixture should parse");

    assert!(!programs.iter().any(|program| program.title == "Jeopardy!"));
}
