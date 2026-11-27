//! Today's big games, and the channels in the catalog that carry them.
//!
//! The schedule comes from ESPN's public scoreboard API: no key, no account,
//! and nothing about the user is sent. Panels rename their event channels
//! for each game ("NFL | 03 - 4:30pm Eagles at Cowboys", "Live | Eagles vs.
//! Cowboys | … | FR: SOCCER PPV 28"), so a channel whose name carries both
//! teams' names is a feed of that game. The network ESPN lists (FOX, NBC)
//! is matched as a fallback.

use std::time::Duration;

use thiserror::Error;

use crate::xtream::LiveStream;

/// ESPN answers in well under a second; ten covers a slow mobile uplink.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A league's scoreboard over two days is 50 to 300 KB; 4 MB is room for
/// a busy World Cup day without letting a broken response fill memory.
const RESPONSE_BYTES_MAX: usize = 4 * 1024 * 1024;
/// Shortest team word used for matching: shorter ones ("FC", "AC", "LA")
/// appear in unrelated channel names.
const TEAM_WORD_CHARS_MIN: usize = 4;

const SCOREBOARD_BASE: &str = "https://site.api.espn.com/apis/site/v2/sports";

/// The competitions we follow: the closed set the frontends can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum League {
    Nfl,
    Nba,
    Nhl,
    Mlb,
    Ligue1,
    ChampionsLeague,
    EuropaLeague,
    ConferenceLeague,
    PremierLeague,
    LaLiga,
    SerieA,
    Bundesliga,
}

impl League {
    pub const ALL: [League; 12] = [
        League::Nfl,
        League::Nba,
        League::Nhl,
        League::Mlb,
        League::Ligue1,
        League::ChampionsLeague,
        League::EuropaLeague,
        League::ConferenceLeague,
        League::PremierLeague,
        League::LaLiga,
        League::SerieA,
        League::Bundesliga,
    ];

