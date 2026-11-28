//! Local EventKit reads and shared meeting-context selection. No scheduling or capture.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DAY_MS: i64 = 86_400_000;
const QUERY_CHUNK_MS: i64 = 90 * DAY_MS;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Authorization {
    NotDetermined,
    Restricted,
    Denied,
    WriteOnly,
    FullAccess,
    Unsupported,
    Unknown,
}

impl Authorization {
    fn label(self) -> &'static str {
        match self {
            Self::NotDetermined => "not requested",
            Self::Restricted => "restricted",
            Self::Denied => "denied",
            Self::WriteOnly => "write-only; events cannot be read",
            Self::FullAccess => "full access",
            Self::Unsupported => "unsupported on this platform",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct Window {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Window {
    fn parse(start: Option<&str>, end: Option<&str>) -> Result<Self> {
        let parse = |value: &str| {
            DateTime::parse_from_rfc3339(value)
                .map(|date| date.timestamp_millis())
                .context("calendar dates must be RFC 3339 timestamps with a timezone")
        };
        let window = match (start, end) {
            (Some(start), Some(end)) => Self {
                start_ms: parse(start)?,
                end_ms: parse(end)?,
            },
            (None, None) => {
                let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
                Self {
                    start_ms: now,
                    end_ms: now.checked_add(DAY_MS).context("calendar date overflow")?,
                }
            }
            _ => bail!("supply both calendar --start and --end"),
        };
        window.validate()?;
        Ok(window)
    }

    fn validate(self) -> Result<()> {
        if self.end_ms <= self.start_ms {
            bail!("calendar end must be later than start");
        }
        if DateTime::<Utc>::from_timestamp_millis(self.start_ms).is_none()
            || DateTime::<Utc>::from_timestamp_millis(self.end_ms).is_none()
        {
            bail!("calendar date is outside the supported timestamp range");
        }
        Ok(())
    }

    fn chunks(self) -> impl Iterator<Item = Self> {
        let mut cursor = self.start_ms;
        std::iter::from_fn(move || {
            if cursor >= self.end_ms {
                return None;
            }
            let next = cursor.saturating_add(QUERY_CHUNK_MS).min(self.end_ms);
            let chunk = Self {
                start_ms: cursor,
                end_ms: next,
            };
            cursor = next;
            Some(chunk)
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Calendar {
    id: String,
    title: String,
    source: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Event {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub declined: bool,
    #[serde(default)]
    pub with_others: bool,
    #[serde(default)]
    pub has_call_link: bool,
}

impl Event {
    fn overlaps(&self, window: Window) -> bool {
        if self.start_ms == self.end_ms {
            window.start_ms <= self.start_ms && self.start_ms < window.end_ms
        } else {
            self.start_ms < window.end_ms && self.end_ms > window.start_ms
        }
    }

    fn eligible(&self) -> bool {
        self.end_ms > self.start_ms
            && !self.title.trim().is_empty()
            && !self.all_day
            && !self.cancelled
            && !self.declined
            && (self.with_others || self.has_call_link)
    }
}

fn select_event(events: &[Event], recording: Window) -> Option<Event> {
    let recording_duration = i128::from(recording.end_ms) - i128::from(recording.start_ms);
    if recording_duration <= 0 {
        return None;
    }
    events
        .iter()
        .filter(|event| event.eligible())
        .filter_map(|event| {
            let overlap = i128::from(event.end_ms.min(recording.end_ms))
                - i128::from(event.start_ms.max(recording.start_ms));
            let duration = i128::from(event.end_ms) - i128::from(event.start_ms);
            (overlap > 0 && overlap * 4 >= duration.min(recording_duration))
                .then_some((event, overlap, duration))
        })
        .min_by(|(a, ao, ad), (b, bo, bd)| {
            bo.cmp(ao)
                .then(ad.cmp(bd))
                .then(a.start_ms.cmp(&b.start_ms))
                .then((&a.calendar_id, &a.id, &a.title).cmp(&(&b.calendar_id, &b.id, &b.title)))
        })
        .map(|(event, _, _)| event.clone())
}

fn has_call_link(text: &str) -> bool {
    const HOSTS: &[&str] = &[
        "meet.google.com",
        "zoom.us",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
        "whereby.com",
        "around.co",
        "facetime.apple.com",
    ];
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|c| {
            matches!(
                c,
                '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '"' | '\''
            )
        });
        let url = reqwest::Url::parse(token)
            .or_else(|_| reqwest::Url::parse(&format!("https://{token}")));
        url.ok()
            .filter(|url| matches!(url.scheme(), "http" | "https" | "zoommtg" | "msteams"))
            .and_then(|url| {
                url.host_str().map(|host| {
                    let host = host.trim_end_matches('.');
                    HOSTS.iter().any(|known| {
                        host == *known
                            || host
                                .strip_suffix(known)
                                .is_some_and(|prefix| prefix.ends_with('.'))
                    })
                })
            })
            .unwrap_or(false)
    })
}

#[derive(Deserialize, Serialize)]
struct Preferences {
    version: u8,
    name_meetings: bool,
}

impl Preferences {
    fn path() -> PathBuf {
        std::env::var_os("FSCRIPT_CALENDAR_CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::settings::config_directory().join("calendar.json"))
    }

    fn load() -> Result<Self> {
        let path = Self::path();
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)
                .with_context(|| format!("invalid calendar preferences in {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self {
                version: 1,
                name_meetings: false,
            },
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot read calendar preferences in {}", path.display())
                })
            }
        };
        if preferences.version != 1 {
            bail!(
                "unsupported calendar preference version {}",
                preferences.version
            );
        }
        Ok(preferences)
    }

    fn save(&self) -> Result<()> {
        let path = Self::path();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, self)?;
        writeln!(temporary)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&path)
            .with_context(|| format!("cannot save calendar preferences in {}", path.display()))?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Fixture {
    authorization: Authorization,
    #[serde(default)]
    calendars: Vec<Calendar>,
    #[serde(default)]
    events: Vec<Event>,
    request_authorization: Option<Authorization>,
    #[serde(default)]
    request_delay_ms: u64,
}

#[derive(Serialize)]
struct Response {
    version: u8,
    supported: bool,
    authorization: Authorization,
    name_meetings: bool,
    queried: bool,
    window: Option<Window>,
    calendars: Vec<Calendar>,
    events: Vec<Event>,
    matched_event: Option<Event>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Status,
    List,
    Match,
    Connect,
    Disconnect,
}

pub(crate) fn run(
    action: Action,
    start: Option<&str>,
    end: Option<&str>,
    json: bool,
) -> Result<()> {
    let window = if matches!(action, Action::List | Action::Match) {
        Some(Window::parse(start, end)?)
    } else {
        None
    };
    let mut preferences = Preferences::load()?;
    let fixture = fixture()?;
    let mut authorization = fixture
        .as_ref()
        .map(|f| f.authorization)
        .unwrap_or_else(native::authorization);
    let mut response = Response {
        version: 1,
        supported: authorization != Authorization::Unsupported,
        authorization,
        name_meetings: preferences.name_meetings,
        queried: false,
        window,
        calendars: Vec::new(),
        events: Vec::new(),
        matched_event: None,
    };
    if action == Action::Connect {
        if matches!(
            authorization,
            Authorization::NotDetermined | Authorization::WriteOnly
        ) {
            authorization = if let Some(fixture) = &fixture {
                if let Some(path) = std::env::var_os("FSCRIPT_CALENDAR_TEST_REQUEST_LOG") {
                    std::fs::write(path, b"requested\n")?;
                }
                std::thread::sleep(std::time::Duration::from_millis(fixture.request_delay_ms));
                fixture.request_authorization.unwrap_or(authorization)
            } else {
                native::request_access()?
            };
        }
        if authorization == Authorization::Unsupported {
            bail!("native calendar access requires macOS 14.2 or later");
        }
        if authorization != Authorization::FullAccess {
            bail!("Calendar access is {}. Allow Fast Transcript in System Settings > Privacy & Security > Calendars, then connect again.",authorization.label());
        }
        preferences.name_meetings = true;
        preferences.save()?;
        response.authorization = authorization;
        response.name_meetings = true;
    } else if action == Action::Disconnect {
        preferences.name_meetings = false;
        preferences.save()?;
        response.name_meetings = false;
    } else if matches!(action, Action::List | Action::Match)
        && authorization == Authorization::FullAccess
    {
        let window = window.unwrap();
        let (calendars, retained) = read_events(window, fixture)?;
        if action == Action::Match {
            response.matched_event = select_event(&retained, window);
        }
        response.calendars = calendars;
        response.events = retained;
        response.queried = true;
    } else if action == Action::Match {
        bail!(
            "Calendar access is {}; run fscript calendar connect before matching meetings.",
            authorization.label()
        );
    }
    if json {
        return crate::delivery::write_stdout(
            &mut std::io::stdout().lock(),
            format_args!("{}\n", serde_json::to_string_pretty(&response)?),
        );
    }
    let text = if action == Action::Status {
        format!(
            "Calendar access: {}; meeting names {}.",
            response.authorization.label(),
            if response.name_meetings {
                "enabled"
            } else {
                "disabled"
            }
        )
    } else if action == Action::Disconnect {
        "Calendar meeting names disabled. macOS access can be managed in System Settings."
            .to_owned()
    } else if action == Action::Connect {
        "Calendar connected. Recorded meetings can use matching local calendar titles.".to_owned()
    } else if !response.queried {
        format!(
            "Calendar access: {}. Run fscript calendar connect to enable local calendar reads.",
            authorization.label()
        )
    } else if action == Action::Match {
        response
            .matched_event
            .as_ref()
            .map(|event| clean_text(&event.title))
            .unwrap_or_else(|| "No matching calendar meeting.".into())
    } else {
        let window = response.window.unwrap();
        let mut text = format!(
            "Calendar events: {} to {} (UTC)\n",
            timestamp(window.start_ms),
            timestamp(window.end_ms)
        );
        for event in &response.events {
            text.push_str(&format!(
                "{}  {}\n",
                timestamp(event.start_ms),
                clean_text(&event.title)
            ));
        }
        if response.events.is_empty() {
            text.push_str("No events in this period.\n");
        }
        text.trim_end().to_owned()
    };
    crate::delivery::write_stdout(&mut std::io::stdout().lock(), format_args!("{text}\n"))
}

fn timestamp(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn clean_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Background naming never requests permission and never reads disabled calendars.
pub(crate) fn recording_context(window: Window) -> Result<Option<Event>> {
    if !Preferences::load()?.name_meetings {
        return Ok(None);
    }
    window.validate()?;
    let fixture = fixture()?;
    let authorization = fixture
        .as_ref()
        .map(|f| f.authorization)
        .unwrap_or_else(native::authorization);
    if authorization != Authorization::FullAccess {
        return Ok(None);
    }
    Ok(select_event(&read_events(window, fixture)?.1, window))
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub(super) fn authorization() -> Authorization {
        Authorization::Unsupported
    }
    pub(super) fn request_access() -> Result<Authorization> {
        bail!("native calendar access requires macOS 14.2 or later")
    }
    pub(super) fn read(_: Window) -> Result<(Vec<Calendar>, Vec<Event>)> {
        bail!("native calendar access requires macOS 14.2 or later")
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::Bool;
    use objc2_event_kit::*;
    use objc2_foundation::{NSDate, NSError};

    pub(super) fn authorization() -> Authorization {
        match unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) } {
            EKAuthorizationStatus::FullAccess => Authorization::FullAccess,
            EKAuthorizationStatus::NotDetermined => Authorization::NotDetermined,
            EKAuthorizationStatus::Denied => Authorization::Denied,
            EKAuthorizationStatus::Restricted => Authorization::Restricted,
            EKAuthorizationStatus::WriteOnly => Authorization::WriteOnly,
            _ => Authorization::Unknown,
        }
    }

    pub(super) fn request_access() -> Result<Authorization> {
        autoreleasepool(|_| {
            let store = unsafe { EKEventStore::new() };
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let completion = block2::RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let result = if let Some(error) = unsafe { error.as_ref() } {
                    Err(error.localizedDescription().to_string())
                } else {
                    Ok(granted.as_bool())
                };
                let _ = sender.send(result);
            });
            unsafe {
                store.requestFullAccessToEventsWithCompletion(block2::RcBlock::as_ptr(&completion));
            }
            let granted=receiver.recv_timeout(std::time::Duration::from_secs(120)).context("calendar permission request timed out; retry Connect when you can respond to macOS")?.map_err(anyhow::Error::msg)?;
            let state = authorization();
            if granted && state != Authorization::FullAccess {
                bail!("macOS did not make full calendar access available");
            }
            Ok(state)
        })
    }

