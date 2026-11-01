use antenna_sports::{
    episode_title_for_airing, local_sports_view, parse_football_scoreboard, parse_schedule,
    render_page,
};

// A real capture from 2026-11-01, not a reconstruction of historical live state.
// National CBS has two live games; Austin KEYE carries KC at Denver only.
#[test]
fn november_first_capture_shows_only_the_locally_scheduled_cbs_game() {
    let mut programs =
        parse_schedule(include_str!("fixtures/keye-2026-11-01-afternoon.html")).unwrap();
    programs.extend(parse_schedule(include_str!("fixtures/keye-2026-11-01-night.html")).unwrap());
    assert_eq!(
        programs.len(),
        3,
        "includes the repeated late-game period copy"
    );
    for program in &mut programs {
        program.title = episode_title_for_airing(
            include_str!("fixtures/keye-2026-11-01-detail.html"),
            program.airing_id.as_deref().unwrap(),
        )
        .unwrap();
    }
    let games = parse_football_scoreboard(include_str!("fixtures/espn-2026-11-01-nfl.json"), false)
        .unwrap();
    assert_eq!(games.iter().filter(|game| game.state == "in").count(), 2);
    let view = local_sports_view(programs, &games, 16 * 60 + 12);
    assert_eq!(view.live.len(), 1);
    assert_eq!(view.live[0].channel, "42.1");
    assert_eq!(view.live[0].title, "Kansas City Chiefs at Denver Broncos");
    assert_eq!(view.live[0].end_minutes, 18 * 60 + 30);
    assert_eq!(view.recent.len(), 1);
    assert_eq!(view.recent[0].title, "Baltimore Ravens at Buffalo Bills");
    let page = render_page(&view, 16 * 60 + 12, "4:12 PM");
    assert!(!page.contains("Patriots"));
    assert!(!page.contains("Dolphins"));
}