    pub fn name(self) -> &'static str {
        match self {
            League::Nfl => "NFL",
            League::Nba => "NBA",
            League::Nhl => "NHL",
            League::Mlb => "MLB",
            League::Ligue1 => "Ligue 1",
            League::ChampionsLeague => "Champions League",
            League::EuropaLeague => "Europa League",
            League::ConferenceLeague => "Conference League",
            League::PremierLeague => "Premier League",
            League::LaLiga => "LaLiga",
            League::SerieA => "Serie A",
            League::Bundesliga => "Bundesliga",
        }
    }

    fn espn_path(self) -> &'static str {
        match self {
            League::Nfl => "football/nfl",
            League::Nba => "basketball/nba",
            League::Nhl => "hockey/nhl",
            League::Mlb => "baseball/mlb",
            League::Ligue1 => "soccer/fra.1",
            League::ChampionsLeague => "soccer/uefa.champions",
            League::EuropaLeague => "soccer/uefa.europa",
            League::ConferenceLeague => "soccer/uefa.europa.conf",
            League::PremierLeague => "soccer/eng.1",
            League::LaLiga => "soccer/esp.1",
            League::SerieA => "soccer/ita.1",
            League::Bundesliga => "soccer/ger.1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventState {
    Scheduled,
    Live,
    Finished,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Team {
    /// "Cowboys", "Marseille".
    pub name: String,
    /// "Dallas Cowboys".
    pub full_name: String,
    pub abbreviation: Option<String>,
    pub logo: Option<String>,
    pub score: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Event {
    pub league: League,
    pub id: String,
    /// Epoch seconds.
    pub start: i64,
    pub state: EventState,
    /// ESPN's short status: "Q3 5:12", "Half", "Final", "8:20 PM EST".
    pub status: Option<String>,
    pub home: Team,
    pub away: Team,
    /// Networks carrying it, as ESPN names them: "Fox", "NBC", "Paramount+".
    pub broadcasts: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SportsError {
    #[error("Sports schedule request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Sports schedule returned HTTP {0}")]
    Status(u16),

    #[error("Sports schedule response is larger than {RESPONSE_BYTES_MAX} bytes")]
    TooLarge,

    #[error("Sports schedule response is unreadable: {0}")]
    Decode(String),
}

/// ESPN's public scoreboard.
pub struct Schedule {
    http: reqwest::Client,
    base: String,
}

impl Schedule {
    pub fn new() -> Result<Self, SportsError> {
        Self::with_base(SCOREBOARD_BASE)
    }

    /// Point at another server: tests use a local fake.
    pub fn with_base(base: &str) -> Result<Self, SportsError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
        })
    }

    /// One league's games on one ESPN day. ESPN days are US Eastern and the
    /// API takes no ranges, so callers ask for the days around now and
    /// filter by start time.
    pub async fn league_events(
        &self,
        league: League,
        day: jiff::civil::Date,
    ) -> Result<Vec<Event>, SportsError> {
        let url = format!(
            "{}/{}/scoreboard?dates={}",
            self.base,
            league.espn_path(),
            day.strftime("%Y%m%d"),
        );
        let mut response = self.http.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(SportsError::Status(response.status().as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > RESPONSE_BYTES_MAX {
                return Err(SportsError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        parse_scoreboard(league, &body)
    }
}

pub fn parse_scoreboard(league: League, body: &[u8]) -> Result<Vec<Event>, SportsError> {
    let scoreboard: wire::Scoreboard =
        serde_json::from_slice(body).map_err(|err| SportsError::Decode(err.to_string()))?;
    Ok(scoreboard
        .events
        .into_iter()
        .filter_map(|event| event_from(league, event))
        .collect())
}

fn event_from(league: League, event: wire::Event) -> Option<Event> {
    let start = parse_start(&event.date)?;
    let competition = event.competitions.into_iter().next()?;
    let mut home = None;
    let mut away = None;
    for competitor in competition.competitors {
        let team = Team {
            name: trimmed(competitor.team.short_display_name),
            full_name: trimmed(competitor.team.display_name),
            abbreviation: competitor.team.abbreviation,
            logo: competitor.team.logo,
            score: competitor.score,
        };
        match competitor.home_away.as_deref() {
            Some("home") => home = Some(team),
            Some("away") => away = Some(team),
            // Neutral listings without a side: fill whichever is free.
            _ if home.is_none() => home = Some(team),
            _ => away = Some(team),
        }
    }
    let state = match event.status.kind.state.as_str() {
        "pre" => EventState::Scheduled,
        "in" => EventState::Live,
        "post" => EventState::Finished,
        // Postponed and cancelled games carry other states: not watchable.
        _ => return None,
    };
    Some(Event {
        league,
        id: event.id,
        start,
        state,
        status: event.status.kind.short_detail,
        home: home?,
        away: away?,
        broadcasts: competition
            .broadcasts
            .into_iter()
            .flat_map(|broadcast| broadcast.names)
            .collect(),
    })
}

fn trimmed(text: Option<String>) -> String {
    text.map(|text| text.trim().to_string()).unwrap_or_default()
}

/// ESPN writes "2026-11-26T21:30Z", without seconds.
fn parse_start(date: &str) -> Option<i64> {
    let with_seconds = match date.len() {
        17 if date.ends_with('Z') => format!("{}:00Z", &date[..16]),
        _ => date.to_string(),
    };
    with_seconds
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|timestamp| timestamp.as_second())
}

/// Channels carrying `event`, best first: event feeds naming both teams,
/// then the networks ESPN lists. Feeds whose name starts with
/// `preferred_region` ("FR") rank first within each group.
pub fn channels_for<'a>(
    event: &Event,
    streams: &'a [LiveStream],
    names: &[String],
    preferred_region: Option<&str>,
    limit: usize,
) -> Vec<&'a LiveStream> {
    assert_eq!(
        streams.len(),
        names.len(),
        "names are the streams' lowercased names"
    );
    let home = team_words(&event.home);
    let away = team_words(&event.away);
    let day = jiff::Timestamp::from_second(event.start)
        .map(|timestamp| timestamp.to_zoned(jiff::tz::TimeZone::UTC).date())
        .ok();
    let region = preferred_region.map(str::to_lowercase);

    let mut feeds: Vec<(u8, &LiveStream)> = streams
        .iter()
        .zip(names)
        .filter(|(stream, _)| !stream.is_adult)
        .filter(|(_, name)| mentions_any(name, &home) && mentions_any(name, &away))
        .filter(|(_, name)| !name.starts_with("end |") && !name.starts_with("end|"))
        .filter(|(_, name)| !is_placeholder(name))
        .filter(|(_, name)| day.is_none_or(|day| dates_agree(name, day)))
        .map(|(stream, name)| (rank(name, region.as_deref()), stream))
        .collect();
    feeds.sort_by_key(|(rank, stream)| (*rank, stream.name.len()));

    let mut chosen: Vec<&LiveStream> = feeds.into_iter().map(|(_, stream)| stream).collect();
    for network in &event.broadcasts {
        if chosen.len() >= limit {
            break;
        }
        if let Some(stream) = network_channel(network, streams, names)
            && !chosen.iter().any(|seen| seen.stream_id == stream.stream_id)
        {
            chosen.push(stream);
        }
    }
    chosen.truncate(limit);
    chosen
}

/// Lowercased names of `streams`, computed once per catalog and reused for
/// every event.
pub fn lowercase_names(streams: &[LiveStream]) -> Vec<String> {
    streams
        .iter()
        .map(|stream| stream.name.to_lowercase())
        .collect()
}

/// Words that identify a team in a channel name: its short name ("Cowboys",
/// "Bayer Leverkusen" → "leverkusen") without club boilerplate.
fn team_words(team: &Team) -> Vec<String> {
    const BOILERPLATE: [&str; 8] = ["fc", "afc", "cf", "ac", "sc", "club", "de", "city"];
    let mut words: Vec<String> = Vec::new();
    for source in [&team.name, &team.full_name] {
        let lowered = source.to_lowercase();
        if lowered.chars().count() >= TEAM_WORD_CHARS_MIN && !words.contains(&lowered) {
            words.push(lowered.clone());
        }
        for word in lowered.split(|c: char| !c.is_alphanumeric()) {
            let is_distinct = word.chars().count() >= TEAM_WORD_CHARS_MIN
                && !BOILERPLATE.contains(&word)
                && !words.iter().any(|seen| seen == word);
            if is_distinct {
                words.push(word.to_string());
            }
        }
    }
    // The last word of "Dallas Cowboys" names the team better than its
    // city, but the city alone ("dallas") would match a news channel; keep
    // both and require the other team too (see `channels_for`).
    words
}

fn mentions_any(name: &str, words: &[String]) -> bool {
    words.iter().any(|word| contains_word(name, word))
}

/// `word` appears in `name` on word boundaries: "lions" must not match
/// "millions".
fn contains_word(name: &str, word: &str) -> bool {
    name.match_indices(word).any(|(start, _)| {
        let before = name[..start].chars().next_back();
        let after = name[start + word.len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
    })
}

/// A name that carries an ISO date ("(2026-12-27 19:00:00)") is a feed for
/// that day; the game's day, or the next for late US games seen from UTC.
fn dates_agree(name: &str, day: jiff::civil::Date) -> bool {
    let bytes = name.as_bytes();
    for window in bytes.windows(10) {
        let is_date = window.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        });
        if !is_date {
            continue;
        }
        // All ten bytes are ASCII, so they are valid UTF-8 on their own.
        let candidate = std::str::from_utf8(window).expect("ASCII is UTF-8");
        let Ok(date) = candidate.parse::<jiff::civil::Date>() else {
            continue;
        };
        let next = day.tomorrow().unwrap_or(day);
        let previous = day.yesterday().unwrap_or(day);
        return date == day || date == next || date == previous;
    }
    true
}