    fn milliseconds(date: &NSDate) -> Result<i64> {
        let ms = date.timeIntervalSince1970() * 1000.0;
        if !ms.is_finite() || ms < i64::MIN as f64 || ms >= i64::MAX as f64 {
            bail!("EventKit returned an invalid date");
        }
        Ok(ms.round() as i64)
    }

    pub(super) fn read(window: Window) -> Result<(Vec<Calendar>, Vec<Event>)> {
        autoreleasepool(|_| {
            if authorization() != Authorization::FullAccess {
                bail!("calendar access changed before reading");
            }
            let store = unsafe { EKEventStore::new() };
            let calendars = unsafe { store.calendarsForEntityType(EKEntityType::Event) }
                .iter()
                .map(|calendar| unsafe {
                    Calendar {
                        id: calendar.calendarIdentifier().to_string(),
                        title: calendar.title().to_string(),
                        source: calendar
                            .source()
                            .map(|source| source.title().to_string())
                            .unwrap_or_default(),
                    }
                })
                .collect();
            let mut events = Vec::new();
            for chunk in window.chunks() {
                if authorization() != Authorization::FullAccess {
                    bail!("calendar access changed while reading");
                }
                let start = NSDate::dateWithTimeIntervalSince1970(chunk.start_ms as f64 / 1000.0);
                let end = NSDate::dateWithTimeIntervalSince1970(chunk.end_ms as f64 / 1000.0);
                let predicate = unsafe {
                    store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None)
                };
                for event in unsafe { store.eventsMatchingPredicate(&predicate) }.iter() {
                    let mut with_others = false;
                    let mut declined = false;
                    if let Some(attendees) = unsafe { event.attendees() } {
                        for participant in attendees.iter() {
                            if unsafe { participant.isCurrentUser() } {
                                declined = unsafe { participant.participantStatus() }
                                    == EKParticipantStatus::Declined;
                            } else {
                                with_others = true;
                            }
                        }
                    }
                    let link_text = unsafe {
                        [
                            event
                                .URL()
                                .and_then(|url| url.absoluteString())
                                .map(|text| text.to_string()),
                            event.location().map(|text| text.to_string()),
                            event.notes().map(|text| text.to_string()),
                        ]
                    }
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                    events.push(unsafe {
                        Event {
                            id: event
                                .eventIdentifier()
                                .map(|text| text.to_string())
                                .unwrap_or_else(|| event.calendarItemIdentifier().to_string()),
                            calendar_id: event
                                .calendar()
                                .map(|calendar| calendar.calendarIdentifier().to_string())
                                .unwrap_or_default(),
                            title: event.title().to_string(),
                            start_ms: milliseconds(&event.startDate())?,
                            end_ms: milliseconds(&event.endDate())?,
                            all_day: event.isAllDay(),
                            cancelled: event.status() == EKEventStatus::Canceled,
                            declined,
                            with_others,
                            has_call_link: has_call_link(&link_text),
                        }
                    });
                }
            }
            if authorization() != Authorization::FullAccess {
                bail!("calendar access changed while reading");
            }
            Ok((calendars, events))
        })
    }
}

