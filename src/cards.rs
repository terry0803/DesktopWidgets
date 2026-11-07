//! The cards: what each one shows at each size, and a key that only changes when its picture would.
//!
//! As with Apple's widget families, a bigger size shows more rather than the same thing larger. A medium card is
//! exactly two small ones and the gap between them, and a large one is two medium ones, so everything lines up.
//! Large numbers are thin; small text is medium weight and bright. That contrast is what gives the look.

use crate::date;
use crate::gfx::{Align, Gfx, Weight};
use crate::store::{Settings, Todo};
use crate::weather::{self, Observation, Sky};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Clock,
    Calendar,
    Countdown,
    Progress,
    Todo,
    Weather,
}

pub const KINDS: [Kind; 6] = [Kind::Clock, Kind::Calendar, Kind::Countdown, Kind::Progress, Kind::Todo, Kind::Weather];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Medium,
    Large,
}

pub const GAP: f32 = 22.0;
const SMALL: f32 = 158.0;
const WIDE: f32 = SMALL * 2.0 + GAP;
pub const RADIUS: f32 = 12.0;
const MARGIN: f32 = 14.0; // Apple's 16, less on the Mac desktop.
const TILE_INSET: f32 = 8.0;

impl Size {
    /// Width and height in device-independent pixels.
    pub fn dimensions(self) -> (f32, f32) {
        match self {
            Size::Small => (SMALL, SMALL),
            Size::Medium => (WIDE, SMALL),
            Size::Large => (WIDE, WIDE),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Size::Small => "小",
            Size::Medium => "中",
            Size::Large => "大",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Size::Small => "s",
            Size::Medium => "m",
            Size::Large => "l",
        }
    }

    pub fn from_key(key: &str) -> Option<Size> {
        [Size::Small, Size::Medium, Size::Large].into_iter().find(|size| size.key() == key)
    }
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::Clock => "翻頁時鐘",
            Kind::Calendar => "日曆",
            Kind::Countdown => "倒數／正數",
            Kind::Progress => "時間進度",
            Kind::Todo => "待辦",
            Kind::Weather => "天氣",
        }
    }

    /// The sizes with something to show. A countdown has nothing to fill a large card with.
    pub fn sizes(self) -> &'static [Size] {
        match self {
            Kind::Countdown | Kind::Progress => &[Size::Small, Size::Medium],
            _ => &[Size::Small, Size::Medium, Size::Large],
        }
    }

    pub fn default_size(self) -> Size {
        match self {
            Kind::Clock | Kind::Calendar | Kind::Todo => Size::Medium,
            Kind::Countdown | Kind::Progress | Kind::Weather => Size::Small,
        }
    }

    /// Where the card goes in the default column, from its top-left corner.
    pub fn slot(self) -> (f32, f32) {
        let row = SMALL + GAP;
        match self {
            Kind::Clock => (0.0, 0.0),
            Kind::Calendar => (0.0, row),
            Kind::Progress => (0.0, row * 2.0),
            Kind::Countdown => (row, row * 2.0),
            Kind::Todo => (0.0, row * 3.0),
            Kind::Weather => (-row, 0.0), // Beside the clock: the column already fills a laptop screen.
        }
    }
}

pub fn column_width() -> f32 {
    WIDE
}

#[derive(Clone, Copy)]
pub struct Now {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Now {
    fn minutes(self) -> u32 {
        self.hour * 60 + self.minute
    }

    /// Minutes since 1970, for comparing moments.
    pub fn stamp(self) -> i64 {
        date::days_from_civil(self.year, self.month, self.day) * i64::from(DAY) + i64::from(self.minutes())
    }
}

/// What the weather card shows.
#[derive(Default)]
pub struct WeatherView<'a> {
    pub place: Option<&'a str>,
    pub observation: Option<&'a Observation>,
    /// When it was last updated, if that's long enough ago to mention.
    pub stale_since: Option<String>,
    /// A place name that was typed in and doesn't exist.
    pub not_found: Option<&'a str>,
    pub failed: bool,
    /// The date at the place, in days since 1970.
    pub today: i64,
}

pub struct Data<'a> {
    pub settings: &'a Settings,
    pub todos: &'a [Todo],
    /// How far down the to-do list the card starts.
    pub todo_start: usize,
    /// The list on the card didn't reach the disk.
    pub todos_unsaved: bool,
    /// The file couldn't be read yet, so an empty card doesn't mean there's nothing to do.
    pub todos_unreadable: bool,
    /// What was typed as the countdown's date a moment ago wasn't one.
    pub date_refused: bool,
    pub weather: WeatherView<'a>,
}

const WEEKDAYS: [&str; 7] = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
const WEEK_HEADER: [&str; 7] = ["日", "一", "二", "三", "四", "五", "六"];
const SHORT_WEEKDAYS: [&str; 7] = ["週日", "週一", "週二", "週三", "週四", "週五", "週六"];
const DAY: u32 = 24 * 60;