/// 0 is best. Preferred region first, then feeds marked live.
fn rank(name: &str, region: Option<&str>) -> u8 {
    let in_region = region.is_some_and(|region| {
        name.starts_with(&format!("{region}:"))
            || name.starts_with(&format!("{region}|"))
            || name.contains(&format!("| {region}:"))
            || name.contains(&format!(" {region}: "))
    });
    let is_live = name.starts_with("live |") || name.starts_with("live|");
    match (in_region, is_live) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

/// Panels keep idle event slots in the list: "- NO EVENT STREAMING - |".
fn is_placeholder(name: &str) -> bool {
    name.contains("no event")
}

/// The best channel for a broadcast network ESPN names: "Fox" → "US: FOX
/// HD". Streaming services ("Paramount+", "Peacock") are skipped: a panel
/// has dozens of channels per service, and the service's name alone does
/// not say which one carries the game.
fn network_channel<'a>(
    network: &str,
    streams: &'a [LiveStream],
    names: &[String],
) -> Option<&'a LiveStream> {
    const STREAMING: [&str; 6] = [
        "peacock",
        "prime video",
        "netflix",
        "apple tv",
        "dazn",
        "max",
    ];
    let network = network.to_lowercase();
    if network.chars().count() < 2 || network.contains('+') || STREAMING.contains(&network.as_str())
    {
        return None;
    }
    let wanted = format!("us: {network}");
    streams
        .iter()
        .zip(names)
        .filter(|(stream, name)| {
            !stream.is_adult && !is_placeholder(name) && contains_word(name, &wanted)
        })
        // "US: FOX HD" before "US: FOX SPORTS 1 ᴿᴬᵂ": the shortest name is
        // the network itself.
        .min_by_key(|(stream, _)| stream.name.len())
        .map(|(stream, _)| stream)
}

