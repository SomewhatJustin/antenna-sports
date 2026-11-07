use std::{
    future::Future,
    pin::Pin,
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
use tokio::{
    net::TcpListener,
    sync::{Mutex, watch},
    task::JoinSet,
};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

const PORT: u16 = 8790;

type Loader = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<String>> + Send>> + Send + Sync>;
type Outcome = Option<std::result::Result<String, String>>;

#[derive(Clone)]
struct AppState {
    loader: Loader,
    cache: Arc<Mutex<Cache>>,
}

#[derive(Default)]
struct Cache {
    page: Option<CachedPage>,
    refresh: Option<watch::Receiver<Outcome>>,
}

impl AppState {
    fn new(loader: Loader) -> Self {
        Self {
            loader,
            cache: Arc::new(Mutex::new(Cache::default())),
        }
    }

    // Publish the flight under the lock before spawning: all callers join it.
    async fn flight(&self, retry: bool) -> watch::Receiver<Outcome> {
        let mut cache = self.cache.lock().await;
        if let Some(rx) = &cache.refresh {
            let failed = matches!(&*rx.borrow(), Some(Err(_)));
            let fresh = cache
                .page
                .as_ref()
                .is_some_and(|p| p.created.elapsed() < Duration::from_secs(120));
            if rx.borrow().is_none() || fresh || (failed && !retry) {
                return rx.clone();
            }
        }
        let (tx, rx) = watch::channel(None);
        cache.refresh = Some(rx.clone());
        let state = self.clone();
        tokio::spawn(async move {
            // A detached worker survives request cancellation; the join also contains panics.
            let loader = state.loader.clone();
            let task = tokio::spawn(async move {
                tokio::time::timeout(Duration::from_secs(30), loader()).await
            });
            let result = match task.await {
                Ok(Ok(Ok(html))) => Ok(html),
                other => {
                    warn!(?other, "schedule refresh failed");
                    Err("The schedule source did not answer. Please retry.".to_string())
                }
            };
            let mut cache = state.cache.lock().await;
            if let Ok(html) = &result {
                cache.page = Some(CachedPage {
                    created: Instant::now(),
                    html: html.clone(),
                });
            }
            tx.send_replace(Some(result));
        });
        rx
    }
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

    let state = AppState::new(Arc::new(move || {
        let client = client.clone();
        Box::pin(async move { build_live_page(&client).await })
    }));
    let app = Router::new()
        .route("/health", get(health))
        .route("/sports/health", get(health))
        .route("/sports/refresh", get(refresh).post(retry_refresh))
        // Tailscale Serve strips the /sports mount prefix.
        .route("/refresh", get(refresh).post(retry_refresh))
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

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

async fn index(State(state): State<AppState>) -> Response {
    {
        let cache = state.cache.lock().await;
        if let Some(page) = &cache.page
            && page.created.elapsed() < Duration::from_secs(120)
        {
            return no_store(Html(page.html.clone()).into_response());
        }
    }
    state.flight(false).await;
    no_store(Html(loading_page()).into_response())
}

async fn refresh(State(state): State<AppState>) -> Response {
    refresh_response(state, false).await
}

async fn retry_refresh(State(state): State<AppState>) -> Response {
    refresh_response(state, true).await
}

async fn refresh_response(state: AppState, retry: bool) -> Response {
    let mut rx = state.flight(retry).await;
    loop {
        let outcome = rx.borrow().clone();
        if let Some(result) = outcome {
            return no_store(match result {
                Ok(html) => Html(html).into_response(),
                Err(message) => (StatusCode::SERVICE_UNAVAILABLE, message).into_response(),
            });
        }
        if rx.changed().await.is_err() {
            return no_store(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Refresh interrupted. Please retry.",
                )
                    .into_response(),
            );
        }
    }
}

fn loading_page() -> String {
    let view = local_sports_view(Vec::new(), &[], 0);
    let base = render_page(&view, 0, "—");
    let head = base
        .split("<body>")
        .next()
        .unwrap()
        .replace("<meta http-equiv=\"refresh\" content=\"300\">", "");
    format!(
        r##"{head}<body><main aria-busy="true">
<header><div class="brand">Antenna Sports</div><div class="location">Austin, TX · 78746<br>ABC · CBS · FOX · NBC</div></header>
<section><p class="eyebrow">ON YOUR ANTENNA NOW</p><h1>Finding your games.</h1><p class="subhead" id="refresh-status" role="status">Loading live football and baseball on your antenna…</p>
<div class="live-grid" aria-hidden="true"><div class="card skeleton"></div><div class="card skeleton"></div></div>
<button id="retry" type="button" hidden>Retry refresh</button><noscript><p>JavaScript is required to load live scores. Reload this page after a moment to see the refreshed schedule.</p></noscript></section></main>
<style>.refresh-error .live-grid{{display:none}}.skeleton{{min-height:210px;background:linear-gradient(110deg,#141414 25%,#222 45%,#141414 65%);background-size:250% 100%;animation:shimmer 1.6s linear infinite}}[hidden]{{display:none!important}}@keyframes shimmer{{to{{background-position:-250% 0}}}}@media(prefers-reduced-motion:reduce){{.skeleton{{animation:none}}}}</style>
<script>
(() => {{
const retry = document.querySelector('#retry');
async function load(method) {{
 retry.hidden = true;
 const main = document.querySelector('main');
 main.classList.remove('refresh-error');
 main.querySelector('h1').textContent = 'Finding your games.';
 main.setAttribute('aria-busy','true');
 document.querySelector('#refresh-status').textContent = 'Loading live scores…';
 const controller = new AbortController();
 const timeout = setTimeout(() => controller.abort(), 35000);
 try {{
  const response = await fetch('/sports/refresh', {{method, cache:'no-store', signal:controller.signal}});
  if (!response.ok) throw new Error('Schedule refresh failed. Please retry.');
  const doc = new DOMParser().parseFromString(await response.text(), 'text/html');
  const content = doc.querySelector('main');
  if (!content) throw new Error('Invalid schedule response. Please retry.');
  main.replaceWith(content);
  const autoRefresh = doc.querySelector('meta[http-equiv="refresh"]');
  if (autoRefresh) document.head.append(autoRefresh);
 }} catch (error) {{
  main.classList.add('refresh-error');
  main.querySelector('h1').textContent = 'Signal lost.';
  main.setAttribute('aria-busy','false');
  document.querySelector('#refresh-status').textContent = 'Unable to load the schedule. Please retry.';
  retry.hidden = false;
 }} finally {{ clearTimeout(timeout); }}
}}
retry.addEventListener('click', () => load('POST'));
load('GET');
}})();
</script></body></html>"##
    )
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

    require_available_source(&[college_today.is_ok(), nfl_today.is_ok(), mlb_today.is_ok()])?;
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

fn require_available_source(available: &[bool]) -> Result<()> {
    anyhow::ensure!(
        available.iter().any(|ok| *ok),
        "all current scoreboard sources failed"
    );
    Ok(())
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

#[cfg(test)]
mod refresh_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Semaphore;

    fn fake() -> (AppState, Arc<AtomicUsize>, Arc<Semaphore>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(0));
        let (c, g) = (calls.clone(), gate.clone());
        let state = AppState::new(Arc::new(move || {
            let (c, g) = (c.clone(), g.clone());
            Box::pin(async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                g.acquire().await.unwrap().forget();
                if n == 0 {
                    anyhow::bail!("fake source unavailable");
                }
                Ok("<html><body><main>loaded</main></body></html>".into())
            })
        }));
        (state, calls, gate)
    }

    #[test]
    fn rejects_total_source_failure_but_accepts_empty_successful_scoreboards() {
        assert!(require_available_source(&[false, false, false]).is_err());
        assert!(require_available_source(&[false, true, false]).is_ok());
    }

    // Manual browser harness: never compiled into the production binary.
    #[tokio::test]
    #[ignore]
    async fn browser_fixture_server() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = AppState::new(Arc::new(move || {
            let calls = calls.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(4)).await;
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    anyhow::bail!("fixture outage");
                }
                Ok(render_page(
                    &local_sports_view(Vec::new(), &[], 0),
                    0,
                    "12:00 PM",
                ))
            })
        }));
        let app = Router::new()
            .route("/sports/refresh", get(refresh).post(retry_refresh))
            // Tailscale Serve strips the /sports mount prefix.
            .route("/refresh", get(refresh).post(retry_refresh))
            .fallback(get(index))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:18790").await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }

    #[tokio::test]
    async fn immediate_shell_deduplicates_survives_disconnect_errors_and_explicit_retry() {
        let (state, calls, gate) = fake();
        let response = tokio::time::timeout(Duration::from_secs(1), index(State(state.clone())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("aria-busy=\"true\""));
        assert!(!body.contains("<meta http-equiv=\"refresh\""));
        let disconnected = tokio::spawn(refresh(State(state.clone())));
        tokio::task::yield_now().await;
        disconnected.abort();
        let mut visitors = Vec::new();
        let mut waiters = Vec::new();
        for _ in 0..10 {
            visitors.push(tokio::spawn(index(State(state.clone()))));
            waiters.push(tokio::spawn(refresh(State(state.clone()))));
        }
        for visitor in visitors {
            assert_eq!(visitor.await.unwrap().status(), StatusCode::OK);
        }
        assert!(waiters.iter().all(|waiter| !waiter.is_finished()));
        gate.add_permits(1);
        for waiter in waiters {
            let response = waiter.await.unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        assert_eq!(
            refresh(State(state.clone())).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            refresh(State(state.clone())).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let retry = tokio::spawn(retry_refresh(State(state.clone())));
        tokio::task::yield_now().await;
        let other = tokio::spawn(refresh(State(state.clone())));
        gate.add_permits(1);
        assert_eq!(retry.await.unwrap().status(), StatusCode::OK);
        assert_eq!(other.await.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let fresh = index(State(state.clone())).await;
        let body = axum::body::to_bytes(fresh.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8(body.to_vec()).unwrap().contains("loaded"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        state.cache.lock().await.page.as_mut().unwrap().created =
            Instant::now() - Duration::from_secs(120);
        let stale = index(State(state.clone())).await;
        let body = axum::body::to_bytes(stale.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            String::from_utf8(body.to_vec())
                .unwrap()
                .contains("aria-busy")
        );
        gate.add_permits(1);
        assert_eq!(refresh(State(state.clone())).await.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