fn progress(now: Now) -> [(&'static str, u32); 4] {
    let minutes = now.minutes();
    let weekday = date::weekday(now.year, now.month, now.day);
    let month_days = date::days_in_month(now.year, now.month);
    let year_days = if date::is_leap(now.year) { 366 } else { 365 };
    let day_of_year = date::day_of_year(now.year, now.month, now.day);
    let percent = |done: u32, total: u32| done * 100 / total;
    [
        ("今天", percent(minutes, DAY)),
        ("本週", percent(weekday * DAY + minutes, 7 * DAY)),
        ("本月", percent((now.day - 1) * DAY + minutes, month_days * DAY)),
        ("今年", percent((day_of_year - 1) * DAY + minutes, year_days * DAY)),
    ]
}

fn days_left(now: Now, target: (i32, u32, u32)) -> i64 {
    date::days_from_civil(target.0, target.1, target.2) - date::days_from_civil(now.year, now.month, now.day)
}

pub fn clock_digits(now: Now) -> (String, String) {
    (format!("{:02}", now.hour), format!("{:02}", now.minute))
}

/// Changes exactly when the card has to be drawn again.
pub fn content_key(kind: Kind, now: Now, data: &Data) -> String {
    let (settings, todos) = (data.settings, data.todos);
    match kind {
        Kind::Clock => format!("{}-{}-{} {}:{}", now.year, now.month, now.day, now.hour, now.minute), // The large card has the date.
        Kind::Calendar => format!("{}-{}-{}", now.year, now.month, now.day),
        Kind::Countdown => {
            let (title, left) = (&settings.countdown_title, days_left(now, settings.countdown_date));
            format!("{title}|{left}|{}|{}", settings.count_up, data.date_refused)
        }
        Kind::Progress => format!("{:?}", progress(now).map(|(_, value)| value)),
        Kind::Todo => {
            let list: String = todos.iter().map(|todo| format!("{}{}\n", u8::from(todo.done), todo.text)).collect();
            format!("{}|{}|{}|{list}", data.todo_start, data.todos_unsaved, data.todos_unreadable)
        }
        Kind::Weather => {
            let view = &data.weather; // What counts as today changes on the card.
            format!("{}|{:?}|{:?}|{:?}|{:?}|{}", view.today, view.place, view.observation, view.stale_since, view.not_found, view.failed)
        }
    }
}

/// What's being typed on a card right now. The card leaves that spot empty for the edit box.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    NewTodo,
    CountdownTitle,
    CountdownDate,
    Place,
}

// The names at the top of the cards can use the card's width, less the weather symbol on that one.
fn title_width(size: Size) -> f32 {
    size.dimensions().0 - MARGIN * 2.0
}

fn place_width(size: Size) -> f32 {
    title_width(size) - 34.0
}

/// Where the edit box goes on its card, and its font size.
pub fn edit_spot(editing: Editing, size: Size, todos: &[Todo]) -> ((f32, f32, f32, f32), f32) {
    let (width, height) = size.dimensions();
    match editing {
        Editing::NewTodo => {
            let y = TODO_TOP + adding_spot(todos, size).1 as f32 * TODO_ROW;
            let left = MARGIN + 24.0;
            ((left, y, width - left - MARGIN, TODO_ROW), 13.0)
        }
        Editing::CountdownTitle => ((MARGIN, 12.0, title_width(size), 20.0), 13.0),
        Editing::CountdownDate => ((MARGIN, height - 32.0, SMALL - MARGIN * 2.0, 18.0), 12.0),
        Editing::Place => ((MARGIN, 12.0, place_width(size), 20.0), 13.0),
    }
}

/// One frame of the flip: the digits being replaced, and how far along it is, 0 to 1.
pub struct Flip {
    pub old: (String, String),
    pub progress: f32,
}

pub fn draw(gfx: &Gfx, kind: Kind, size: Size, now: Now, data: &Data, flip: Option<&Flip>, editing: Option<Editing>) {
    let (width, height) = size.dimensions();
    gfx.card(width, height, RADIUS);
    match kind {
        Kind::Clock => draw_clock(gfx, size, now, flip),
        Kind::Calendar => draw_calendar(gfx, size, now),
        Kind::Countdown => draw_countdown(gfx, size, now, data, editing),
        Kind::Progress => draw_progress(gfx, size, now),
        Kind::Todo => draw_todo(gfx, size, data, editing == Some(Editing::NewTodo)),
        Kind::Weather => draw_weather(gfx, size, &data.weather, editing == Some(Editing::Place)),
    }
}

/// The outline a dragged card will settle into.
pub fn draw_guide(gfx: &Gfx, size: Size) {
    let (width, height) = size.dimensions();
    let palette = gfx.palette;
    gfx.fill_squircle(1.0, 1.0, width - 2.0, height - 2.0, RADIUS, palette.track);
    gfx.stroke_squircle(1.0, 1.0, width - 2.0, height - 2.0, RADIUS, 1.5, palette.tertiary);
}

fn draw_clock(gfx: &Gfx, size: Size, now: Now, flip: Option<&Flip>) {
    let palette = gfx.palette;
    let (hours, minutes) = clock_digits(now);
    let (width, height) = size.dimensions();
    let (between, digits, footer) = match size {
        Size::Small => (6.0, 50.0, 0.0),
        Size::Medium => (10.0, 68.0, 0.0),
        Size::Large => (10.0, 100.0, 46.0),
    };
    let tile_width = (width - TILE_INSET * 2.0 - between) / 2.0;
    let tile_height = height - TILE_INSET * 2.0 - footer;
    let right = TILE_INSET + tile_width + between;
    let (old_hours, old_minutes, progress) = match flip {
        Some(flip) => (flip.old.0.as_str(), flip.old.1.as_str(), flip.progress),
        None => (hours.as_str(), minutes.as_str(), 1.0),
    };
    let tile = |x: f32, old: &str, new: &str| flip_tile(gfx, (x, TILE_INSET, tile_width, tile_height), digits, old, new, progress);
    tile(TILE_INSET, old_hours, &hours);
    tile(right, old_minutes, &minutes);
    if size != Size::Small {
        let (middle, center) = (TILE_INSET + tile_width + between / 2.0, TILE_INSET + tile_height / 2.0);
        gfx.fill_circle(middle, center - 11.0, 2.0, palette.tertiary);
        gfx.fill_circle(middle, center + 11.0, 2.0, palette.tertiary);
    }
    if size == Size::Large {
        let weekday = date::weekday(now.year, now.month, now.day) as usize;
        let y = height - footer - 2.0;
        gfx.text(WEEKDAYS[weekday], 15.0, Weight::Medium, Align::Left, (MARGIN + 2.0, y, 120.0, footer), palette.primary);
        let day = format!("{} 月 {} 日", now.month, now.day);
        gfx.text(&day, 15.0, Weight::Regular, Align::Right, (MARGIN, y, width - MARGIN * 2.0 - 2.0, footer), palette.secondary);
    }
}