/// ESPN's JSON, slimmed to what we read. Every field tolerates absence.
mod wire {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct Scoreboard {
        #[serde(default)]
        pub events: Vec<Event>,
    }

    #[derive(Deserialize)]
    pub struct Event {
        pub id: String,
        pub date: String,
        pub status: Status,
        #[serde(default)]
        pub competitions: Vec<Competition>,
    }

    #[derive(Deserialize)]
    pub struct Status {
        #[serde(rename = "type")]
        pub kind: StatusType,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatusType {
        pub state: String,
        #[serde(default)]
        pub short_detail: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct Competition {
        #[serde(default)]
        pub competitors: Vec<Competitor>,
        #[serde(default)]
        pub broadcasts: Vec<Broadcast>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Competitor {
        #[serde(default)]
        pub home_away: Option<String>,
        #[serde(default)]
        pub score: Option<String>,
        pub team: Team,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Team {
        #[serde(default)]
        pub display_name: Option<String>,
        #[serde(default)]
        pub short_display_name: Option<String>,
        #[serde(default)]
        pub abbreviation: Option<String>,
        #[serde(default)]
        pub logo: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct Broadcast {
        #[serde(default)]
        pub names: Vec<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtream::StreamId;

    const SCOREBOARD: &str = r#"{"events": [
        {"id": "1", "date": "2026-11-26T21:30Z",
         "status": {"type": {"state": "in", "shortDetail": "Q3 5:12"}},
         "competitions": [{
            "competitors": [
                {"homeAway": "home", "score": "17", "team": {"displayName": "Dallas Cowboys", "shortDisplayName": "Cowboys", "abbreviation": "DAL", "logo": "https://a/dal.png"}},
                {"homeAway": "away", "score": "21", "team": {"displayName": "Philadelphia Eagles", "shortDisplayName": "Eagles", "abbreviation": "PHI"}}
            ],
            "broadcasts": [{"names": ["Fox"]}]
         }]},
        {"id": "2", "date": "2026-11-26T18:00Z",
         "status": {"type": {"state": "postponed"}},
         "competitions": [{"competitors": []}]},
        {"id": "3", "date": "garbage", "status": {"type": {"state": "pre"}}, "competitions": []}
    ]}"#;

    fn stream(id: u64, name: &str) -> LiveStream {
        LiveStream {
            stream_id: StreamId(id),
            name: name.to_string(),
            category_id: None,
            epg_channel_id: None,
            is_adult: false,
            added: None,
            tv_archive: false,
            tv_archive_duration: None,
            direct_source: None,
            stream_icon: None,
        }
    }

    fn game() -> Event {
        parse_scoreboard(League::Nfl, SCOREBOARD.as_bytes())
            .unwrap()
            .remove(0)
    }

    #[test]
    fn scoreboard_parses_and_skips_unwatchable_events() {
        let events = parse_scoreboard(League::Nfl, SCOREBOARD.as_bytes()).unwrap();
        assert_eq!(events.len(), 1, "postponed and undated events are dropped");
        let event = &events[0];
        assert_eq!(event.state, EventState::Live);
        assert_eq!(event.start, 1_795_728_600);
        assert_eq!(event.home.name, "Cowboys");
        assert_eq!(event.away.score.as_deref(), Some("21"));
        assert_eq!(event.broadcasts, vec!["Fox".to_string()]);
        assert!(parse_scoreboard(League::Nfl, b"not json").is_err());
        assert!(parse_scoreboard(League::Nfl, b"{}").unwrap().is_empty());
    }

    #[test]
    fn feeds_need_both_teams_and_the_right_day() {
        let streams = vec![
            stream(1, "NFL  | 03 - 4:30pm Eagles at Cowboys"),
            stream(2, "RK: DALLAS COWBOYS CHEERLEADERS ᴿᴬᵂ"),
            stream(
                3,
                "US (Peacock 075) | Jaguars vs. Cowboys (2026-12-27 19:00:00)",
            ),
            stream(
                4,
                "Live | Eagles vs. Cowboys | all | 8K EXCLUSIVE | FR: SOCCER PPV 28",
            ),
            stream(
                5,
                "End | Eagles vs. Cowboys | all | 8K EXCLUSIVE | US: SOCCER PPV 2",
            ),
            stream(6, "US: FOX HD"),
            stream(7, "US: FOX SPORTS 1"),
            stream(
                8,
                "US (ESPN+ 058) | Eagles vs. Cowboys (2026-12-30 03:00:00)",
            ),
            stream(9, "Eagles Cowboys Millions"),
            stream(
                10,
                "- NO EVENT STREAMING - | Eagles vs Cowboys | US: ESPN+ PPV 12",
            ),
        ];
        let names = lowercase_names(&streams);
        let found = channels_for(&game(), &streams, &names, Some("FR"), 10);
        let ids: Vec<u64> = found.iter().map(|stream| stream.stream_id.0).collect();
        // French live feed first, other feeds next, the FOX network last.
        assert_eq!(ids, vec![4, 9, 1, 6]);

        let capped = channels_for(&game(), &streams, &names, None, 1);
        assert_eq!(capped.len(), 1);
    }

    #[test]
    fn streaming_services_are_not_networks() {
        let streams = vec![
            stream(1, "US: PARAMOUNT+ ORIGINAL 1"),
            stream(2, "US: NBC HD"),
        ];
        let names = lowercase_names(&streams);
        assert!(network_channel("Paramount+", &streams, &names).is_none());
        assert!(network_channel("Peacock", &streams, &names).is_none());
        let nbc = network_channel("NBC", &streams, &names).expect("NBC is a network");
        assert_eq!(nbc.stream_id, StreamId(2));
    }

    #[test]
    fn words_match_on_boundaries_only() {
        assert!(contains_word("eagles at cowboys", "cowboys"));
        assert!(!contains_word("millions", "lions"));
        assert!(contains_word("lions-bears", "lions"));
        assert!(contains_word("us: fox hd", "us: fox"));
        assert!(!contains_word("us: foxtel", "us: fox"));
    }

    #[test]
    fn team_words_drop_club_boilerplate() {
        let team = Team {
            name: "Milan".into(),
            full_name: "AC Milan".into(),
            abbreviation: None,
            logo: None,
            score: None,
        };
        assert_eq!(
            team_words(&team),
            vec!["milan".to_string(), "ac milan".to_string()]
        );
    }
}
