//! Current weather from Open-Meteo, which needs no account or key. The place comes from the internet address through
//! ipwho.is, unless a place was typed in. Runs on a short-lived thread, about twice an hour, while the screen is on.

use crate::http;
use std::str::Chars;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Globalization::{LCMapStringEx, LCMAP_TRADITIONAL_CHINESE};
use windows::core::w;

#[derive(Clone, PartialEq, Debug)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Typed in, so it isn't replaced by the automatic one.
    pub manual: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Hour {
    pub hour: u32,
    pub temperature: f32,
    pub code: u32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Day {
    /// Days since 1970, to tell which one is today once the forecast is a few hours old.
    pub date: i64,
    /// 0 is Sunday.
    pub weekday: u32,
    pub high: f32,
    pub low: f32,
    pub code: u32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Observation {
    pub temperature: f32,
    pub code: u32,
    pub high: f32,
    pub low: f32,
    pub day: bool,
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    /// How far the place's clocks are ahead of UTC, in minutes. Its days aren't the computer's.
    pub utc_offset: Option<i64>,
}

impl Observation {
    /// The date at the place, given the minutes since 1970 in UTC, and the computer's date to go by if that isn't known.
    pub fn today(&self, utc: i64, here: i64) -> i64 {
        self.utc_offset.map_or(here, |offset| (utc + offset).div_euclid(24 * 60))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Storm,
}

/// WMO weather codes.
pub fn describe(code: u32) -> (&'static str, Sky) {
    match code {
        0 => ("晴朗", Sky::Clear),
        1 => ("大致晴朗", Sky::Clear),
        2 => ("局部多雲", Sky::PartlyCloudy),
        3 => ("陰天", Sky::Cloudy),
        45 | 48 => ("有霧", Sky::Fog),
        51..=57 => ("毛毛雨", Sky::Rain),
        61 | 63 | 66 => ("下雨", Sky::Rain),
        65 | 67 => ("大雨", Sky::Rain),
        71..=77 => ("下雪", Sky::Snow),
        80..=82 => ("陣雨", Sky::Rain),
        85 | 86 => ("陣雪", Sky::Snow),
        95..=99 => ("雷雨", Sky::Storm),
        _ => ("未知", Sky::Cloudy),
    }
}

pub struct Request {
    pub place: Option<Place>,
    /// Look the automatic place up again.
    pub locate: bool,
    /// A typed place name to find.
    pub search: Option<String>,
    /// Comes back in the outcome, so a change of place made in the meantime can be noticed.
    pub revision: u32,
}

/// What became of a typed place name.
#[derive(PartialEq, Debug)]
pub enum Search {
    /// None was typed, or it was found.
    Done,
    /// Answered, and there's no such place.
    NotFound(String),
    /// No answer. Worth asking again.
    Unanswered(String),
}

pub struct Outcome {
    /// None if no place could be found.
    pub place: Option<Place>,
    pub observation: Option<Observation>,
    pub located: bool,
    pub search: Search,
    pub revision: u32,
}

pub fn fetch(request: Request) -> Outcome {
    let mut located = false;
    let mut result = Search::Done;
    let place = match &request.search {
        Some(name) => {
            let (found, how) = search(name, http_search);
            result = how;
            found.or(request.place)
        }
        None => match request.place {
            Some(place) if place.manual || !request.locate => Some(place),
            known => {
                let found = locate();
                located = found.is_some();
                found.or(known)
            }
        },
    };
    let observation = place.as_ref().and_then(forecast);
    Outcome { place, observation, located, search: result, revision: request.revision }
}

// What the conversion leaves as it is, because the character is right in other words.
const PLACE_NAMES: [(&str, &str); 5] = [("云林", "雲林"), ("恒春", "恆春"), ("樸子", "朴子"), ("后龍", "後龍"), ("苑里", "苑裡")];

// ipwho.is has no traditional Chinese, only zh-CN.
fn traditional(name: &str) -> String {
    let wide: Vec<u16> = name.encode_utf16().collect();
    if wide.is_empty() {
        return String::new();
    }
    let mut mapped = vec![0u16; wide.len() * 2];
    let length = unsafe {
        LCMapStringEx(w!("zh-TW"), LCMAP_TRADITIONAL_CHINESE, &wide, Some(&mut mapped), None, None, LPARAM(0))
    };
    let converted = if length > 0 { String::from_utf16_lossy(&mapped[..length as usize]) } else { name.to_string() };
    PLACE_NAMES.iter().fold(converted, |text, (from, to)| text.replace(from, to))
}

fn locate() -> Option<Place> {
    let place = parse_location(&http::get("ipwho.is", "/?lang=zh-CN&fields=success,city,latitude,longitude")?)?;
    Some(Place { name: traditional(&place.name), ..place })
}

fn forecast(place: &Place) -> Option<Observation> {
    let path = format!(
        "/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,weather_code,is_day\
         &hourly=temperature_2m,weather_code&forecast_hours=7\
         &daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=5",
        place.latitude, place.longitude
    );
    parse_forecast(&http::get("api.open-meteo.com", &path)?)
}

// A bare 高雄 finds a village of that name in China, where 高雄市 finds the city, and 臺 isn't in the index.
fn spellings(name: &str) -> Vec<String> {
    let plain = name.trim().replace('臺', "台");
    let chinese = plain.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let suffixed = plain.ends_with(['市', '縣', '區', '鎮', '鄉']);
    if chinese && !suffixed { vec![format!("{plain}市"), plain] } else { vec![plain] }
}

fn http_search(path: &str) -> Option<String> {
    http::get("geocoding-api.open-meteo.com", path)
}

// Only a real answer moves on to the next spelling: 高雄 alone would otherwise find that village whenever the request
// for 高雄市 happened to fail.
fn search(name: &str, get: impl Fn(&str) -> Option<String>) -> (Option<Place>, Search) {
    for spelling in spellings(name) {
        let path = format!("/v1/search?name={}&count=1&language=zh&format=json", encode(&spelling));
        let Some(answer) = get(&path).filter(|text| complete(text)) else {
            return (None, Search::Unanswered(name.to_string()));
        };
        if let Some(place) = parse_search(&answer) {
            // Named as it was typed: the geocoder can answer in simplified Chinese, and a place name is too easily
            // spoilt by converting it (里 is not always 裡).
            return (Some(Place { name: name.trim().to_string(), ..place }), Search::Done);
        }
    }
    (None, Search::NotFound(name.to_string()))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => char::from(byte).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

// A response cut off on the way can still contain everything that gets looked up, with numbers ending too soon.
fn complete(text: &str) -> bool {
    text.trim_end().ends_with('}')
}

// These responses are small and fixed in shape, so a few lookups do the job of a JSON library.
fn after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let quoted = format!("\"{key}\"");
    let rest = &text[text.find(&quoted)? + quoted.len()..];
    Some(rest.trim_start().strip_prefix(':')?.trim_start())
}

fn number(text: &str, key: &str) -> Option<f64> {
    let value = after(text, key)?.trim_start_matches(['[', ' ']);
    let end = value.find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')))?;
    value[..end].parse().ok()
}

fn hex_unit(characters: &mut Chars) -> Option<u16> {
    let code: String = characters.by_ref().take(4).collect();
    u16::from_str_radix(&code, 16).ok()
}

fn string(text: &str, key: &str) -> Option<String> {
    let value = after(text, key)?.strip_prefix('"')?;
    let mut result = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => return Some(result),
            '\\' => match characters.next()? {
                'u' => {
                    let first = hex_unit(&mut characters)?;
                    let mut units = vec![first];
                    // Outside the basic plane a character comes in two halves, each with its own \u.
                    if (0xd800..0xdc00).contains(&first) {
                        characters.next().filter(|c| *c == '\\')?;
                        characters.next().filter(|c| *c == 'u')?;
                        units.push(hex_unit(&mut characters)?);
                    }
                    result.push_str(&String::from_utf16(&units).ok()?);
                }
                'n' | 'r' | 't' | 'b' | 'f' => result.push(' '),
                other => result.push(other),
            },
            other => result.push(other),
        }
    }
    None
}

// The items of an array value, as text with any quotes taken off.
fn items<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let Some(rest) = after(text, key).and_then(|value| value.strip_prefix('[')) else { return Vec::new() };
    let end = rest.find(']').unwrap_or(0);
    rest[..end].split(',').map(|item| item.trim().trim_matches('"')).filter(|item| !item.is_empty()).collect()
}