fn fixture() -> Result<Option<Fixture>> {
    std::env::var_os("FSCRIPT_CALENDAR_TEST_JSON")
        .map(|path| -> Result<Fixture> {
            serde_json::from_slice(&std::fs::read(path).context("cannot read calendar test data")?)
                .context("invalid calendar test data")
        })
        .transpose()
}

fn read_events(window: Window, fixture: Option<Fixture>) -> Result<(Vec<Calendar>, Vec<Event>)> {
    let (mut calendars, events) = if let Some(fixture) = fixture {
        (fixture.calendars, fixture.events)
    } else {
        native::read(window)?
    };
    calendars.sort_by(|a, b| (&a.source, &a.title, &a.id).cmp(&(&b.source, &b.title, &b.id)));
    let mut retained = Vec::new();
    let mut seen = BTreeSet::new();
    for mut event in events {
        if event.end_ms < event.start_ms
            || DateTime::<Utc>::from_timestamp_millis(event.start_ms).is_none()
            || DateTime::<Utc>::from_timestamp_millis(event.end_ms).is_none()
        {
            bail!("calendar returned an event with invalid dates");
        }
        event.title = event.title.trim().to_owned();
        if event.overlaps(window)
            && (event.id.is_empty()
                || seen.insert((
                    event.calendar_id.clone(),
                    event.id.clone(),
                    event.start_ms,
                    event.end_ms,
                )))
        {
            retained.push(event);
        }
    }
    retained.sort_by(|a, b| {
        (a.start_ms, a.end_ms, &a.calendar_id, &a.id, &a.title).cmp(&(
            b.start_ms,
            b.end_ms,
            &b.calendar_id,
            &b.id,
            &b.title,
        ))
    });
    Ok((calendars, retained))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, start: i64, end: i64) -> Event {
        Event {
            id: id.into(),
            calendar_id: "work".into(),
            title: id.into(),
            start_ms: start,
            end_ms: end,
            all_day: false,
            cancelled: false,
            declined: false,
            with_others: true,
            has_call_link: false,
        }
    }

    #[test]
    fn matching_filters_and_full_recording_overlap_are_deterministic() {
        let window = Window {
            start_ms: 20,
            end_ms: 40,
        };
        let long = event("All hands", 0, 100);
        let short = event("Team", 10, 50);
        let mut personal = event("Focus", 20, 40);
        personal.with_others = false;
        let mut cancelled = event("Cancelled", 20, 40);
        cancelled.cancelled = true;
        let mut declined = event("Declined", 20, 40);
        declined.declined = true;
        let mut all_day = event("All day", 20, 40);
        all_day.all_day = true;
        assert_eq!(
            select_event(
                &[long, personal, cancelled, declined, all_day, short.clone()],
                window
            ),
            Some(short)
        );
        let window = Window {
            start_ms: 0,
            end_ms: 100,
        };
        assert!(select_event(&[event("Quarter", 75, 175)], window).is_some());
        assert!(select_event(&[event("Too late", 76, 176)], window).is_none());
    }

    #[test]
    fn differential_matching_agrees_with_an_independent_time_grid() {
        let window = Window {
            start_ms: 32,
            end_ms: 96,
        };
        for seed in 1..=128u64 {
            let mut random = seed;
            let mut events = Vec::new();
            for index in 0..128 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let start = (random % 160) as i64;
                let mut event = event(
                    &format!("{index:03}"),
                    start,
                    start + ((random >> 10) % 100) as i64,
                );
                event.with_others = random & 8 != 0;
                event.has_call_link = random & 16 != 0;
                event.all_day = random % 13 == 0;
                event.declined = random % 17 == 0;
                event.cancelled = random % 19 == 0;
                events.push(event);
            }
            let mut reference: Vec<_> = events
                .iter()
                .filter_map(|event| {
                    if event.all_day
                        || event.declined
                        || event.cancelled
                        || (!event.with_others && !event.has_call_link)
                        || event.end_ms <= event.start_ms
                    {
                        return None;
                    }
                    let covered = (32..96)
                        .filter(|t| event.start_ms <= *t && *t < event.end_ms)
                        .count();
                    let shorter = (event.end_ms - event.start_ms).min(64) as f64;
                    (covered as f64 / shorter >= 0.25).then_some((event, covered))
                })
                .collect();
            reference.sort_by_key(|(e, count)| {
                (
                    std::cmp::Reverse(*count),
                    e.end_ms - e.start_ms,
                    e.start_ms,
                    &e.calendar_id,
                    &e.id,
                    &e.title,
                )
            });
            let expected = reference.first().map(|(event, _)| (*event).clone());
            assert_eq!(select_event(&events, window), expected);
            events.reverse();
            assert_eq!(select_event(&events, window), expected);
        }
    }

    #[test]
    fn property_query_chunks_cover_the_full_period_without_gaps_or_four_year_truncation() {
        for seed in 1..=256i64 {
            let window = Window {
                start_ms: -seed * DAY_MS,
                end_ms: seed * DAY_MS * 20,
            };
            let chunks: Vec<_> = window.chunks().collect();
            assert_eq!(chunks.first().unwrap().start_ms, window.start_ms);
            assert_eq!(chunks.last().unwrap().end_ms, window.end_ms);
            assert!(chunks
                .iter()
                .all(|c| c.end_ms > c.start_ms && c.end_ms - c.start_ms <= QUERY_CHUNK_MS));
            assert!(chunks
                .windows(2)
                .all(|pair| pair[0].end_ms == pair[1].start_ms));
            assert_eq!(
                chunks.iter().map(|c| c.end_ms - c.start_ms).sum::<i64>(),
                window.end_ms - window.start_ms
            );
        }
        let parsed = Window::parse(
            Some("2026-11-27T09:00:00-03:00"),
            Some("2026-11-27T13:00:00Z"),
        )
        .unwrap();
        assert_eq!(parsed.end_ms - parsed.start_ms, 3_600_000);
        assert!(Window::parse(Some("2026-11-27"), Some("2026-11-28")).is_err());
    }

    #[test]
    fn property_call_links_match_hosts_rather_than_substrings_or_usernames() {
        for suffix in ["", "/j/123?code=4", ":443/j/123"] {
            assert!(has_call_link(&format!(
                "Join (https://us02.zoom.us{suffix})"
            )));
            assert!(!has_call_link(&format!(
                "https://zoom.us.evil.example{suffix}"
            )));
            assert!(!has_call_link(&format!(
                "https://zoom.us@evil.example{suffix}"
            )));
            assert!(!has_call_link(&format!("https://evilzoom.us{suffix}")));
        }
        assert!(has_call_link("meet.google.com/abc-defg-hij"));
        assert!(has_call_link("zoommtg://zoom.us/join?confno=12"));
        assert!(!has_call_link("https://example.test/zoom.us"));
    }
}