fn flip_tile(gfx: &Gfx, frame: (f32, f32, f32, f32), digits: f32, old: &str, new: &str, progress: f32) {
    let palette = gfx.palette;
    let (x, y, w, h) = frame;
    let radius = RADIUS - TILE_INSET; // Concentric with the card.
    let hinge = y + h / 2.0;
    let half = |text: &str, top: bool, fold: f32, moving: bool| {
        let clip = if top { (x, y, w, h / 2.0) } else { (x, hinge, w, h / 2.0) };
        gfx.folded(clip, hinge, fold, || {
            // The moving leaf is solid: it has to cover the digit behind it.
            gfx.fill_squircle(x, y, w, h, radius, if moving { palette.leaf } else { palette.tile });
            gfx.number(text, digits, Weight::SemiLight, Align::Center, (x, y - digits * 0.04, w, h), palette.primary);
        });
    };
    if old == new || progress >= 1.0 {
        half(new, true, 1.0, false);
        half(new, false, 1.0, false);
    } else {
        // Behind the moving leaf: the new top and the old bottom. It falls from the top, then lands on the bottom.
        half(new, true, 1.0, false);
        half(old, false, 1.0, false);
        if progress < 0.5 {
            half(old, true, 1.0 - progress * 2.0, true);
        } else {
            half(new, false, (progress - 0.5) * 2.0, true);
        }
    }
    gfx.line(x, hinge, x + w, hinge, 1.0, palette.hinge);
}

struct Grid {
    left: f32,
    top: f32,
    cell: f32,
    row: f32,
    font: f32,
    circle: f32,
}

fn month_grid(gfx: &Gfx, now: Now, grid: Grid) {
    let palette = gfx.palette;
    let Grid { left, top, cell, row, font, circle } = grid;
    for (column, name) in WEEK_HEADER.iter().enumerate() {
        let x = left + column as f32 * cell;
        gfx.text(name, font - 0.5, Weight::Medium, Align::Center, (x, top, cell, row), palette.secondary);
    }
    let first = date::weekday(now.year, now.month, 1);
    for day in 1..=date::days_in_month(now.year, now.month) {
        let position = first + day - 1;
        let x = left + (position % 7) as f32 * cell;
        let y = top + row + (position / 7) as f32 * row;
        let today = day == now.day;
        if today {
            gfx.fill_circle(x + cell / 2.0, y + row / 2.0, circle, palette.accent);
        }
        let weekend = position % 7 == 0 || position % 7 == 6;
        let color = if today {
            palette.on_accent
        } else if weekend {
            palette.secondary
        } else {
            palette.primary
        };
        let weight = if today { Weight::Semibold } else { Weight::Medium };
        gfx.number(&day.to_string(), font, weight, Align::Center, (x, y, cell, row), color);
    }
}

fn draw_calendar(gfx: &Gfx, size: Size, now: Now) {
    let palette = gfx.palette;
    let (width, _) = size.dimensions();
    let weekday = date::weekday(now.year, now.month, now.day) as usize;
    match size {
        Size::Small => {
            let title = format!("{} 月", now.month);
            gfx.text(&title, 13.0, Weight::Medium, Align::Left, (MARGIN - 1.0, 9.0, 100.0, 20.0), palette.accent);
            let cell = (SMALL - 16.0) / 7.0;
            month_grid(gfx, now, Grid { left: 8.0, top: 29.0, cell, row: 17.5, font: 10.5, circle: 8.0 });
        }
        Size::Medium => {
            let left_width = 120.0;
            gfx.text(WEEKDAYS[weekday], 13.0, Weight::Medium, Align::Left, (MARGIN + 2.0, 12.0, left_width, 20.0), palette.accent);
            gfx.number(&now.day.to_string(), 40.0, Weight::SemiLight, Align::Left, (MARGIN, 30.0, left_width, 52.0), palette.primary);
            let month = format!("{} 年 {} 月", now.year, now.month);
            gfx.text(&month, 12.0, Weight::Medium, Align::Left, (MARGIN + 2.0, SMALL - 32.0, left_width, 18.0), palette.secondary);
            let cell = 24.0;
            month_grid(gfx, now, Grid { left: WIDE - MARGIN - cell * 7.0, top: 12.0, cell, row: 19.0, font: 11.0, circle: 9.0 });
        }
        Size::Large => {
            gfx.text(WEEKDAYS[weekday], 13.0, Weight::Medium, Align::Left, (MARGIN + 2.0, 14.0, 160.0, 20.0), palette.accent);
            gfx.number(&now.day.to_string(), 40.0, Weight::SemiLight, Align::Left, (MARGIN, 32.0, 120.0, 52.0), palette.primary);
            let month = format!("{} 年 {} 月", now.year, now.month);
            gfx.text(&month, 13.0, Weight::Medium, Align::Right, (MARGIN, 14.0, width - MARGIN * 2.0 - 2.0, 20.0), palette.secondary);
            let cell = (WIDE - MARGIN * 2.0) / 7.0;
            month_grid(gfx, now, Grid { left: MARGIN, top: 92.0, cell, row: 34.0, font: 14.0, circle: 14.0 });
        }
    }
}