// One entry per item, so a missing value (null) doesn't shift the ones after it onto the wrong hour or day.
fn numbers(text: &str, key: &str) -> Vec<Option<f32>> {
    items(text, key).iter().map(|item| item.parse().ok()).collect()
}

// From 2026-11-07T14:00.
fn parse_hour(time: &str) -> Option<u32> {
    time.split('T').nth(1)?.split(':').next()?.parse().ok()
}

fn parse_day(time: &str) -> Option<(i64, u32)> {
    let (year, month, day) = crate::date::parse(time)?;
    Some((crate::date::days_from_civil(year, month, day), crate::date::weekday(year, month, day)))
}

fn parse_location(text: &str) -> Option<Place> {
    if !complete(text) || !after(text, "success")?.starts_with("true") {
        return None;
    }
    Some(Place {
        name: string(text, "city")?,
        latitude: number(text, "latitude")?,
        longitude: number(text, "longitude")?,
        manual: false,
    })
}

fn parse_forecast(text: &str) -> Option<Observation> {
    if !complete(text) {
        return None;
    }
    let current = after(text, "current")?;
    let daily = after(text, "daily")?;
    let hourly = after(text, "hourly").unwrap_or("");

    // The first entry is the hour that's already under way.
    let hours = items(hourly, "time")
        .iter()
        .zip(numbers(hourly, "temperature_2m"))
        .zip(numbers(hourly, "weather_code"))
        .skip(1)
        .filter_map(|((time, temperature), code)| {
            Some(Hour { hour: parse_hour(time)?, temperature: temperature?, code: code? as u32 })
        })
        .collect();
    let days = items(daily, "time")
        .iter()
        .zip(numbers(daily, "temperature_2m_max"))
        .zip(numbers(daily, "temperature_2m_min"))
        .zip(numbers(daily, "weather_code"))
        .filter_map(|(((time, high), low), code)| {
            let (date, weekday) = parse_day(time)?;
            Some(Day { date, weekday, high: high?, low: low?, code: code? as u32 })
        })
        .collect();
    Some(Observation {
        temperature: number(current, "temperature_2m")? as f32,
        code: number(current, "weather_code")? as u32,
        day: number(current, "is_day")? != 0.0,
        high: number(daily, "temperature_2m_max")? as f32,
        low: number(daily, "temperature_2m_min")? as f32,
        hours,
        days,
        utc_offset: number(text, "utc_offset_seconds").map(|seconds| (seconds / 60.0).round() as i64),
    })
}

