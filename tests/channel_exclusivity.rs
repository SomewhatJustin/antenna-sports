use antenna_sports::{Program, local_sports_view, parse_football_scoreboard};

#[test]
fn only_the_immediately_preceding_local_game_can_overrun_into_postgame() {
    let mut game = parse_football_scoreboard(include_str!("fixtures/espn-football.json"), true)
        .unwrap()
        .into_iter()
        .find(|game| game.state == "in" && game.network == "CBS")
        .unwrap();
    let first_title = format!("{} at {}", game.away.name, game.home.name);
    let first = listing(&first_title, 720, 900);
    game.away.name = "Visitors".into();
    game.away.location = "Visitors".into();
    game.away.abbreviation = "VIS".into();
    let second_title = format!("{} at {}", game.away.name, game.home.name);
    let second = listing(&second_title, 900, 1080);
    let mut first_game = game.clone();
    first_game.away.name = first_title.split(" at ").next().unwrap().into();
    first_game.away.location = first_game.away.name.clone();
    first_game.away.abbreviation = "OLD".into();
    let postgame = listing("NFL Postgame", 1080, 1110);
    let view = local_sports_view(vec![first, second, postgame], &[first_game, game], 1090);
    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].title, second_title);
}

fn listing(title: &str, start: i32, end: i32) -> Program {
    Program {
        network: "CBS".into(),
        station: "KEYE HDTV".into(),
        channel: "42.1".into(),
        title: title.into(),
        start_minutes: start,
        end_minutes: end,
        detail_url: None,
        airing_id: Some(format!("airing-{title}")),
        score: None,
    }
}

#[test]
fn newer_local_slot_replaces_overlapping_earlier_slot() {
    let programs = vec![
        listing("Earlier local game", 720, 990),
        listing("Later local game", 925, 1110),
    ];
    for programs in [programs.clone(), programs.into_iter().rev().collect()] {
        let view = local_sports_view(programs, &[], 970);
        assert_eq!(view.live.len(), 1);
        assert_eq!(view.live[0].title, "Later local game");
    }
}

#[test]
fn ambiguous_same_start_local_matchups_are_not_guessed() {
    let programs = vec![
        listing("Regional game one", 925, 1110),
        listing("Regional game two", 925, 1110),
    ];
    let view = local_sports_view(programs, &[], 970);
    assert!(
        view.live.is_empty(),
        "conflicting guide entries cannot both be local"
    );
}

#[test]
fn identical_period_copies_remain_one_local_broadcast() {
    let first = listing("Local game", 925, 1080);
    let continuation = Program {
        end_minutes: 1110,
        ..first.clone()
    };
    let view = local_sports_view(vec![first, continuation], &[], 970);
    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].end_minutes, 1110);
}

#[test]
fn distinct_channels_can_still_have_simultaneous_games() {
    let first = listing("Local game", 925, 1110);
    let second = Program {
        station: "KTBC HDTV".into(),
        network: "FOX".into(),
        channel: "7.1".into(),
        ..listing("Other local game", 925, 1110)
    };
    let view = local_sports_view(vec![first, second], &[], 970);
    assert_eq!(view.live.len(), 2);
}