/// The number on the countdown card and the word under it, or None for 就是今天. Counting up, the first day counts
/// as one, the way 在一起第一天 does, so the start day shows 1 天.
fn countdown_value(left: i64, count_up: bool) -> Option<(i64, &'static str)> {
    match (count_up, left) {
        (true, ..=0) => Some((1 - left, "天")),
        (_, 0) => None,
        (_, 1..) => Some((left, "天後")),
        (false, _) => Some((-left, "天前")),
    }
}

const DATE_REFUSED: &str = "不是日期，沒有更改";

fn draw_countdown(gfx: &Gfx, size: Size, now: Now, data: &Data, editing: Option<Editing>) {
    let palette = gfx.palette;
    let settings = data.settings;
    let left = days_left(now, settings.countdown_date);
    let (year, month, day) = settings.countdown_date;
    let width = SMALL - MARGIN * 2.0;
    if editing != Some(Editing::CountdownTitle) {
        gfx.text(&settings.countdown_title, 13.0, Weight::Medium, Align::Left, (MARGIN, 12.0, title_width(size), 20.0), palette.accent);
    }
    match countdown_value(left, settings.count_up) {
        None => gfx.text("就是今天", 20.0, Weight::Regular, Align::Left, (MARGIN, 44.0, width, 44.0), palette.primary),
        Some((days, label)) => {
            gfx.number(&days.to_string(), 40.0, Weight::SemiLight, Align::Left, (MARGIN - 1.0, 34.0, width, 52.0), palette.primary);
            gfx.text(label, 12.0, Weight::Medium, Align::Left, (MARGIN, 86.0, width, 18.0), palette.secondary);
        }
    }
    if editing != Some(Editing::CountdownDate) && size == Size::Small {
        let spot = (MARGIN, SMALL - 32.0, width, 18.0);
        if data.date_refused {
            gfx.text(DATE_REFUSED, 12.0, Weight::Medium, Align::Left, spot, palette.secondary);
        } else {
            let since = if settings.count_up { "從 " } else { "" };
            gfx.number(&format!("{since}{year}/{month}/{day}"), 12.0, Weight::Medium, Align::Left, spot, palette.secondary);
        }
    }
    if size == Size::Medium {
        let right = WIDE / 2.0 + 14.0;
        let room = WIDE - right - MARGIN;
        let weekday = date::weekday(year, month, day) as usize;
        let since = if settings.count_up { "從 " } else { "" };
        gfx.text(&format!("{since}{year} 年"), 12.0, Weight::Medium, Align::Left, (right, 34.0, room, 18.0), palette.secondary);
        gfx.text(&format!("{month} 月 {day} 日"), 22.0, Weight::Regular, Align::Left, (right, 54.0, room, 34.0), palette.primary);
        let note = if data.date_refused { DATE_REFUSED } else { WEEKDAYS[weekday] };
        gfx.text(note, 12.0, Weight::Medium, Align::Left, (right, 90.0, room, 18.0), palette.secondary);
    }
}

fn draw_progress(gfx: &Gfx, size: Size, now: Now) {
    let palette = gfx.palette;
    let values = progress(now);
    if size == Size::Small {
        let width = SMALL - MARGIN * 2.0;
        for (index, (name, value)) in values.iter().enumerate() {
            let y = 11.0 + index as f32 * 35.0;
            gfx.text(name, 12.0, Weight::Medium, Align::Left, (MARGIN, y, 60.0, 18.0), palette.secondary);
            gfx.number(&format!("{value}%"), 12.0, Weight::Semibold, Align::Right, (MARGIN, y, width, 18.0), palette.primary);
            bar(gfx, MARGIN, y + 21.0, width, *value);
        }
        return;
    }
    let between = 22.0;
    let width = (WIDE - MARGIN * 2.0 - between) / 2.0;
    for (index, (name, value)) in values.iter().enumerate() {
        let x = MARGIN + (index % 2) as f32 * (width + between);
        let y = 12.0 + (index / 2) as f32 * 69.0;
        gfx.text(name, 12.0, Weight::Medium, Align::Left, (x, y, 60.0, 18.0), palette.secondary);
        gfx.number(&format!("{value}%"), 26.0, Weight::SemiLight, Align::Left, (x - 1.0, y + 17.0, width, 34.0), palette.primary);
        bar(gfx, x, y + 54.0, width, *value);
    }
}

fn bar(gfx: &Gfx, x: f32, y: f32, width: f32, value: u32) {
    gfx.fill_round(x, y, width, 4.0, 2.0, gfx.palette.track);
    let done = width * value as f32 / 100.0;
    if done >= 4.0 {
        gfx.fill_round(x, y, done, 4.0, 2.0, gfx.palette.bar);
    }
}

// Rounded first: -0.4 would otherwise come out as -0.
fn degrees(value: f32) -> String {
    format!("{:.0}°", value.round() + 0.0)
}