fn parse_search(text: &str) -> Option<Place> {
    let first = after(text, "results")?;
    Some(Place {
        name: string(first, "name")?,
        latitude: number(first, "latitude")?,
        longitude: number(first, "longitude")?,
        manual: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = r#"{"latitude":25.06151,"current_units":{"time":"iso8601","temperature_2m":"°C","weather_code":"wmo code","is_day":""},"current":{"time":"2026-11-06T23:45","interval":900,"temperature_2m":22.8,"weather_code":2,"is_day":0},"hourly_units":{"time":"iso8601"},"hourly":{"time":["2026-11-06T23:00","2026-11-07T00:00","2026-11-07T01:00"],"temperature_2m":[22.9,22.5,-1.0],"weather_code":[2,3,61]},"daily_units":{"time":"iso8601","temperature_2m_max":"°C"},"daily":{"time":["2026-11-06","2026-11-07"],"temperature_2m_max":[26.6,25.0],"temperature_2m_min":[-2.2,20.5],"weather_code":[2,61]}}"#;

    #[test]
    fn reads_a_forecast() {
        let observation = parse_forecast(FORECAST).unwrap();
        assert_eq!((observation.temperature, observation.code, observation.day), (22.8, 2, false));
        assert_eq!((observation.high, observation.low), (26.6, -2.2));
        assert_eq!(observation.hours, [Hour { hour: 0, temperature: 22.5, code: 3 }, Hour { hour: 1, temperature: -1.0, code: 61 }]);
        let friday = crate::date::days_from_civil(2026, 11, 6);
        assert_eq!(observation.days[0], Day { date: friday, weekday: 5, high: 26.6, low: -2.2, code: 2 });
        assert_eq!(observation.days[1], Day { date: friday + 1, weekday: 6, high: 25.0, low: 20.5, code: 61 });
    }

    #[test]
    fn reads_how_far_the_place_is_from_utc() {
        let text = FORECAST.replace("\"latitude\":25.06151,", "\"latitude\":25.06151,\"utc_offset_seconds\":-18000,");
        assert_eq!(parse_forecast(&text).unwrap().utc_offset, Some(-300));
        assert_eq!(parse_forecast(FORECAST).unwrap().utc_offset, None);
    }

    #[test]
    fn the_date_is_the_place_s_own() {
        let mut observation = parse_forecast(FORECAST).unwrap();
        let saturday = crate::date::days_from_civil(2026, 11, 7);
        let utc = saturday * 24 * 60 + 2 * 60; // 02:00 UTC, so 10:00 in Taipei.
        assert_eq!(observation.today(utc, saturday), saturday);
        observation.utc_offset = Some(-300); // New York: still Friday evening.
        assert_eq!(observation.today(utc, saturday), saturday - 1);
        observation.utc_offset = Some(9 * 60);
        assert_eq!(observation.today(utc + 15 * 60, saturday), saturday + 1); // Tokyo at 02:00 on Sunday.
    }

    #[test]
    fn place_names_the_conversion_misses() {
        assert_eq!(traditional("云林"), "雲林");
        assert_eq!(traditional("恒春"), "恆春");
        assert_eq!(traditional("朴子"), "朴子");
        assert_eq!(traditional("后龙"), "後龍");
        assert_eq!(traditional("苑里"), "苑裡");
    }

    #[test]
    fn a_forecast_without_hours_still_reads() {
        let text = r#"{"current":{"temperature_2m":20,"weather_code":0,"is_day":1},"daily":{"temperature_2m_max":[21],"temperature_2m_min":[19]}}"#;
        let observation = parse_forecast(text).unwrap();
        assert!(observation.hours.is_empty() && observation.days.is_empty());
    }

    #[test]
    fn reads_a_location() {
        let text = "{\n  \"success\": true,\n  \"city\": \"新北\",\n  \"latitude\": 25.0530639,\n  \"longitude\": 121.5263927\n}";
        let place = parse_location(text).unwrap();
        assert_eq!(place.name, "新北");
        assert_eq!(place.latitude, 25.0530639);
        assert!(!place.manual);
        assert!(parse_location(r#"{"success":false,"message":"limit"}"#).is_none());
    }

    #[test]
    fn reads_a_search() {
        let text = r#"{"results":[{"id":1673820,"name":"高雄市","latitude":22.61626,"longitude":120.31333}],"generationtime_ms":0.1}"#;
        let place = parse_search(text).unwrap();
        assert_eq!((place.name.as_str(), place.manual), ("高雄市", true));
        assert!(parse_search(r#"{"generationtime_ms":0.18}"#).is_none());
    }

    #[test]
    fn reads_escaped_names() {
        assert_eq!(string(r#"{"city":"新北 \"x\""}"#, "city").unwrap(), "新北 \"x\"");
        assert_eq!(string(r#"{"city":"a\/b"}"#, "city").unwrap(), "a/b");
        assert_eq!(string(r#"{"city":"\u65b0\u5317"}"#, "city").unwrap(), "新北");
    }

    #[test]
    fn reads_a_character_outside_the_basic_plane() {
        assert_eq!(string(r#"{"city":"\ud842\udfb7野家"}"#, "city").unwrap(), "𠮷野家");
    }

    #[test]
    fn control_escapes_are_not_letters() {
        assert_eq!(string(r#"{"city":"a\nb\tc"}"#, "city").unwrap(), "a b c");
    }

    #[test]
    fn a_missing_hour_does_not_shift_the_others() {
        let text = r#"{"current":{"temperature_2m":20,"weather_code":0,"is_day":1},"hourly":{"time":["2026-11-07T09:00","2026-11-07T10:00","2026-11-07T11:00","2026-11-07T12:00"],"temperature_2m":[9,10,null,12],"weather_code":[0,0,0,0]},"daily":{"temperature_2m_max":[21],"temperature_2m_min":[19]}}"#;
        let hours = parse_forecast(text).unwrap().hours;
        assert_eq!(hours, [Hour { hour: 10, temperature: 10.0, code: 0 }, Hour { hour: 12, temperature: 12.0, code: 0 }]);
    }

    #[test]
    fn a_missing_day_does_not_shift_the_others() {
        let text = r#"{"current":{"temperature_2m":20,"weather_code":0,"is_day":1},"daily":{"time":["2026-11-07","2026-11-08","2026-11-09"],"temperature_2m_max":[21,22,23],"temperature_2m_min":[19,20,21],"weather_code":[1,null,3]}}"#;
        let days = parse_forecast(text).unwrap().days;
        assert_eq!(days.iter().map(|day| day.code).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(days[1].high, 23.0);
    }

    #[test]
    fn a_response_cut_short_is_refused_wherever_it_ends() {
        for end in 0..FORECAST.len() - 1 {
            if FORECAST.is_char_boundary(end) && !FORECAST[..end].ends_with('}') {
                assert!(parse_forecast(&FORECAST[..end]).is_none(), "{end}");
            }
        }
        let location = r#"{"success":true,"city":"新北","latitude":25.05,"longitude":121.52}"#;
        for end in 0..location.len() - 1 {
            if location.is_char_boundary(end) {
                assert!(parse_location(&location[..end]).is_none(), "{end}");
            }
        }
    }

    const FOUND: &str = r#"{"results":[{"name":"高雄市","latitude":22.6,"longitude":120.3}]}"#;
    const VILLAGE: &str = r#"{"results":[{"name":"高雄","latitude":29.0,"longitude":110.0}]}"#;
    const NOTHING: &str = r#"{"generationtime_ms":0.1}"#;

    #[test]
    fn a_failed_request_does_not_fall_through_to_the_next_spelling() {
        let asked = std::cell::Cell::new(0);
        let (place, result) = search("高雄", |_| {
            asked.set(asked.get() + 1);
            if asked.get() == 1 { None } else { Some(VILLAGE.to_string()) }
        });
        assert_eq!(place, None);
        assert_eq!(result, Search::Unanswered("高雄".to_string()));
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn a_cut_off_answer_counts_as_no_answer() {
        let (place, result) = search("高雄", |_| Some(FOUND[..30].to_string()));
        assert_eq!((place, result), (None, Search::Unanswered("高雄".to_string())));
    }

    #[test]
    fn no_results_moves_on_to_the_next_spelling() {
        let (place, result) = search("高雄", |path| Some(if path.contains("%E5%B8%82") { NOTHING } else { VILLAGE }.to_string()));
        assert_eq!(place.as_ref().map(|place| place.latitude), Some(29.0));
        assert_eq!(place.map(|place| place.name), Some("高雄".to_string()));
        assert_eq!(result, Search::Done);
    }

    #[test]
    fn says_so_when_there_is_no_such_place() {
        let (place, result) = search("zzzz", |_| Some(NOTHING.to_string()));
        assert_eq!((place, result), (None, Search::NotFound("zzzz".to_string())));
    }

    #[test]
    fn simplified_names_become_traditional() {
        assert_eq!(traditional("桃园"), "桃園");
        assert_eq!(traditional("新北"), "新北");
        assert_eq!(traditional("Tokyo"), "Tokyo");
        assert_eq!(traditional(""), "");
    }

    #[test]
    fn bad_responses_are_refused() {
        assert!(parse_forecast("").is_none());
        assert!(parse_forecast("<html>busy</html>").is_none());
        assert!(parse_forecast(r#"{"current":{"temperature_2m":null}}"#).is_none());
    }

    #[test]
    fn spellings_try_the_city_first() {
        assert_eq!(spellings("高雄"), ["高雄市", "高雄"]);
        assert_eq!(spellings("臺北"), ["台北市", "台北"]);
        assert_eq!(spellings("臺北市"), ["台北市"]);
        assert_eq!(spellings(" Tokyo "), ["Tokyo"]);
    }

    #[test]
    fn encodes_names() {
        assert_eq!(encode("New York"), "New%20York");
        assert_eq!(encode("台"), "%E5%8F%B0");
    }

    #[test]
    fn every_code_has_a_description() {
        for code in [0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 99] {
            assert_ne!(describe(code).0, "未知", "{code}");
        }
    }
}
