use antenna_sports::{
    FootballGame, Program, TeamScore, classify_sports, enrich_program_scores, local_sports_view,
    parse_football_scoreboard, reconcile_live_overruns, render_page,
};

const SCOREBOARD: &str = include_str!("fixtures/espn-football.json");

fn program(title: &str, network: &str) -> Program {
    Program {
        network: network.into(),
        station: "TEST HDTV".into(),
        channel: "1.1".into(),
        title: title.into(),
        start_minutes: 14 * 60 + 30,
        end_minutes: 18 * 60,
        detail_url: None,
        airing_id: None,
        score: None,
    }
}

#[test]
fn parses_scores_rankings_and_game_state() {
    let games = parse_football_scoreboard(SCOREBOARD, true).expect("scoreboard should parse");
    let indiana = games
        .iter()
        .find(|game| game.away.name == "Indiana")
        .unwrap();

    assert_eq!(indiana.away.score, "13");
    assert_eq!(indiana.away.rank, Some(6));
    assert_eq!(indiana.home.score, "6");
    assert_eq!(indiana.home.rank, None);
    assert_eq!(indiana.status, "Final/OT");
    assert_eq!(indiana.state, "post");
    assert_eq!(indiana.network, "FOX");
}

#[test]
fn matches_scoreboards_to_the_correct_broadcast_without_confusing_michigan_teams() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let mut programs = vec![
        program("Indiana at Michigan", "FOX"),
        program("Michigan State at UCLA", "CBS"),
    ];

    enrich_program_scores(&mut programs, &games);

    assert_eq!(programs[0].score.as_ref().unwrap().status, "Final/OT");
    assert_eq!(programs[1].score.as_ref().unwrap().status, "7:49 - 3rd");
}

#[test]
fn moves_a_final_game_out_of_live_and_into_recently_finished() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let mut programs = vec![
        program("Indiana at Michigan", "FOX"),
        program("Michigan State at UCLA", "CBS"),
    ];
    enrich_program_scores(&mut programs, &games);

    let view = classify_sports(programs, 16 * 60 + 30);

    assert_eq!(view.live.len(), 1);
    assert_eq!(view.recent.len(), 1);
    assert_eq!(view.recent[0].title, "Indiana at Michigan");
}

#[test]
fn renders_ranked_team_scores_clock_and_final_overtime() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let mut programs = vec![
        program("Indiana at Michigan", "FOX"),
        program("Michigan State at UCLA", "CBS"),
    ];
    enrich_program_scores(&mut programs, &games);
    let view = classify_sports(programs, 16 * 60 + 30);

    let page = render_page(&view, 16 * 60 + 30, "4:30 PM");

    assert!(page.contains("Recently finished"));
    assert!(page.contains("Final/OT"));
    assert!(page.contains("#6"));
    assert!(page.contains(">13<"));
    assert!(page.contains(">6<"));
    assert!(page.contains("7:49 - 3rd"));
    assert!(page.contains("#9"));
}

#[test]
fn keeps_a_live_game_on_screen_when_the_tv_grid_has_switched_to_scoreboard() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let mut live_game = program("Michigan State at UCLA", "CBS");
    live_game.end_minutes = 17 * 60 + 30;
    let mut scoreboard = program("College Football Scoreboard", "CBS");
    scoreboard.start_minutes = 17 * 60 + 30;
    scoreboard.end_minutes = 18 * 60 + 30;
    let mut programs = vec![live_game, scoreboard];
    enrich_program_scores(&mut programs, &games);

    reconcile_live_overruns(&mut programs, 17 * 60 + 42);
    let view = classify_sports(programs, 17 * 60 + 42);

    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].title, "Michigan State at UCLA");
    assert_eq!(view.live[0].end_minutes, 18 * 60 + 30);
    assert_eq!(view.live[0].score.as_ref().unwrap().status, "7:49 - 3rd");
}

#[test]
fn does_not_call_scoreboard_postgame_or_countdown_shows_live_sports() {
    let programs = vec![
        program("College Football Scoreboard", "ABC"),
        program("The U.S. Army Postgame", "CBS"),
        program("B1G College Countdown", "NBC"),
    ];

    let view = classify_sports(programs, 16 * 60 + 30);

    assert!(view.live.is_empty());
}