fn draw_weather(gfx: &Gfx, size: Size, view: &WeatherView, editing: bool) {
    let palette = gfx.palette;
    let (width, _) = size.dimensions();
    let room = width - MARGIN * 2.0;
    if !editing {
        let missing = view.not_found.map(|name| format!("找不到「{name}」"));
        let place = missing.as_deref().or(view.place).unwrap_or("天氣");
        gfx.text(place, 13.0, Weight::Medium, Align::Left, (MARGIN, 12.0, place_width(size), 20.0), palette.primary);
    }
    let Some(observation) = view.observation else {
        let note = if view.failed { "無法取得天氣" } else { "讀取中" };
        gfx.text(note, 12.0, Weight::Medium, Align::Left, (MARGIN, 44.0, room, 20.0), palette.secondary);
        return;
    };
    let (condition, sky) = weather::describe(observation.code);
    let temperature = degrees(observation.temperature);
    // The forecast may be from before midnight there.
    let today = view.today;
    let forecast = observation.days.iter().find(|day| day.date == today);
    let (high, low) = forecast.map_or((observation.high, observation.low), |day| (day.high, day.low));
    let high_low = match &view.stale_since {
        Some(when) => format!("上次更新 {when}"),
        None => format!("高 {}  低 {}", degrees(high), degrees(low)),
    };
    gfx.number(&temperature, 40.0, Weight::SemiLight, Align::Left, (MARGIN - 1.0, 30.0, room, 52.0), palette.primary);
    if size == Size::Small {
        draw_sky(gfx, sky, observation.day, SMALL - MARGIN - 13.0, 26.0, 0.9);
        gfx.text(condition, 12.0, Weight::Medium, Align::Left, (MARGIN, SMALL - 50.0, room, 18.0), palette.primary);
        gfx.text(&high_low, 12.0, Weight::Medium, Align::Left, (MARGIN, SMALL - 32.0, room, 18.0), palette.primary);
        return;
    }
    draw_sky(gfx, sky, observation.day, width - MARGIN - 11.0, 24.0, 0.68);
    gfx.text(condition, 12.0, Weight::Medium, Align::Right, (MARGIN, 40.0, room, 18.0), palette.primary);
    gfx.text(&high_low, 12.0, Weight::Medium, Align::Right, (MARGIN, 58.0, room, 18.0), palette.primary);
    draw_hours(gfx, observation, 86.0, width);
    if size == Size::Large {
        gfx.line(MARGIN, 158.0, width - MARGIN, 158.0, 1.0, palette.track);
        draw_days(gfx, observation, today, 164.0, width);
    }
}

fn draw_hours(gfx: &Gfx, observation: &Observation, y: f32, width: f32) {
    let palette = gfx.palette;
    let count = observation.hours.len().min(6);
    if count == 0 {
        return;
    }
    let column = (width - MARGIN * 2.0) / count as f32;
    for (index, hour) in observation.hours.iter().take(count).enumerate() {
        let x = MARGIN + index as f32 * column;
        let middle = x + column / 2.0;
        let label = format!("{} 時", hour.hour);
        gfx.text(&label, 11.0, Weight::Medium, Align::Center, (x, y, column, 16.0), palette.secondary);
        let (_, sky) = weather::describe(hour.code);
        draw_sky(gfx, sky, (6..18).contains(&hour.hour), middle, y + 28.0, 0.56);
        gfx.number(&degrees(hour.temperature), 12.0, Weight::Semibold, Align::Center, (x, y + 42.0, column, 18.0), palette.primary);
    }
}

fn draw_days(gfx: &Gfx, observation: &Observation, today: i64, top: f32, width: f32) {
    let palette = gfx.palette;
    let days: Vec<_> = observation.days.iter().filter(|day| day.date >= today).collect();
    let Some(lowest) = days.iter().map(|day| day.low).reduce(f32::min) else { return };
    let highest = days.iter().map(|day| day.high).fold(lowest + 1.0, f32::max);
    let row = 33.0;
    let (bar_left, bar_right) = (150.0, width - 52.0);
    for (index, day) in days.iter().take(5).enumerate() {
        let y = top + index as f32 * row;
        let middle = y + row / 2.0;
        let name = if day.date == today { "今天" } else { SHORT_WEEKDAYS[day.weekday as usize % 7] };
        gfx.text(name, 12.0, Weight::Medium, Align::Left, (MARGIN, y, 50.0, row), palette.primary);
        let (_, sky) = weather::describe(day.code);
        draw_sky(gfx, sky, true, 78.0, middle, 0.56);
        gfx.number(&degrees(day.low), 12.0, Weight::Medium, Align::Right, (92.0, y, bar_left - 100.0, row), palette.secondary);
        gfx.number(&degrees(day.high), 12.0, Weight::Semibold, Align::Right, (bar_right, y, width - MARGIN - bar_right, row), palette.primary);
        let span = bar_right - bar_left;
        let scale = span / (highest - lowest);
        gfx.fill_round(bar_left, middle - 2.0, span, 4.0, 2.0, palette.track);
        let start = bar_left + (day.low - lowest) * scale;
        gfx.fill_round(start, middle - 2.0, ((day.high - day.low) * scale).max(4.0), 4.0, 2.0, palette.bar);
    }
}

