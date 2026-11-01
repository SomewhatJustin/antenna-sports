# Austin channel 42 recurrence — 2026-11-01

## Observed production failure

Read-only `GET http://127.0.0.1:8790/sports/refresh` returned the following live cards at approximately 22:13 UTC (4:13 PM Central):

- CBS / KEYE / 42.1: Chiefs at Broncos
- CBS / KEYE / 42.1: Patriots at Dolphins
- FOX / KTBC / 7.1: Chargers at Rams

It also labeled national regional finals as local CBS/FOX broadcasts. The response footer said `Football and baseball broadcasts and scores from ESPN.` The local guide instead specified `Kansas City Chiefs at Denver Broncos` for KEYE's 3:25–6:30 PM slot; the earlier local game was `Baltimore Ravens at Buffalo Bills`.

Full capture: `/home/justin/apps/antenna-sports-evidence-2026-11-01/production-refresh.html`. Requests only read existing resources; no production process or binary was changed.

## History and why the same symptom returned

Remote history contains three commits:

- `38e8452fc16da5415f56150cfac1f8a98e853892` (2026-10-25 17:53 UTC): initial public release. `sports_view_from_games` mapped **every national CBS game** to KEYE 42.1, regardless of Austin carriage. Multiple simultaneous regional games therefore became multiple local cards.
- `d7d84902aa8bf0d39f4ad172c3e9dea616c182af` (2026-10-25 17:56 UTC): CI action hardening only.
- `0d82043d2efedc9ca650f19d84b04d8726053645` (2026-10-25 18:09 UTC): the actual prior application fix, `Filter sports by Austin antenna listings before adding ESPN scores`. It uses the Austin guide for carriage, and ESPN only for score/state enrichment. It also preserves encrypted locale cookies while resolving airing details and merges period copies.

At investigation time `/home/justin/apps/antenna-sports` was still at `d7d8490`, one commit behind the fetched remote, with pre-existing uncommitted loading/refresh changes in `README.md` and `src/main.rs`. Its main still called `sports_view_from_games`, not `local_sports_view`. Those files were preserved untouched. The installed binary and that checkout's release artifact had the same SHA-256:

`76b9821e86e4463af08dc5d7ade4a977b6b74914d083575a6162d78d4c9d3efd`

The running user service executed `/home/justin/.local/bin/antenna-sports` and restarted on 2026-11-01 at 18:09:25 and 18:10:00 UTC. Its loading shell matched the uncommitted local refresh implementation, while its completed response matched the old national-game model. This is strong evidence that a stale checkout was rebuilt/reinstalled, reintroducing the old bug; the exact command/caller responsible is not established here.

**The existing `deployed-sha` file still said `0d82043...`, despite this contradictory executable/response evidence. Do not trust that marker alone as proof of the running source revision.** Deployment/CI investigation is separate from this application-only change.

## Additional application regression and fix

Starting from the fetched prior application fix in an isolated worktree, tests reproduced two remaining ways a local guide could yield simultaneous local games:

1. Two different local airings with overlapping schedule windows both became live.
2. Two distinct local airings with the same start time both became live.

A third failing test showed that two earlier games whose national scores still said `in` could both overrun into one later postgame placeholder. Existing overrun matching checked only network, without requiring adjacent local slots.

The fix:

- Truncates an earlier local slot at the next distinct scheduled start on the same network/station/channel, before ESPN state enrichment. National live state cannot override the next local game slot.
- Omits contradictory equal-start local airings with a structured warning rather than choosing an arbitrary regional matchup or showing both.
- Only extends an immediately adjacent game into postgame on its own station/channel.
- Retains merging of identical period copies and simultaneous games on distinct channels.

`tests/channel_exclusivity.rs` was run before these changes: the overlap and equal-start tests failed as expected (`2` live cards instead of `1`, and ambiguous live cards not omitted). The overrun test was subsequently added and separately failed (`2` instead of `1`) before its fix. RED logs are retained in the evidence directory.

`tests/current_provider_capture.rs` replays the real November 1 guide and ESPN capture through parsing, airing-detail resolution, merging, enrichment, classification, and rendering. ESPN contains two live CBS games; the rendered local result contains only Chiefs/Broncos, plus the correctly local earlier Ravens/Bills final. Patriots/Dolphins are absent. This is a current captured-state replay, **not** a reconstruction of past live states. The four small dated fixtures preserve only the relevant guide attributes, airing titles, and scoreboard fields; unrelated markup and unused JSON fields were removed.

## Historical API investigation: October 11–31, 2026

All 21 preceding dates were queried with the application's real endpoint parameters:

- Guide: `/guide/schedule?provider=Y341768466&region=78746&TVperiod=Afternoon&date=YYYY-MM-DD`
- ESPN: `/apis/site/v2/sports/{sport}/{league}/scoreboard?dates=YYYYMMDD&limit=1000`, for NFL, college football, and MLB.

Results from 84 actual requests:

- Guide: 21 HTTP 200 responses, but October 11–30 contain **zero schedule rows**. October 31 contains 12 rows, **none of the four required major Austin stations**. Therefore these responses do not supply the local carriage evidence required to replay the requested three weeks. HTTP success is not historical support.
- ESPN NFL: 43 events across 21 successful dated requests.
- ESPN college football: 106 events across 21 successful dated requests.
- ESPN MLB: 16 events across 21 successful dated requests.
- All 165 ESPN events match the requested date in America/Chicago; all are now `post`. This establishes historical completed-score data, **not historical in-progress snapshots or Austin station carriage**.

A valid full local/live replay of those three weeks was therefore **not performed**. Substituting today's guide or changing final games to `in` would fabricate historical evidence. It requires archived dated Austin guide/detail pages plus actual in-progress scoreboard snapshots (or another verified regional-carriage archive). Afternoon coverage alone already fails the required local-station prerequisite; no additional periods are represented as validated.

Raw responses and a per-request manifest containing requested dates, HTTP status, event dates/counts, guide row counts, filenames, and SHA-256 hashes are preserved in:

`/home/justin/apps/antenna-sports-evidence-2026-11-01/history/`

## Verification and release boundary

Use `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`. All passed locally; the full suite has 30 passing tests, including six new tests (five channel invariants and one real captured-state pipeline replay), in addition to the existing 24 tests. The added-lines security scan found no secrets, shell injection, eval/exec, or unsafe deserialization patterns. An independent Codex CLI review was attempted but could not run because its OAuth token refresh returned HTTP 401; no independent-review approval is claimed, and that remains a review limitation.

This work changes application/library code, tests/fixtures, and this report only. It does not alter CI workflows, the dirty original checkout, the service, or the deployed binary. No remote push or deployment is authorized or performed. A separately approved deployment must build the corrected revision (including `0d82043`, not the stale original checkout), preserve/rebase any desired loading-shell work onto it, install/restart the service, and inspect completed `/sports/refresh` content—not just `/health` or a SHA marker—to verify that KEYE 42.1 has one locally carried live matchup.