#[test]
fn keeps_a_recently_final_game_visible_after_its_tv_slot_ends() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let mut finished = program("Indiana at Michigan", "FOX");
    finished.end_minutes = 17 * 60 + 30;
    let mut programs = vec![finished];
    enrich_program_scores(&mut programs, &games);

    let view = classify_sports(programs, 18 * 60 + 5);

    assert_eq!(view.recent.len(), 1);
    assert_eq!(view.recent[0].score.as_ref().unwrap().status, "Final/OT");
}

#[test]
fn does_not_mark_a_scheduled_world_series_broadcast_live_before_first_pitch() {
    let team = |name: &str| TeamScore {
        name: name.into(),
        location: name.into(),
        abbreviation: name.into(),
        score: "0".into(),
        rank: None,
        winner: false,
    };
    let game = FootballGame {
        away: team("Rays"),
        home: team("Brewers"),
        status: "Scheduled".into(),
        state: "pre".into(),
        network: "FOX".into(),
        sport: "baseball".into(),
    };
    let mut world_series = program("World Series Game 2", "FOX");
    world_series.start_minutes = 18 * 60;
    world_series.end_minutes = 19 * 60;
    let mut programs = vec![world_series];

    enrich_program_scores(&mut programs, &[game]);
    let view = classify_sports(programs, 18 * 60 + 5);

    assert!(view.live.is_empty());
}

#[test]
fn excludes_other_regional_games_on_the_same_network() {
    let mut games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    games.push(FootballGame {
        away: TeamScore {
            name: "UTSA".into(),
            location: "UTSA".into(),
            abbreviation: "UTSA".into(),
            score: "35".into(),
            rank: None,
            winner: false,
        },
        home: TeamScore {
            name: "Tulane".into(),
            location: "Tulane".into(),
            abbreviation: "TULN".into(),
            score: "19".into(),
            rank: None,
            winner: false,
        },
        status: "2:59 - 4th".into(),
        state: "in".into(),
        network: "CBS".into(),
        sport: "football".into(),
    });

    let programs = vec![
        Program {
            station: "KEYE HDTV".into(),
            channel: "42.1".into(),
            ..program("Michigan St at UCLA", "CBS")
        },
        program("Indiana at Michigan", "FOX"),
    ];
    let view = local_sports_view(programs, &games, 16 * 60 + 30);

    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].title, "Michigan St at UCLA");
    assert_eq!(view.live[0].network, "CBS");
    assert_eq!(view.live[0].station, "KEYE HDTV");
    assert_eq!(view.live[0].channel, "42.1");
    assert_eq!(view.recent.len(), 1);
    assert_eq!(view.recent[0].title, "Indiana at Michigan");
    assert!(
        view.live
            .iter()
            .all(|program| program.title != "UTSA at Tulane")
    );
}

#[test]
fn never_falls_back_to_national_games_without_a_local_listing() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let view = local_sports_view(vec![], &games, 16 * 60 + 30);
    assert!(view.live.is_empty());
    assert!(view.recent.is_empty());
    assert!(view.upcoming.is_empty());
}

#[test]
fn keeps_an_unresolved_local_listing_without_guessing_a_regional_matchup() {
    let games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    let view = local_sports_view(vec![program("College Football", "CBS")], &games, 16 * 60);
    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].title, "College Football");
    assert!(view.live[0].score.is_none());
}

#[test]
fn filters_upcoming_games_by_local_carriage_and_uses_central_schedule_times() {
    let mut games = parse_football_scoreboard(SCOREBOARD, true).unwrap();
    for game in &mut games {
        game.state = "pre".into();
        game.status = "10/24 - 3:30 PM EDT".into();
    }
    let view = local_sports_view(vec![program("Michigan St at UCLA", "CBS")], &games, 12 * 60);
    assert_eq!(view.upcoming.len(), 1);
    let page = render_page(&view, 12 * 60, "12:00 PM");
    assert!(page.contains("Starts at 2:30 PM"));
    assert!(!page.contains("EDT"));
    assert!(!page.contains("Indiana at Michigan"));
}