// Drawn from shapes, centred on (x, y), about 30 points across at a scale of 1.
fn draw_sky(gfx: &Gfx, sky: Sky, day: bool, x: f32, y: f32, scale: f32) {
    let palette = gfx.palette;
    let s = scale;
    let light = |x: f32, y: f32, radius: f32| {
        if !day {
            gfx.fill_moon(x, y, radius, palette.sun);
            return;
        }
        gfx.fill_circle(x, y, radius * 0.62, palette.sun);
        for ray in 0..8 {
            let (sin, cos) = (ray as f32 * std::f32::consts::FRAC_PI_4).sin_cos();
            let (near, far) = (radius * 0.84, radius * 1.14);
            gfx.line(x + cos * near, y + sin * near, x + cos * far, y + sin * far, 1.5 * s.max(0.8), palette.sun);
        }
    };
    let cloud = |x: f32, y: f32| {
        gfx.fill_round(x - 11.0 * s, y, 22.0 * s, 9.0 * s, 4.5 * s, palette.cloud);
        gfx.fill_circle(x - 4.5 * s, y + 0.5 * s, 5.5 * s, palette.cloud);
        gfx.fill_circle(x + 3.0 * s, y - 1.0 * s, 7.0 * s, palette.cloud);
    };
    match sky {
        Sky::Clear => light(x, y, 12.0 * s),
        Sky::PartlyCloudy => {
            light(x + 5.5 * s, y - 6.0 * s, 8.5 * s);
            cloud(x - 1.5 * s, y + 1.0 * s);
        }
        Sky::Cloudy => cloud(x, y - 2.0 * s),
        Sky::Fog => {
            cloud(x, y - 7.0 * s);
            for line in 0..2 {
                let row = y + (6.0 + line as f32 * 5.0) * s;
                gfx.line(x - 11.0 * s, row, x + 11.0 * s, row, 1.6 * s, palette.cloud);
            }
        }
        Sky::Rain => {
            cloud(x, y - 7.0 * s);
            for drop in 0..3 {
                let left = x + (-7.0 + drop as f32 * 7.0) * s;
                gfx.line(left + 1.5 * s, y + 5.0 * s, left - 1.0 * s, y + 11.0 * s, 1.7 * s, palette.rain);
            }
        }
        Sky::Snow => {
            cloud(x, y - 7.0 * s);
            for flake in 0..3 {
                gfx.fill_circle(x + (-7.0 + flake as f32 * 7.0) * s, y + 8.0 * s, 1.7 * s, palette.rain);
            }
        }
        Sky::Storm => {
            cloud(x, y - 7.0 * s);
            gfx.line(x + 1.5 * s, y + 3.5 * s, x - 2.5 * s, y + 8.5 * s, 1.8 * s, palette.sun);
            gfx.line(x - 2.5 * s, y + 8.5 * s, x + 1.5 * s, y + 8.5 * s, 1.8 * s, palette.sun);
            gfx.line(x + 1.5 * s, y + 8.5 * s, x - 2.0 * s, y + 13.0 * s, 1.8 * s, palette.sun);
        }
    }
}

const TODO_TOP: f32 = 38.0;
const TODO_ROW: f32 = 27.0;
const ADD_WIDTH: f32 = 48.0;

fn todo_rows(size: Size) -> usize {
    let (_, height) = size.dimensions();
    ((height - TODO_TOP - 6.0) / TODO_ROW) as usize
}

const WHEEL_NOTCH: i32 = 120;

/// Adds a wheel message to what's left over from the ones before: a mouse sends a notch at a time, a touchpad a little
/// at a time. Gives the whole notches, up being positive, and the new remainder. Turning back starts over.
pub fn wheel_notches(rest: i32, delta: i32) -> (i32, i32) {
    let total = if rest.signum() * delta.signum() < 0 { delta } else { rest + delta };
    (total / WHEEL_NOTCH, total % WHEEL_NOTCH)
}

/// What a click on the to-do card lands on.
pub enum TodoHit {
    Add,
    /// A to-do, by its place in `todo_order`.
    Row(usize),
    /// The note about the rest of the list.
    More,
    Nothing,
}

/// The order to show to-dos in: open ones first, or a card full of finished ones would hide the rest for good.
pub fn todo_order(todos: &[Todo]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..todos.len()).collect();
    order.sort_by_key(|index| todos[*index].done);
    order
}

/// Where a new to-do is typed: the first to-do shown then, and the row the field is in. A new one goes after the
/// open ones, so it's typed right there and stays in that row once it's added.
pub fn adding_spot(todos: &[Todo], size: Size) -> (usize, usize) {
    let open = todos.iter().filter(|todo| !todo.done).count();
    let start = open.saturating_sub(todo_rows(size) - 1);
    (start, open - start)
}

/// The first to-do on the card, for a list that's been scrolled by `scroll`. It can't go past a full last page.
pub fn todo_start(count: usize, size: Size, scroll: usize) -> usize {
    scroll.min(count.saturating_sub(todo_rows(size)))
}

/// Where the list goes when the note at its end is clicked: on a page, keeping one row, or from the end back to the top.
pub fn next_todo_page(count: usize, size: Size, start: usize) -> usize {
    let rows = todo_rows(size);
    if start + rows >= count { 0 } else { (start + rows - 1).min(count - rows) }
}

fn more_width(size: Size) -> f32 {
    if size.dimensions().0 > SMALL { 60.0 } else { 34.0 }
}

// The note at the end of the last row, when the list is longer than the card.
fn more_label(size: Size, count: usize, start: usize) -> Option<String> {
    let wide = size.dimensions().0 > SMALL;
    let below = count.saturating_sub(start + todo_rows(size));
    match (below, start) {
        (0, 0) => None,
        (0, _) => Some(if wide { "回到開頭" } else { "↑" }.to_string()),
        _ => Some(if wide { format!("還有 {below} 項") } else { format!("+{below}") }),
    }
}

pub fn todo_hit(x: f32, y: f32, size: Size, count: usize, start: usize) -> TodoHit {
    let (width, height) = size.dimensions();
    if x < 0.0 || y < 0.0 || x >= width || y >= height {
        return TodoHit::Nothing;
    }
    if y < TODO_TOP {
        return if x > width - ADD_WIDTH { TodoHit::Add } else { TodoHit::Nothing };
    }
    let rows = todo_rows(size);
    let row = ((y - TODO_TOP) / TODO_ROW) as usize;
    if row >= count.saturating_sub(start).min(rows) {
        return TodoHit::Nothing;
    }
    if row == rows - 1 && x >= width - MARGIN - more_width(size) && more_label(size, count, start).is_some() {
        return TodoHit::More;
    }
    TodoHit::Row(start + row)
}

