use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use antenna_sports::{
    FootballGame, Period, Program, decode_schedule_response, episode_title_for_airing,
    local_sports_view, parse_football_scoreboard, parse_schedule, periods_around, render_page,
};
use anyhow::{Context, Result};
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use chrono::{Duration as ChronoDuration, Timelike, Utc};
use chrono_tz::America::Chicago;
use reqwest::Client;
use tokio::{net::TcpListener, sync::RwLock, task::JoinSet};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

const PORT: u16 = 8790;

#[derive(Clone)]
struct AppState {
    client: Client,
    cache: Arc<RwLock<Option<CachedPage>>>,
}

struct CachedPage {
    created: Instant,
    html: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let client = Client::builder()
        .user_agent("AntennaSports/0.1 (+private personal TV guide)")
        .timeout(Duration::from_secs(20))
        .build()
        .context("build HTTP client")?;

    let state = AppState {
        client,
        cache: Arc::new(RwLock::new(None)),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/sports/health", get(health))
        .fallback(get(index))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(("127.0.0.1", PORT)).await?;
    info!(port = PORT, "Antenna Sports listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn index(State(state): State<AppState>) -> Response {
    if let Some(html) = cached_page(&state).await {
        return Html(html).into_response();
    }

    match build_live_page(&state.client).await {
        Ok(html) => {
            *state.cache.write().await = Some(CachedPage {
                created: Instant::now(),
                html: html.clone(),
            });
            Html(html).into_response()
        }
        Err(error) => {
            warn!(%error, "schedule refresh failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(error_page(
                    "The schedule source did not answer. Try refresh in a minute.",
                )),
            )
                .into_response()
        }
    }
}

async fn cached_page(state: &AppState) -> Option<String> {
    let cache = state.cache.read().await;
    cache.as_ref().and_then(|cached| {
        (cached.created.elapsed() < Duration::from_secs(120)).then(|| cached.html.clone())
    })
}

async fn build_live_page(client: &Client) -> Result<String> {
    let now = Utc::now().with_timezone(&Chicago);
    let yesterday = now.date_naive() - ChronoDuration::days(1);
    let today = now.date_naive();
    let tomorrow = now.date_naive() + ChronoDuration::days(1);

    let (
        programs,
        college_yesterday,
        nfl_yesterday,
        mlb_yesterday,
        college_today,
        nfl_today,
        mlb_today,
        college_tomorrow,
        nfl_tomorrow,
        mlb_tomorrow,
    ) = tokio::join!(
        fetch_local_programs(now.date_naive(), now.hour()),
        fetch_espn_scoreboard(client, yesterday, "football", "college-football", true),
        fetch_espn_scoreboard(client, yesterday, "football", "nfl", false),
        fetch_espn_scoreboard(client, yesterday, "baseball", "mlb", false),
        fetch_espn_scoreboard(client, today, "football", "college-football", true),
        fetch_espn_scoreboard(client, today, "football", "nfl", false),
        fetch_espn_scoreboard(client, today, "baseball", "mlb", false),
        fetch_espn_scoreboard(client, tomorrow, "football", "college-football", true),
        fetch_espn_scoreboard(client, tomorrow, "football", "nfl", false),
        fetch_espn_scoreboard(client, tomorrow, "baseball", "mlb", false),
    );

    let mut games = Vec::new();
    append_games(
        &mut games,
        college_yesterday,
        "yesterday's college football",
        Some("in"),
    );
    append_games(&mut games, nfl_yesterday, "yesterday's NFL", Some("in"));
    append_games(&mut games, mlb_yesterday, "yesterday's MLB", Some("in"));
    append_games(&mut games, college_today, "college football", None);
    append_games(&mut games, nfl_today, "NFL", None);
    append_games(&mut games, mlb_today, "MLB", None);
    append_games(
        &mut games,
        college_tomorrow,
        "tomorrow's college football",
        Some("pre"),
    );
    append_games(&mut games, nfl_tomorrow, "tomorrow's NFL", Some("pre"));
    append_games(&mut games, mlb_tomorrow, "tomorrow's MLB", Some("pre"));

    games.sort_by_key(|game| {
        (
            game.sport.clone(),
            game.network.clone(),
            game.away.abbreviation.clone(),
            game.home.abbreviation.clone(),
        )
    });
    games.dedup_by(|left, right| {
        left.sport == right.sport
            && left.away.abbreviation == right.away.abbreviation
            && left.home.abbreviation == right.home.abbreviation
    });

    let now_minutes = (now.hour() * 60 + now.minute()) as i32;
    let view = local_sports_view(programs?, &games, now_minutes);
    let updated_at = now.format("%-I:%M %p").to_string();
    Ok(render_page(&view, now_minutes, &updated_at))
}

