use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate};
use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TeamScore {
    pub name: String,
    pub location: String,
    pub abbreviation: String,
    pub score: String,
    pub rank: Option<u32>,
    pub winner: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FootballGame {
    pub away: TeamScore,
    pub home: TeamScore,
    pub status: String,
    pub state: String,
    pub network: String,
    pub sport: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Program {
    pub network: String,
    pub station: String,
    pub channel: String,
    pub title: String,
    pub start_minutes: i32,
    pub end_minutes: i32,
    #[serde(skip)]
    pub detail_url: Option<String>,
    #[serde(skip)]
    pub airing_id: Option<String>,
    pub score: Option<FootballGame>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SportsView {
    pub live: Vec<Program>,
    pub recent: Vec<Program>,
    pub upcoming: Vec<Program>,
}

pub fn parse_schedule(html: &str) -> Result<Vec<Program>> {
    let document = Html::parse_document(html);
    let row_selector = Selector::parse(".schedule-row").expect("valid row selector");
    let channel_selector = Selector::parse("a.channel-link").expect("valid channel selector");
    let sport_selector = Selector::parse(".program-card.is-sport").expect("valid sport selector");
    let link_selector = Selector::parse(".tvshow a[title]").expect("valid program selector");

    let mut programs = Vec::new();

    for row in document.select(&row_selector) {
        let Some(channel_link) = row.select(&channel_selector).next() else {
            continue;
        };
        let station = channel_link.value().attr("title").unwrap_or_default();
        let Some(network) = network_for_station(station) else {
            continue;
        };
        let channel = channel_link
            .value()
            .attr("aria-label")
            .and_then(|label| label.rsplit_once("channel ").map(|(_, value)| value))
            .unwrap_or("—")
            .to_string();

        for card in row.select(&sport_selector) {
            let classes = card.value().attr("class").unwrap_or_default();
            let Some((start_minutes, slots)) =
                classes.split_whitespace().find_map(parse_slot_class)
            else {
                continue;
            };
            let card_end = start_minutes + slots * 30;
            let mut shows = card
                .select(&link_selector)
                .filter_map(|link| {
                    let title = link.value().attr("title")?.trim();
                    if title.is_empty() {
                        return None;
                    }
                    let show_start = link
                        .value()
                        .attr("aria-label")
                        .and_then(parse_start_from_aria)
                        .unwrap_or(start_minutes);
                    let href = link.value().attr("href").unwrap_or_default();
                    let (detail_url, airing_id) = href
                        .split_once('#')
                        .map(|(url, fragment)| (Some(url.to_string()), Some(fragment.to_string())))
                        .unwrap_or((None, None));
                    Some((show_start, title.to_string(), detail_url, airing_id))
                })
                .collect::<Vec<_>>();
            shows.sort_by_key(|show| show.0);

            for (index, (show_start, title, detail_url, airing_id)) in shows.iter().enumerate() {
                let show_end = shows.get(index + 1).map(|next| next.0).unwrap_or(card_end);
                if show_end <= *show_start {
                    continue;
                }
                programs.push(Program {
                    network: network.to_string(),
                    station: station.to_string(),
                    channel: channel.clone(),
                    title: title.clone(),
                    start_minutes: *show_start,
                    end_minutes: show_end,
                    detail_url: detail_url.clone(),
                    airing_id: airing_id.clone(),
                    score: None,
                });
            }
        }
    }

    if programs.is_empty() && html.contains("schedule-row") {
        return Ok(programs);
    }
    if !html.contains("schedule-row") {
        anyhow::bail!("schedule response did not contain any schedule rows");
    }

    programs.sort_by_key(|program| {
        (
            program.start_minutes,
            program.network.clone(),
            program.title.clone(),
        )
    });
    programs.dedup_by(|left, right| {
        left.network == right.network
            && left.title == right.title
            && left.start_minutes == right.start_minutes
            && left.end_minutes == right.end_minutes
    });
    Ok(programs)
}

pub fn episode_title_for_airing(html: &str, airing_id: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let airing_selector = Selector::parse(".show-airing").ok()?;
    let anchor_selector = Selector::parse(".ott-series-airing__anchor[id]").ok()?;
    let title_selector = Selector::parse(".show-episode-title").ok()?;

    for airing in document.select(&airing_selector) {
        let matches = airing
            .select(&anchor_selector)
            .any(|anchor| anchor.value().attr("id") == Some(airing_id));
        if !matches {
            continue;
        }
        let title = airing
            .select(&title_selector)
            .next()?
            .text()
            .collect::<String>()
            .trim()
            .to_string();
        if title.is_empty()
            || title.eq_ignore_ascii_case("Teams TBA")
            || ["%m-%d-%Y", "%Y-%m-%d", "%m/%d/%Y", "%Y/%m/%d"]
                .iter()
                .any(|format| NaiveDate::parse_from_str(&title, format).is_ok())
        {
            return None;
        }
        return Some(title);
    }
    None
}

pub fn parse_football_scoreboard(json: &str, include_rankings: bool) -> Result<Vec<FootballGame>> {
    let root: Value =
        serde_json::from_str(json).context("football scoreboard returned invalid JSON")?;
    let mut games = Vec::new();

    for event in root["events"].as_array().into_iter().flatten() {
        let Some(competition) = event["competitions"]
            .as_array()
            .and_then(|items| items.first())
        else {
            continue;
        };
        let competitors = competition["competitors"].as_array();
        let Some(away) = competitors
            .into_iter()
            .flatten()
            .find(|team| team["homeAway"] == "away")
            .and_then(|team| parse_team_score(team, include_rankings))
        else {
            continue;
        };
        let Some(home) = competitors
            .into_iter()
            .flatten()
            .find(|team| team["homeAway"] == "home")
            .and_then(|team| parse_team_score(team, include_rankings))
        else {
            continue;
        };
        let status = event["status"]["type"]["shortDetail"]
            .as_str()
            .or_else(|| event["status"]["type"]["detail"].as_str())
            .unwrap_or("Status unavailable")
            .to_string();
        let state = event["status"]["type"]["state"]
            .as_str()
            .unwrap_or("pre")
            .to_string();
        let network = competition["broadcasts"]
            .as_array()
            .and_then(|broadcasts| broadcasts.first())
            .and_then(|broadcast| broadcast["names"].as_array())
            .and_then(|names| names.first())
            .and_then(Value::as_str)
            .map(normalize_network)
            .unwrap_or_default();

        games.push(FootballGame {
            away,
            home,
            status,
            state,
            network,
            sport: "football".to_string(),
        });
    }

    Ok(games)
}

pub fn enrich_program_scores(programs: &mut [Program], games: &[FootballGame]) {
    for program in programs {
        let normalized_title = normalize_words(&program.title);
        let normalized_network = normalize_network(&program.network);
        if let Some(game) = games.iter().find(|game| {
            (game.network.is_empty() || game.network == normalized_network)
                && team_appears_in_title(&normalized_title, &game.away)
                && team_appears_in_title(&normalized_title, &game.home)
        }) {
            program.score = Some(game.clone());
            continue;
        }
        if normalized_title.contains("world series") {
            let matching_network = games
                .iter()
                .filter(|game| game.network == normalized_network && game.sport == "baseball")
                .collect::<Vec<_>>();
            if matching_network.len() == 1 {
                program.score = Some(matching_network[0].clone());
            }
        }
    }
}

fn parse_team_score(value: &Value, include_rankings: bool) -> Option<TeamScore> {
    let team = &value["team"];
    let rank = include_rankings
        .then(|| value["curatedRank"]["current"].as_u64())
        .flatten()
        .filter(|rank| *rank <= 25)
        .map(|rank| rank as u32);
    Some(TeamScore {
        name: team["shortDisplayName"]
            .as_str()
            .or_else(|| team["location"].as_str())?
            .to_string(),
        location: team["location"].as_str().unwrap_or_default().to_string(),
        abbreviation: team["abbreviation"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        score: value["score"].as_str().unwrap_or("0").to_string(),
        rank,
        winner: value["winner"].as_bool().unwrap_or(false),
    })
}

fn team_appears_in_title(title: &str, team: &TeamScore) -> bool {
    [&team.location, &team.name, &team.abbreviation]
        .into_iter()
        .map(|value| normalize_words(value))
        .any(|value| value.len() >= 2 && title.contains(&value))
}

fn normalize_words(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_network(value: &str) -> String {
    match value.trim().to_ascii_uppercase().as_str() {
        "FOX" => "FOX".to_string(),
        "CBS" => "CBS".to_string(),
        "ABC" => "ABC".to_string(),
        "NBC" => "NBC".to_string(),
        other => other.to_string(),
    }
}

pub fn reconcile_live_overruns(programs: &mut Vec<Program>, now_minutes: i32) {
    let mut reconciliations = Vec::new();
    for (game_index, game_program) in programs.iter().enumerate() {
        let is_live_game = game_program
            .score
            .as_ref()
            .is_some_and(|score| score.state == "in");
        if !is_live_game || game_program.end_minutes > now_minutes {
            continue;
        }
        let Some((placeholder_index, placeholder)) =
            programs.iter().enumerate().find(|(_, candidate)| {
                candidate.network == game_program.network
                    && candidate.start_minutes <= now_minutes
                    && now_minutes < candidate.end_minutes
                    && candidate.score.is_none()
                    && is_overrun_placeholder(&candidate.title)
            })
        else {
            continue;
        };
        reconciliations.push((game_index, placeholder_index, placeholder.end_minutes));
    }

    for (game_index, _, new_end) in &reconciliations {
        programs[*game_index].end_minutes = programs[*game_index].end_minutes.max(*new_end);
    }
    let mut removals = reconciliations
        .into_iter()
        .map(|(_, placeholder_index, _)| placeholder_index)
        .collect::<Vec<_>>();
    removals.sort_unstable();
    removals.dedup();
    for index in removals.into_iter().rev() {
        programs.remove(index);
    }
}

fn is_non_game_sports_show(title: &str) -> bool {
    let title = normalize_words(title);
    is_overrun_placeholder(&title)
        || title.contains("countdown")
        || title.contains("pregame")
        || title.contains("pre game")
        || title.contains("kickoff")
        || title.contains("showtime")
        || title.contains("football today")
}

fn is_overrun_placeholder(title: &str) -> bool {
    let title = normalize_words(title);
    title.contains("scoreboard") || title.contains("postgame") || title.contains("post game")
}

pub fn merge_adjacent_programs(mut programs: Vec<Program>) -> Vec<Program> {
    programs.sort_by_key(|program| (program.start_minutes, program.network.clone()));
    let mut merged: Vec<Program> = Vec::with_capacity(programs.len());
    for program in programs {
        if let Some(existing) = merged.iter_mut().rev().find(|candidate| {
            candidate.network == program.network
                && candidate.title == program.title
                && candidate.start_minutes <= program.start_minutes
                && candidate.end_minutes >= program.start_minutes
                && (candidate.airing_id.is_none()
                    || program.airing_id.is_none()
                    || candidate.airing_id == program.airing_id)
        }) {
            existing.end_minutes = existing.end_minutes.max(program.end_minutes);
        } else {
            merged.push(program);
        }
    }
    merged.sort_by_key(|program| (program.start_minutes, program.network.clone()));
    merged
}

pub fn classify_sports(mut programs: Vec<Program>, now_minutes: i32) -> SportsView {
    programs.sort_by_key(|program| (program.start_minutes, program.network.clone()));
    let current = programs.iter().filter(|program| {
        program.start_minutes <= now_minutes && now_minutes < program.end_minutes
    });
    let live = current
        .clone()
        .filter(|program| {
            !is_non_game_sports_show(&program.title)
                && program
                    .score
                    .as_ref()
                    .is_none_or(|score| score.state == "in")
        })
        .cloned()
        .collect();
    let recent = programs
        .iter()
        .filter(|program| {
            let is_final = program
                .score
                .as_ref()
                .is_some_and(|score| score.state == "post");
            let is_current =
                program.start_minutes <= now_minutes && now_minutes < program.end_minutes;
            let ended_recently =
                program.end_minutes <= now_minutes && now_minutes - program.end_minutes <= 90;
            is_final && (is_current || ended_recently)
        })
        .cloned()
        .collect();
    let upcoming = programs
        .into_iter()
        .filter(|program| program.start_minutes > now_minutes)
        .collect();

    SportsView {
        live,
        recent,
        upcoming,
    }
}

pub fn local_sports_view(
    programs: Vec<Program>,
    games: &[FootballGame],
    now_minutes: i32,
) -> SportsView {
    // A national network name does not establish local carriage. Only programs
    // from the Austin guide can become cards; ESPN supplies scores and state.
    let mut programs = merge_adjacent_programs(programs);
    enrich_program_scores(&mut programs, games);
    reconcile_live_overruns(&mut programs, now_minutes);
    classify_sports(programs, now_minutes)
}

fn network_for_station(station: &str) -> Option<&'static str> {
    match station {
        "KTBC HDTV" => Some("FOX"),
        "KVUE HDTV" => Some("ABC"),
        "KXAN HDTV" => Some("NBC"),
        "KEYE HDTV" => Some("CBS"),
        _ => None,
    }
}

fn parse_start_from_aria(label: &str) -> Option<i32> {
    let marker = "starts at ";
    let value = label.split(marker).nth(1)?.split(',').next()?.trim();
    let mut parts = value.split_whitespace();
    let clock = parts.next()?;
    let meridiem = parts.next()?.to_ascii_lowercase();
    let (hour, minute) = clock.split_once(':')?;
    let mut hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    if !(1..=12).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    if hour == 12 {
        hour = 0;
    }
    if meridiem == "pm" {
        hour += 12;
    } else if meridiem != "am" {
        return None;
    }
    Some(hour * 60 + minute)
}

fn parse_slot_class(class_name: &str) -> Option<(i32, i32)> {
    let value = class_name.strip_prefix("slt-")?;
    let (hhmm, slots) = value.rsplit_once('-')?;
    if hhmm.len() != 4 {
        return None;
    }
    let hour: i32 = hhmm[..2].parse().ok()?;
    let minute: i32 = hhmm[2..].parse().ok()?;
    let slots: i32 = slots.parse().ok()?;
    if hour > 23 || minute > 59 || slots <= 0 {
        return None;
    }
    Some((hour * 60 + minute, slots))
}

pub fn decode_schedule_response(body: &str) -> Result<String> {
    serde_json::from_str(body).context("schedule provider returned invalid JSON")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub name: &'static str,
    pub date: NaiveDate,
    pub day_offset: i32,
}

pub fn periods_for(date: NaiveDate, hour: u32) -> [Period; 2] {
    let names = ["Early", "Morning", "Afternoon", "Night"];
    let index = match hour {
        0..=5 => 0,
        6..=11 => 1,
        12..=17 => 2,
        _ => 3,
    };
    let next_index = (index + 1) % names.len();
    let rolls_to_tomorrow = next_index == 0;
    let next_date = if rolls_to_tomorrow {
        date + Duration::days(1)
    } else {
        date
    };

    [
        Period {
            name: names[index],
            date,
            day_offset: 0,
        },
        Period {
            name: names[next_index],
            date: next_date,
            day_offset: i32::from(rolls_to_tomorrow),
        },
    ]
}

pub fn periods_around(date: NaiveDate, hour: u32) -> [Period; 3] {
    let names = ["Early", "Morning", "Afternoon", "Night"];
    let current_index = match hour {
        0..=5 => 0,
        6..=11 => 1,
        12..=17 => 2,
        _ => 3,
    };
    let previous_index = (current_index + names.len() - 1) % names.len();
    let next_index = (current_index + 1) % names.len();
    let previous_rolls_back = current_index == 0;
    let next_rolls_forward = current_index == names.len() - 1;

    [
        Period {
            name: names[previous_index],
            date: if previous_rolls_back {
                date - Duration::days(1)
            } else {
                date
            },
            day_offset: if previous_rolls_back { -1 } else { 0 },
        },
        Period {
            name: names[current_index],
            date,
            day_offset: 0,
        },
        Period {
            name: names[next_index],
            date: if next_rolls_forward {
                date + Duration::days(1)
            } else {
                date
            },
            day_offset: i32::from(next_rolls_forward),
        },
    ]
}

pub fn render_page(view: &SportsView, now_minutes: i32, updated_at: &str) -> String {
    let live_count = view.live.len();
    let headline = match live_count {
        0 => "No sports are on right now".to_string(),
        1 => "1 sports broadcast is on now".to_string(),
        count => format!("{count} sports broadcasts are on now"),
    };

    let live_cards = if view.live.is_empty() {
        "<div class=\"empty\"><strong>The antenna is quiet.</strong><span>Next broadcast is listed below.</span></div>".to_string()
    } else {
        view.live
            .iter()
            .map(|program| render_current_card(program, false))
            .collect::<Vec<_>>()
            .join("")
    };

    let recent_section = if view.recent.is_empty() {
        String::new()
    } else {
        let cards = view
            .recent
            .iter()
            .map(|program| render_current_card(program, true))
            .collect::<Vec<_>>()
            .join("");
        format!(
            "<section class=\"recent-section\"><p class=\"eyebrow muted-eyebrow\">RECENTLY FINISHED</p><h2 class=\"section-title\">Recently finished</h2><div class=\"live-grid recent-grid\">{cards}</div></section>"
        )
    };

    let upcoming_cards = view.upcoming.iter().take(8).map(|program| {
        let when = if program.start_minutes >= 1440 && now_minutes < 1440 {
            format!("Tomorrow at {}", format_clock(program.start_minutes))
        } else {
            format!("Starts at {}", format_clock(program.start_minutes))
        };
        format!(
            "<article class=\"card upcoming-card\"><div class=\"card-top\"><span class=\"network\">{}</span><span class=\"channel\">{} · {}</span></div><h3>{}</h3><p class=\"timing\">{}</p></article>",
            escape_html(&program.network),
            escape_html(&program.station.replace(" HDTV", "")),
            escape_html(&program.channel),
            escape_html(&program.title),
            escape_html(&when),
        )
    }).collect::<Vec<_>>().join("");

    let upcoming_section = if upcoming_cards.is_empty() {
        "<div class=\"empty compact\"><span>No more sports found in the next schedule window.</span></div>".to_string()
    } else {
        format!("<div class=\"upcoming-grid\">{upcoming_cards}</div>")
    };

    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="theme-color" content="#050505">
<meta http-equiv="refresh" content="300">
<title>Antenna Sports</title>
<style>
:root{{--bg:#050505;--surface:#141414;--surface-2:#1d1d1f;--text:#f5f5f7;--muted:#a1a1a6;--blue:#2997ff;--live:#ff453a}}
*{{box-sizing:border-box}}html{{background:var(--bg)}}body{{margin:0;background:var(--bg);color:var(--text);font-family:system-ui,-apple-system,"Segoe UI",sans-serif;letter-spacing:-.02em}}
main{{width:min(100% - 32px,980px);margin:0 auto;padding:48px 0 72px}}
header{{display:flex;justify-content:space-between;align-items:center;gap:24px;margin-bottom:72px}}.brand{{font-size:17px;font-weight:650}}.location{{color:var(--muted);font-size:13px;text-align:right}}
.eyebrow{{color:var(--blue);font-size:14px;font-weight:650;margin:0 0 12px}}h1{{font-size:clamp(42px,8vw,76px);line-height:1.02;letter-spacing:-.055em;max-width:850px;margin:0 0 18px}}.subhead{{color:var(--muted);font-size:17px;line-height:1.45;margin:0 0 36px}}
.live-grid{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px;margin-bottom:72px}}.card{{background:var(--surface);border-radius:18px;padding:24px;min-width:0}}.live-card{{min-height:220px;display:flex;flex-direction:column}}.card-top{{display:flex;justify-content:space-between;align-items:center;gap:14px;margin-bottom:26px}}.network{{font-size:13px;font-weight:750;color:var(--blue)}}.channel{{font-size:13px;color:var(--muted)}}h2,h3{{margin:0;overflow-wrap:anywhere}}h2{{font-size:clamp(26px,4vw,38px);line-height:1.08;letter-spacing:-.04em}}h3{{font-size:21px;line-height:1.15}}.timing{{margin:auto 0 0;color:var(--muted);font-size:14px;padding-top:28px}}.timing.live{{color:#fff}}.live-dot{{display:inline-block;width:8px;height:8px;border-radius:50%;background:var(--live);margin-right:8px;box-shadow:0 0 0 5px rgba(255,69,58,.12)}}
.scoreboard{{display:grid;gap:12px;margin-top:2px}}.team-row{{display:grid;grid-template-columns:1fr auto;align-items:baseline;gap:16px}}.team-name{{font-size:clamp(24px,3.2vw,34px);font-weight:650;letter-spacing:-.04em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}.team-score{{font-size:clamp(30px,4vw,42px);font-weight:700;font-variant-numeric:tabular-nums}}.rank{{display:inline-block;color:var(--muted);font-size:.58em;font-weight:700;min-width:2.1em;vertical-align:.18em}}.game-status{{margin:auto 0 0;padding-top:24px;font-size:14px;font-weight:650;color:#fff}}.game-status.final{{color:var(--muted)}}.recent-section{{margin-top:-24px}}.recent-grid{{margin-bottom:72px}}.muted-eyebrow{{color:var(--muted)}}.section-title{{font-size:30px;margin:0 0 20px}}.sr-only{{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}}
.section-head{{display:flex;align-items:end;justify-content:space-between;margin-bottom:20px}}.section-head h2{{font-size:30px}}button{{appearance:none;border:0;background:var(--surface-2);color:var(--text);font:inherit;font-size:14px;padding:10px 16px;border-radius:999px;cursor:pointer}}button:hover{{background:#2a2a2d}}button:focus-visible{{outline:2px solid var(--blue);outline-offset:3px}}
.upcoming-grid{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}}.upcoming-card{{min-height:164px;display:flex;flex-direction:column;background:#101010}}.upcoming-card .card-top{{margin-bottom:22px}}
.empty{{grid-column:1/-1;background:var(--surface);border-radius:18px;padding:36px;display:flex;flex-direction:column;gap:6px;color:var(--muted)}}.empty strong{{color:var(--text);font-size:21px}}.empty.compact{{padding:24px}}footer{{margin-top:64px;color:#6e6e73;font-size:12px;line-height:1.5}}
@media(max-width:640px){{main{{width:min(100% - 24px,980px);padding-top:28px}}header{{margin-bottom:56px;align-items:flex-start}}.location{{max-width:150px}}h1{{font-size:46px}}.live-grid,.upcoming-grid{{grid-template-columns:1fr}}.live-card{{min-height:190px}}.card{{border-radius:16px;padding:20px}}.section-head{{align-items:center}}}}
</style>
</head>
<body><main>
<header><div class="brand">Antenna Sports</div><div class="location">Austin, TX · 78746<br>ABC · CBS · FOX · NBC</div></header>
<section><p class="eyebrow">ON YOUR ANTENNA NOW</p><h1>{headline}</h1><p class="subhead">Live football and baseball on Austin's major over-the-air networks.</p><div class="live-grid">{live_cards}</div></section>
{recent_section}
<section><div class="section-head"><h2>Up next</h2><button type="button" onclick="location.reload()">Refresh</button></div>{upcoming_section}</section>
<footer>Updated {updated_at} Central · Refreshes every 5 minutes<br>Austin antenna listings from On TV Tonight · Scores from ESPN.</footer>
</main></body></html>"##
    )
}

fn render_current_card(program: &Program, finished: bool) -> String {
    let header = format!(
        "<div class=\"card-top\"><span class=\"network\">{}</span><span class=\"channel\">{} · {}</span></div>",
        escape_html(&program.network),
        escape_html(&program.station.replace(" HDTV", "")),
        escape_html(&program.channel),
    );
    if let Some(game) = &program.score {
        let status_class = if finished {
            "game-status final"
        } else {
            "game-status"
        };
        let status_prefix = if finished {
            String::new()
        } else {
            "<span class=\"live-dot\"></span>".to_string()
        };
        return format!(
            "<article class=\"card live-card\">{header}<h2 class=\"sr-only\">{}</h2><div class=\"scoreboard\">{}{}</div><p class=\"{status_class}\">{status_prefix}{}</p></article>",
            escape_html(&program.title),
            render_team_row(&game.away),
            render_team_row(&game.home),
            escape_html(&game.status),
        );
    }

    let timing = if finished {
        "Finished".to_string()
    } else {
        format!(
            "<span class=\"live-dot\"></span>Live · until {}",
            format_clock(program.end_minutes)
        )
    };
    format!(
        "<article class=\"card live-card\">{header}<h2>{}</h2><p class=\"timing {}\">{timing}</p></article>",
        escape_html(&program.title),
        if finished { "" } else { "live" },
    )
}

fn render_team_row(team: &TeamScore) -> String {
    let rank = team
        .rank
        .map(|rank| format!("<span class=\"rank\">#{rank}</span>"))
        .unwrap_or_else(|| "<span class=\"rank\"></span>".to_string());
    format!(
        "<div class=\"team-row\"><div class=\"team-name\">{rank}{}</div><strong class=\"team-score\">{}</strong></div>",
        escape_html(&team.name),
        escape_html(&team.score),
    )
}

fn format_clock(total_minutes: i32) -> String {
    let day_minutes = total_minutes.rem_euclid(1440);
    let hour_24 = day_minutes / 60;
    let minute = day_minutes % 60;
    let period = if hour_24 < 12 { "AM" } else { "PM" };
    let hour_12 = match hour_24 % 12 {
        0 => 12,
        hour => hour,
    };
    if minute == 0 {
        format!("{hour_12}:00 {period}")
    } else {
        format!("{hour_12}:{minute:02} {period}")
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