fn draw_todo(gfx: &Gfx, size: Size, data: &Data, adding: bool) {
    let palette = gfx.palette;
    let (todos, start) = (data.todos, data.todo_start);
    let (width, _) = size.dimensions();
    let rows = todo_rows(size);
    let open = todos.iter().filter(|todo| !todo.done).count();
    gfx.text("待辦", 13.0, Weight::Medium, Align::Left, (MARGIN + 2.0, 12.0, 40.0, 20.0), palette.accent);
    gfx.number(&open.to_string(), 13.0, Weight::Medium, Align::Left, (MARGIN + 36.0, 12.0, 40.0, 20.0), palette.secondary);

    if data.todos_unsaved {
        gfx.text("未儲存", 11.0, Weight::Medium, Align::Right, (MARGIN, 12.0, width - MARGIN * 2.0 - 28.0, 20.0), palette.secondary);
    }

    let add_x = width - MARGIN - 8.0;
    gfx.line(add_x - 6.0, 22.0, add_x + 6.0, 22.0, 1.6, palette.accent);
    gfx.line(add_x, 16.0, add_x, 28.0, 1.6, palette.accent);

    if todos.is_empty() && !adding {
        let note = if data.todos_unreadable { "讀不到清單，稍後再試" } else { "沒有待辦事項" };
        gfx.text(note, 12.5, Weight::Medium, Align::Left, (MARGIN + 2.0, TODO_TOP, width - MARGIN * 2.0, TODO_ROW), palette.secondary);
    }
    let circle = MARGIN + 8.0;
    // While one is typed, the list makes room for it where it will go, and the ones after it move down a row.
    let (more, start, new_row) = match adding {
        true => {
            let (start, row) = adding_spot(todos, size);
            let middle = TODO_TOP + row as f32 * TODO_ROW + TODO_ROW / 2.0;
            gfx.stroke_circle(circle, middle, 6.5, 1.2, palette.accent);
            (None, start, row)
        }
        false => (more_label(size, todos.len(), start), start, rows),
    };
    for (index, position) in todo_order(todos).into_iter().skip(start).enumerate() {
        let todo = &todos[position];
        let index = if index >= new_row { index + 1 } else { index };
        if index >= rows {
            break;
        }
        let y = TODO_TOP + index as f32 * TODO_ROW;
        let middle = y + TODO_ROW / 2.0;
        if todo.done {
            gfx.fill_circle(circle, middle, 7.0, palette.accent);
            gfx.line(circle - 3.2, middle, circle - 0.8, middle + 2.6, 1.4, palette.on_accent);
            gfx.line(circle - 0.8, middle + 2.6, circle + 3.6, middle - 2.6, 1.4, palette.on_accent);
        } else {
            gfx.stroke_circle(circle, middle, 6.5, 1.2, palette.secondary);
        }
        let note = more.as_deref().filter(|_| index == rows - 1);
        let text_left = MARGIN + 24.0;
        let room = width - text_left - MARGIN - if note.is_some() { more_width(size) } else { 0.0 };
        let color = if todo.done { palette.secondary } else { palette.primary };
        gfx.text(&todo.text, 13.0, Weight::Regular, Align::Left, (text_left, y, room, TODO_ROW), color);
        if let Some(note) = note {
            let room = more_width(size);
            gfx.text(note, 11.0, Weight::Medium, Align::Right, (width - MARGIN - room, y, room, TODO_ROW), palette.secondary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Now {
        Now { year, month, day, hour, minute }
    }

    #[test]
    fn sizes_line_up() {
        let (small, _) = Size::Small.dimensions();
        let (medium, _) = Size::Medium.dimensions();
        let (_, large) = Size::Large.dimensions();
        assert_eq!(medium, small * 2.0 + GAP);
        assert_eq!(large, small * 2.0 + GAP);
        let (right, _) = Kind::Countdown.slot();
        assert_eq!(right + small, medium);
    }

    #[test]
    fn every_kind_can_be_small_and_keeps_its_default() {
        for kind in KINDS {
            assert!(kind.sizes().contains(&Size::Small));
            assert!(kind.sizes().contains(&kind.default_size()));
        }
    }

    #[test]
    fn size_keys_round_trip() {
        for size in [Size::Small, Size::Medium, Size::Large] {
            assert_eq!(Size::from_key(size.key()), Some(size));
        }
        assert_eq!(Size::from_key("x"), None);
    }

    #[test]
    fn progress_ends() {
        assert_eq!(progress(at(2026, 1, 1, 0, 0)).map(|p| p.1)[3], 0);
        assert_eq!(progress(at(2026, 12, 31, 23, 59)).map(|p| p.1), [99, 71, 99, 99]); // A Thursday.
        assert_eq!(progress(at(2026, 11, 7, 23, 59)).map(|p| p.1)[1], 99); // A Saturday, the week's last day.
        assert_eq!(progress(at(2026, 6, 1, 12, 0)).map(|p| p.1)[0], 50);
    }

    #[test]
    fn days_to_go() {
        assert_eq!(days_left(at(2026, 12, 31, 8, 0), (2027, 1, 1)), 1);
        assert_eq!(days_left(at(2027, 1, 1, 8, 0), (2027, 1, 1)), 0);
        assert_eq!(days_left(at(2027, 1, 3, 8, 0), (2027, 1, 1)), -2);
    }

    #[test]
    fn todo_rows_follow_the_size() {
        assert_eq!(todo_rows(Size::Small), 4);
        assert_eq!(todo_rows(Size::Large), 10);
    }

    #[test]
    fn todo_clicks() {
        assert!(matches!(todo_hit(320.0, 20.0, Size::Medium, 3, 0), TodoHit::Add));
        assert!(matches!(todo_hit(140.0, 20.0, Size::Small, 3, 0), TodoHit::Add));
        assert!(matches!(todo_hit(100.0, 20.0, Size::Medium, 3, 0), TodoHit::Nothing));
        assert!(matches!(todo_hit(100.0, 45.0, Size::Medium, 3, 0), TodoHit::Row(0)));
        assert!(matches!(todo_hit(100.0, 70.0, Size::Medium, 3, 0), TodoHit::Row(1)));
        assert!(matches!(todo_hit(100.0, 130.0, Size::Medium, 3, 0), TodoHit::Nothing));
        assert!(matches!(todo_hit(100.0, 300.0, Size::Large, 12, 0), TodoHit::Row(9)));
    }

    #[test]
    fn a_click_outside_the_card_hits_nothing() {
        for (x, y) in [(-400.0, 70.0), (900.0, 20.0), (330.0, -300.0), (100.0, 158.0)] {
            assert!(matches!(todo_hit(x, y, Size::Medium, 3, 0), TodoHit::Nothing), "{x},{y}");
        }
    }

    #[test]
    fn the_note_at_the_end_is_not_a_row() {
        assert!(matches!(todo_hit(300.0, 140.0, Size::Medium, 9, 0), TodoHit::More));
        assert!(matches!(todo_hit(100.0, 140.0, Size::Medium, 9, 0), TodoHit::Row(3)));
        assert!(matches!(todo_hit(300.0, 140.0, Size::Medium, 4, 0), TodoHit::Row(3)));
    }

    #[test]
    fn every_to_do_can_be_reached() {
        let (count, size) = (9, Size::Medium);
        let mut seen = vec![false; count];
        let mut start = 0;
        for _ in 0..count {
            for row in 0..todo_rows(size) {
                if let TodoHit::Row(position) = todo_hit(100.0, TODO_TOP + 5.0 + row as f32 * TODO_ROW, size, count, start) {
                    seen[position] = true;
                }
            }
            start = next_todo_page(count, size, start);
        }
        assert!(seen.iter().all(|reached| *reached));
    }

    #[test]
    fn the_last_page_leads_back_to_the_top() {
        assert_eq!(next_todo_page(9, Size::Medium, 0), 3);
        assert_eq!(next_todo_page(9, Size::Medium, 3), 5);
        assert_eq!(next_todo_page(9, Size::Medium, 5), 0);
        assert_eq!(more_label(Size::Medium, 9, 5).as_deref(), Some("回到開頭"));
    }

    #[test]
    fn scrolling_stops_at_the_end_of_the_list() {
        assert_eq!(todo_start(9, Size::Medium, 99), 5);
        assert_eq!(todo_start(3, Size::Medium, 2), 0);
    }

    #[test]
    fn no_minus_zero() {
        assert_eq!(degrees(-0.4), "0°");
        assert_eq!(degrees(-0.5), "-1°");
        assert_eq!(degrees(0.0), "0°");
        assert_eq!(degrees(22.6), "23°");
    }

    #[test]
    fn keys_ignore_what_a_card_does_not_show() {
        let settings = Settings::default();
        let data = Data {
            settings: &settings,
            todos: &[],
            todo_start: 0,
            todos_unsaved: false,
            todos_unreadable: false,
            date_refused: false,
            weather: WeatherView::default(),
        };
        let morning = content_key(Kind::Calendar, at(2026, 11, 6, 8, 0), &data);
        let evening = content_key(Kind::Calendar, at(2026, 11, 6, 20, 0), &data);
        assert_eq!(morning, evening);
        assert_ne!(morning, content_key(Kind::Calendar, at(2026, 11, 7, 8, 0), &data));
    }

    #[test]
    fn counting_up_includes_the_first_day() {
        assert_eq!(countdown_value(0, true), Some((1, "天")));
        assert_eq!(countdown_value(-364, true), Some((365, "天")));
        assert_eq!(countdown_value(3, true), Some((3, "天後")));
        assert_eq!(countdown_value(0, false), None);
        assert_eq!(countdown_value(5, false), Some((5, "天後")));
        assert_eq!(countdown_value(-2, false), Some((2, "天前")));
    }

    #[test]
    fn a_new_to_do_stays_in_the_row_it_was_typed_in() {
        let todo = |done: bool, n: usize| Todo { done, text: format!("{n}") };
        for size in [Size::Small, Size::Medium, Size::Large] {
            for open in 0..14 {
                for done in 0..3 {
                    let mut todos: Vec<Todo> = (0..open).map(|n| todo(false, n)).chain((0..done).map(|n| todo(true, 100 + n))).collect();
                    let (start, row) = adding_spot(&todos, size);
                    assert!(row < todo_rows(size), "{size:?} {open} {done}");
                    todos.push(todo(false, 999));
                    let shown = todo_start(todos.len(), size, start);
                    assert_eq!(shown, start, "{size:?} {open} {done}");
                    assert_eq!(todos[todo_order(&todos)[start + row]].text, "999", "{size:?} {open} {done}");
                }
            }
        }
    }

    #[test]
    fn a_wheel_notch_moves_one_row_however_it_arrives() {
        assert_eq!(wheel_notches(0, -120), (-1, 0));
        assert_eq!(wheel_notches(0, 240), (2, 0));
        let (mut rest, mut notches) = (0, 0);
        for _ in 0..10 {
            let (whole, left) = wheel_notches(rest, -12);
            (notches, rest) = (notches + whole, left);
        }
        assert_eq!((notches, rest), (-1, 0));
    }

    #[test]
    fn turning_the_wheel_back_starts_over() {
        assert_eq!(wheel_notches(-100, 30), (0, 30));
        assert_eq!(wheel_notches(100, 30), (1, 10));
    }

    #[test]
    fn open_todos_come_first() {
        let todo = |done: bool, text: &str| Todo { done, text: text.to_string() };
        let todos = [todo(true, "a"), todo(false, "b"), todo(true, "c"), todo(false, "d")];
        assert_eq!(todo_order(&todos), [1, 3, 0, 2]);
    }
}