async fn fetch_local_programs(date: chrono::NaiveDate, hour: u32) -> Result<Vec<Program>> {
    // The provider sets encrypted locale cookies. Detail pages ignore region query
    // parameters, so they must share this cookie jar with the schedule requests.
    let client = Client::builder()
        .user_agent("AntennaSports/0.1 (+private personal TV guide)")
        .cookie_store(true)
        .timeout(Duration::from_secs(20))
        .build()?;
    let [previous, current, next] = periods_around(date, hour);
    let (previous, current, next) = tokio::try_join!(
        fetch_local_period(&client, previous),
        fetch_local_period(&client, current),
        fetch_local_period(&client, next),
    )?;
    let mut programs = [previous, current, next].concat();

    let urls = programs
        .iter()
        .filter_map(|program| program.detail_url.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut details = JoinSet::new();
    for url in urls {
        let client = client.clone();
        details.spawn(async move {
            let result = async {
                client
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await
            }
            .await;
            (url, result)
        });
    }
    while let Some(result) = details.join_next().await {
        let (url, result) = result.context("join local airing details request")?;
        match result {
            Ok(html) => {
                for program in &mut programs {
                    if program.detail_url.as_deref() == Some(&url)
                        && let Some(title) = program
                            .airing_id
                            .as_deref()
                            .and_then(|id| episode_title_for_airing(&html, id))
                    {
                        program.title = title;
                    }
                }
            }
            Err(error) => warn!(%error, %url, "local matchup unavailable; keeping guide title"),
        }
    }
    Ok(programs)
}

async fn fetch_local_period(client: &Client, period: Period) -> Result<Vec<Program>> {
    let date = period.date.to_string();
    let body = client
        .get("https://www.ontvtonight.com/guide/schedule")
        .query(&[
            ("provider", "Y341768466"),
            ("region", "78746"),
            ("TVperiod", period.name),
            ("date", date.as_str()),
        ])
        .send()
        .await
        .context("request Austin antenna schedule")?
        .error_for_status()
        .context("Austin antenna schedule returned an error")?
        .text()
        .await?;
    let mut programs = parse_schedule(&decode_schedule_response(&body)?)?;
    // Keep the product's football/baseball scope before replacing generic titles
    // with matchup names from the detail pages.
    programs.retain(|program| {
        let title = program.title.to_ascii_lowercase();
        title.contains("football")
            || title.contains("nfl")
            || title.contains("baseball")
            || title.contains("world series")
    });
    for program in &mut programs {
        program.start_minutes += period.day_offset * 1440;
        program.end_minutes += period.day_offset * 1440;
    }
    Ok(programs)
}

fn append_games(
    destination: &mut Vec<FootballGame>,
    result: Result<Vec<FootballGame>>,
    source: &str,
    required_state: Option<&str>,
) {
    match result {
        Ok(mut games) => {
            if let Some(state) = required_state {
                games.retain(|game| game.state == state);
            }
            destination.extend(games);
        }
        Err(error) => warn!(%error, %source, "ESPN scores unavailable"),
    }
}

async fn fetch_espn_scoreboard(
    client: &Client,
    date: chrono::NaiveDate,
    sport: &str,
    league: &str,
    include_rankings: bool,
) -> Result<Vec<FootballGame>> {
    let date = date.format("%Y%m%d").to_string();
    let url = format!("https://site.api.espn.com/apis/site/v2/sports/{sport}/{league}/scoreboard");
    let body = client
        .get(url)
        .query(&[("dates", date.as_str()), ("limit", "1000")])
        .send()
        .await
        .context("request football scoreboard")?
        .error_for_status()
        .context("football scoreboard returned an error")?
        .text()
        .await
        .context("read football scoreboard")?;
    let mut games = parse_football_scoreboard(&body, include_rankings)?;
    for game in &mut games {
        game.sport = sport.to_string();
    }
    Ok(games)
}

fn error_page(message: &str) -> String {
    format!(
        "<!doctype html><meta name=viewport content='width=device-width'><title>Antenna Sports</title><style>body{{margin:0;background:#050505;color:#f5f5f7;font:17px system-ui;display:grid;place-items:center;min-height:100vh}}main{{max-width:560px;padding:32px}}h1{{font-size:48px;letter-spacing:-.05em;margin:0 0 16px}}p{{color:#a1a1a6;line-height:1.5}}</style><main><h1>Signal lost.</h1><p>{message}</p></main>"
    )
}
