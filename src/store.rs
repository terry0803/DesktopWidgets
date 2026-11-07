//! Settings and to-dos, kept as plain text next to the program.
//!
//! Losing what's in these files is the worst thing this program could do, so a file it couldn't read is never
//! replaced, and a file that changed behind its back gets a copy before it is.

use crate::cards::{Kind, Size, KINDS};
use crate::weather::Place as WeatherPlace;
use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::Duration;
use windows::Win32::Globalization::{IsDBCSLeadByteEx, MultiByteToWideChar, CP_ACP, MB_ERR_INVALID_CHARS};

const SETTINGS_FILE: &str = "widgets.ini";
const TODO_FILE: &str = "todo.txt";
const SETTINGS_SET_ASIDE: &str = "widgets.unsaved.ini";
const TODO_SET_ASIDE: &str = "todo.unsaved.txt";
const ATTEMPTS: usize = 3;
const ATTEMPT_PAUSE: Duration = Duration::from_millis(60);
const BACKUPS: usize = 9;

#[derive(Clone, Copy)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    /// False until the card has a saved position; the default layout applies then.
    pub placed: bool,
    pub visible: bool,
    pub size: Size,
}

#[derive(Clone)]
pub struct Settings {
    pub places: [Place; KINDS.len()],
    pub locked: bool,
    pub countdown_title: String,
    pub countdown_date: (i32, u32, u32),
    /// Counting the days since the date, rather than the days until it.
    pub count_up: bool,
    pub weather_place: Option<WeatherPlace>,
    /// Size of everything, in percent. Apple's sizes come from the iPhone and feel large on a laptop.
    pub zoom: u32,
    /// False for settings from before there was a zoom, whose positions are still at 100%.
    pub zoom_saved: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            places: KINDS.map(|kind| Place { x: 0, y: 0, placed: false, visible: true, size: kind.default_size() }),
            locked: false,
            countdown_title: "新年".to_string(),
            countdown_date: (2027, 1, 1),
            count_up: false,
            weather_place: None,
            zoom: 85,
            zoom_saved: false,
        }
    }
}

#[derive(Clone)]
pub struct Todo {
    pub done: bool,
    pub text: String,
}

/// What's known about one of the two files.
struct Tracked {
    name: &'static str,
    /// What it held when last read or written, to tell whether someone else changed it. None if it wasn't there.
    /// The content rather than the time: these files are tiny, and the disk here only keeps times to two seconds.
    known: RefCell<Option<Vec<u8>>>,
    /// It's there but couldn't be read, so nothing may replace it until it can.
    guarded: Cell<bool>,
    /// It needed a copy kept and that failed, so the copy still has to come before anything replaces it.
    uncopied: Cell<bool>,
}

thread_local! {
    static SETTINGS: Tracked = const {
        Tracked { name: SETTINGS_FILE, known: RefCell::new(None), guarded: Cell::new(false), uncopied: Cell::new(false) }
    };
    static TODOS: Tracked = const {
        Tracked { name: TODO_FILE, known: RefCell::new(None), guarded: Cell::new(false), uncopied: Cell::new(false) }
    };
}

#[cfg(test)]
thread_local! {
    static TEST_FOLDER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn folder() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_FOLDER.with(|folder| folder.borrow().clone()) {
        return path;
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

// Keeps what's in a file before it's replaced, without keeping the same content twice. Once every slot is taken the
// oldest copy goes.
fn back_up(name: &str) -> bool {
    let path = folder().join(name);
    let Ok(content) = fs::read(&path) else { return false };
    let mut free = None;
    let mut oldest = None;
    for number in 1..=BACKUPS {
        let suffix = if number == 1 { "bak".to_string() } else { format!("bak{number}") };
        let candidate = folder().join(format!("{name}.{suffix}"));
        match fs::read(&candidate) {
            Ok(kept) if kept == content => return true,
            Ok(_) => {
                let written = fs::metadata(&candidate).and_then(|metadata| metadata.modified()).ok();
                if oldest.as_ref().is_none_or(|(known, _)| written < *known) {
                    oldest = Some((written, candidate));
                }
            }
            Err(_) => {
                free.get_or_insert(candidate);
            }
        }
    }
    let target = free.or(oldest.map(|(_, candidate)| candidate));
    target.is_some_and(|target| fs::write(target, content).is_ok())
}

fn write_file(name: &str, text: &str) -> std::io::Result<()> {
    let path = folder().join(name);
    let temporary = path.with_extension("tmp");
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all() // Otherwise a power cut could leave the new name pointing at nothing.
    })();
    let mut result = written;
    if result.is_ok() {
        // A virus scanner or an editor can hold the old file for a moment.
        for attempt in 0..ATTEMPTS {
            result = fs::rename(&temporary, &path);
            if result.is_ok() {
                break;
            }
            if attempt + 1 < ATTEMPTS {
                std::thread::sleep(ATTEMPT_PAUSE);
            }
        }
    }
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

// Through a temporary file, so a crash mid-write can't leave half a file behind. False if the text didn't get there.
fn save(tracked: &Tracked, text: &str) -> bool {
    if tracked.guarded.get() {
        return false;
    }
    let on_disk = match fs::read(folder().join(tracked.name)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(_) => return false, // Something has hold of it, and it can't be told what would be replaced.
    };
    let changed = on_disk != *tracked.known.borrow() || tracked.uncopied.get();
    // Already there, word for word: nothing to write, and the disk can stay asleep.
    if !changed && on_disk.as_deref() == Some(text.as_bytes()) {
        return true;
    }
    if on_disk.is_some() && changed && !back_up(tracked.name) {
        return false;
    }
    let saved = write_file(tracked.name, text).is_ok();
    if saved {
        *tracked.known.borrow_mut() = Some(text.as_bytes().to_vec());
        tracked.uncopied.set(false);
    }
    saved
}

// For when the program is going and the real file can't be written: not lost, and nothing replaced either.
fn set_aside(name: &str, text: &str) {
    let target = folder().join(name);
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let beside = || (2..).map(|number| folder().join(format!("{stem}.{number}.{extension}"))).find(|path| !path.exists());
    // One from an earlier time that can't be copied first isn't replaced, and one that can't be written to is left
    // as it is: either way this one goes beside it.
    if (target.exists() && !back_up(name)) || fs::write(&target, text).is_err() {
        if let Some(other) = beside() {
            let _ = fs::write(other, text);
        }
    }
}

/// Reads text the way a person may have saved it: UTF-8 with or without a BOM, UTF-16, or the system's old code page.
/// The flag says the guess needed care, so the original is worth keeping.
fn decode(bytes: &[u8]) -> (String, bool) {
    let (text, keep_original) = guess(bytes);
    // A NUL left in a to-do would be written back, and read as UTF-16 the next time.
    if text.contains('\0') { (text.replace('\0', ""), true) } else { (text, keep_original) }
}

fn guess(bytes: &[u8]) -> (String, bool) {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        if rest.contains(&0) {
            return (with_nuls(rest), true);
        }
        return (String::from_utf8_lossy(rest).into_owned(), std::str::from_utf8(rest).is_err());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return (wide(rest, true), true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return (wide(rest, false), true);
    }
    if bytes.contains(&0) {
        return (with_nuls(bytes), true);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), false),
        Err(_) => (mixed(bytes), true),
    }
}

// Line by line, as some may be UTF-8 and others not: in Windows PowerShell, Add-Content adds lines in the old code
// page to the end of a UTF-8 file. A line that's valid both ways goes by which reads less strangely, since a line in
// the old code page can just happen to be valid UTF-8 too.
fn mixed(bytes: &[u8]) -> String {
    bytes.split_inclusive(|byte| *byte == b'\n').map(line_text).collect()
}

fn line_text(line: &[u8]) -> String {
    match std::str::from_utf8(line) {
        Ok(text) => match from_code_page(line) {
            Some(other) if strangeness(&other) < strangeness(text) => other,
            _ => text.to_string(),
        },
        Err(_) => from_code_page(line).unwrap_or_else(|| String::from_utf8_lossy(line).into_owned()),
    }
}

fn strangeness(text: &str) -> usize {
    text.chars().filter(|character| unlikely(*character)).count()
}

fn wide(bytes: &[u8], little: bool) -> String {
    let unit = if little { u16::from_le_bytes } else { u16::from_be_bytes };
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units)
}

// Whether UTF-16 without a BOM is little-endian, if these bytes can be that: whole units, line ends among them.
fn wide_order(bytes: &[u8]) -> Option<bool> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let has_unit = |wanted: [u8; 2]| bytes.chunks_exact(2).any(|pair| pair == wanted);
    if has_unit([b'\n', 0]) {
        return Some(true);
    }
    if has_unit([0, b'\n']) {
        return Some(false);
    }
    // No line end to go by. A stray NUL isn't UTF-16, but plenty of them, all on one side, are.
    let nuls_at = |parity: usize| bytes.iter().skip(parity).step_by(2).filter(|byte| **byte == 0).count();
    let (little, big) = (nuls_at(1), nuls_at(0));
    if little.min(big) > 0 || little.max(big) * 4 < bytes.len() / 2 {
        return None;
    }
    Some(little >= big)
}

// Characters a to-do list here is made of. Text read with the wrong encoding comes out as others: General Punctuation
// from "[ ]", Hangul and private-use characters from UTF-8, accented letters from the old code page read as UTF-8.
fn unlikely(character: char) -> bool {
    !matches!(u32::from(character),
        0x00 | 0x09 | 0x0a | 0x0d | 0x20..=0x7e | 0x2010..=0x2027 | 0x2030..=0x203a | 0x2190..=0x21ff
        | 0x2460..=0x24ff | 0x25a0..=0x27bf | 0x3000..=0x312f | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f | 0xff00..=0xffef | 0x1f300..=0x1faff)
}

// Text never holds a NUL, so something else went on. Nothing but NULs at the end is what a crash can leave. UTF-16
// without a BOM has them after or before every Latin letter. In Windows PowerShell `>>` adds UTF-16 to the end of a
// UTF-8 file, starting just before the first NUL or some characters earlier, with or without a line end before it.
// The file before it may also be in the old code page. So every reading that's possible is tried, and the one with
// the fewest strange characters wins. When that's even: the whole file as UTF-16, or a part that starts on a new line,
// before a part that starts mid-line, and a later cut before an earlier one, which keeps every complete line intact.
fn with_nuls(bytes: &[u8]) -> String {
    let length = bytes.iter().rposition(|byte| *byte != 0).map_or(0, |last| last + 1);
    let Some(first) = bytes[..length].iter().position(|byte| *byte == 0) else {
        return guess(&bytes[..length]).0;
    };
    let mut best: Option<((usize, u8), Candidate)> = None;
    let mut consider = |key: (usize, u8), reading: Candidate| {
        if best.as_ref().is_none_or(|(known, _)| key < *known) {
            best = Some((key, reading));
        }
    };
    // Padding after UTF-16 can be an odd number of NULs, the last of them part of the last character.
    let whole = &bytes[..(length + length % 2).min(bytes.len())];
    if let Some(little) = wide_order(whole) {
        let text = wide(whole, little);
        consider((strangeness(&text), 0), Candidate::Text(text));
    }
    // Every cut has the same parity, so every tail is the end of one run of UTF-16 units, worked out once.
    let parity = (first + 1) % 2;
    let tails = (first >= 2 && (bytes.len() - parity) % 2 == 0).then(|| Tails::new(&bytes[parity..]));
    if let Some(tails) = &tails {
        let utf8 = |line: &[u8]| std::str::from_utf8(line).is_ok().then(|| line_text(line));
        let old = |line: &[u8]| if line.is_empty() { Some(String::new()) } else { from_code_page(line) };
        // The old code page reads almost anything, so it's only for a file whose start isn't UTF-8 wherever it's cut.
        if !cuts(bytes, first, parity, tails, &utf8, false, &mut consider) {
            cuts(bytes, first, parity, tails, &old, true, &mut consider);
        }
    }
    let stripped = guess(&bytes.iter().copied().filter(|byte| *byte != 0).collect::<Vec<u8>>()).0;
    consider((strangeness(&stripped), 2), Candidate::Text(stripped));
    match best {
        Some((_, Candidate::Text(text))) => text,
        Some((_, Candidate::Cut { at, old_page })) => {
            let head: String = bytes[..at]
                .split_inclusive(|byte| *byte == b'\n')
                .map(|line| if old_page { from_code_page(line).unwrap_or_default() } else { line_text(line) })
                .collect();
            let units = tails.as_ref().map_or(&[][..], |tails| &tails.units[(at - parity) / 2..]);
            head + &String::from_utf16_lossy(units)
        }
        None => String::new(),
    }
}

enum Candidate {
    Text(String),
    /// UTF-16 from this byte on, and the part before it in UTF-8 or the old code page.
    Cut { at: usize, old_page: bool },
}

// What the UTF-16 from each unit on to the end reads as: strange characters, broken surrogate pairs, whether a line
// end is still to come, and how many units have a zero in either byte, for when there's no line end to go by.
struct Tails {
    units: Vec<u16>,
    strange: Vec<usize>,
    broken: Vec<usize>,
    line_end: Vec<bool>,
    second_half: Vec<bool>,
    high_zero: Vec<usize>,
    low_zero: Vec<usize>,
}

impl Tails {
    fn new(bytes: &[u8]) -> Self {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        let count = units.len();
        let (mut strange, mut broken) = (vec![0; count + 1], vec![0; count + 1]);
        let (mut line_end, mut second_half) = (vec![false; count + 1], vec![false; count + 1]);
        let (mut high_zero, mut low_zero) = (vec![0; count + 1], vec![0; count + 1]);
        let mut index = 0;
        while index < count {
            let unit = units[index];
            let low = units.get(index + 1).filter(|next| (0xdc00..0xe000).contains(*next));
            match (unit, low) {
                (0xd800..=0xdbff, Some(&low)) => {
                    let code = 0x10000 + ((u32::from(unit) - 0xd800) << 10) + (u32::from(low) - 0xdc00);
                    strange[index] = usize::from(char::from_u32(code).is_none_or(unlikely));
                    second_half[index + 1] = true;
                    index += 2;
                }
                (0xd800..=0xdfff, _) => {
                    (strange[index], broken[index]) = (1, 1);
                    index += 1;
                }
                _ => {
                    strange[index] = usize::from(char::from_u32(u32::from(unit)).is_none_or(unlikely));
                    index += 1;
                }
            }
        }
        for index in (0..count).rev() {
            strange[index] += strange[index + 1];
            broken[index] += broken[index + 1];
            line_end[index] = units[index] == u16::from(b'\n') || line_end[index + 1];
            high_zero[index] = high_zero[index + 1] + usize::from(units[index] >> 8 == 0);
            low_zero[index] = low_zero[index + 1] + usize::from(units[index] & 0xff == 0);
        }
        Self { units, strange, broken, line_end, second_half, high_zero, low_zero }
    }

    /// How strange the UTF-16 from this unit on is, if it can be read as that at all: with a line end in it, or
    /// without one, with plenty of Latin letters, the way `wide_order` judges a whole file.
    fn from(&self, index: usize) -> Option<usize> {
        let count = self.units.len().saturating_sub(index);
        let latin = self.low_zero.get(index) == Some(&0) && self.high_zero.get(index).is_some_and(|zeros| zeros * 4 >= count);
        let whole = index < self.units.len() && !self.second_half[index] && self.broken[index] == 0 && (self.line_end[index] || latin);
        whole.then_some(self.strange[index])
    }
}

// Where a line can be cut and still be read, and how strangely it reads up to there: as UTF-8 at every character
// boundary up to the first byte that isn't, and the same in the old code page. Made once per line, so a long line
// costs no more than reading it, however many cuts fall in it.
struct Prefixes {
    utf8: Vec<(usize, usize)>,
    old: Vec<(usize, usize)>,
}

impl Prefixes {
    fn new(line: &[u8]) -> Self {
        let valid = match std::str::from_utf8(line) {
            Ok(text) => text,
            Err(error) => std::str::from_utf8(&line[..error.valid_up_to()]).unwrap_or_default(),
        };
        let mut utf8 = vec![(0, 0)];
        for (at, character) in valid.char_indices() {
            let so_far = utf8[utf8.len() - 1].1 + usize::from(unlikely(character));
            utf8.push((at + character.len_utf8(), so_far));
        }
        let mut old = vec![(0, 0)];
        let mut at = 0;
        while at < line.len() {
            let width = if unsafe { IsDBCSLeadByteEx(CP_ACP, line[at]) }.is_ok() { 2 } else { 1 };
            let Some(text) = line.get(at..at + width).and_then(from_code_page) else { break };
            at += width;
            old.push((at, old[old.len() - 1].1 + strangeness(&text)));
        }
        Self { utf8, old }
    }

    fn find(list: &[(usize, usize)], length: usize) -> Option<usize> {
        list.binary_search_by_key(&length, |(end, _)| *end).ok().map(|found| list[found].1)
    }

    /// As `line_text` reads the first `length` bytes: UTF-8, unless the old code page reads less strangely.
    fn utf8(&self, length: usize) -> Option<usize> {
        let utf8 = Self::find(&self.utf8, length)?;
        Some(Self::find(&self.old, length).filter(|old| *old < utf8).unwrap_or(utf8))
    }

    fn old(&self, length: usize) -> Option<usize> {
        Self::find(&self.old, length)
    }
}

// Tries every cut, from the last possible one back. Each line before it is read once, not once for every cut, so this
// stays about as fast as reading the file. False if no cut could be read with `line_text`.
fn cuts(
    bytes: &[u8],
    first: usize,
    parity: usize,
    tails: &Tails,
    line_text: &dyn Fn(&[u8]) -> Option<String>,
    old_page: bool,
    consider: &mut dyn FnMut((usize, u8), Candidate),
) -> bool {
    let starts: Vec<usize> =
        std::iter::once(0).chain(bytes[..first].iter().enumerate().filter(|(_, byte)| **byte == b'\n').map(|(at, _)| at + 1)).collect();
    // For the lines before each start: whether all of them can be read, and how strange they are.
    let (mut readable, mut strange) = (vec![true], vec![0]);
    for pair in starts.windows(2) {
        let text = line_text(&bytes[pair[0]..pair[1]]);
        readable.push(readable[readable.len() - 1] && text.is_some());
        strange.push(strange[strange.len() - 1] + text.map_or(0, |text| strangeness(&text)));
    }
    let mut found = false;
    let mut prefixes: Option<(usize, Prefixes)> = None;
    for at in (1..first).rev().step_by(2) {
        let line = starts.partition_point(|start| *start <= at) - 1;
        if !readable[line] {
            continue;
        }
        let Some(tail) = tails.from((at - parity) / 2) else { continue };
        let start = starts[line];
        if prefixes.as_ref().is_none_or(|(known, _)| *known != line) {
            let end = starts.get(line + 1).copied().unwrap_or(first);
            prefixes = Some((line, Prefixes::new(&bytes[start..end])));
        }
        let Some((_, known)) = &prefixes else { continue };
        let Some(partial) = (if old_page { known.old(at - start) } else { known.utf8(at - start) }) else { continue };
        let rank = if at == start { 0 } else { 1 };
        consider((strange[line] + partial + tail, rank), Candidate::Cut { at, old_page });
        found = true;
    }
    found
}

fn from_code_page(bytes: &[u8]) -> Option<String> {
    unsafe {
        let length = MultiByteToWideChar(CP_ACP, MB_ERR_INVALID_CHARS, bytes, None);
        if length <= 0 {
            return None;
        }
        let mut wide = vec![0u16; length as usize];
        let written = MultiByteToWideChar(CP_ACP, MB_ERR_INVALID_CHARS, bytes, Some(&mut wide));
        (written > 0).then(|| String::from_utf16_lossy(&wide[..written as usize]))
    }
}

enum Reading {
    Missing,
    Text(String),
    Refused,
}

fn read(tracked: &Tracked) -> Reading {
    let path = folder().join(tracked.name);
    let mut outcome = fs::read(&path);
    for _ in 1..ATTEMPTS {
        match &outcome {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                std::thread::sleep(ATTEMPT_PAUSE);
                outcome = fs::read(&path);
            }
            _ => break,
        }
    }
    match outcome {
        Ok(bytes) => {
            let (text, keep_original) = match tracked.known.borrow().as_deref() {
                // Added to the end of what was read or written last, as `>>` and Add-Content do: where the addition
                // starts is known, so only the addition needs its encoding guessed.
                Some(known)
                    if !known.is_empty()
                        && bytes.len() > known.len()
                        && bytes.starts_with(known)
                        && (bytes[known.len()..].contains(&0) || std::str::from_utf8(&bytes).is_err()) =>
                {
                    let added = &bytes[known.len()..];
                    // Added to a UTF-16 file, it's UTF-16 too, though it has no BOM of its own.
                    let added = match known {
                        [0xff, 0xfe, ..] => wide(added, true),
                        [0xfe, 0xff, ..] => wide(added, false),
                        _ => decode(added).0,
                    };
                    (decode(known).0 + &added, true)
                }
                _ => decode(&bytes),
            };
            tracked.uncopied.set(keep_original && !back_up(tracked.name));
            tracked.guarded.set(false);
            *tracked.known.borrow_mut() = Some(bytes);
            Reading::Text(text)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            tracked.guarded.set(false);
            *tracked.known.borrow_mut() = None;
            Reading::Missing
        }
        Err(_) => {
            tracked.guarded.set(true);
            Reading::Refused
        }
    }
}

fn key_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Clock => "clock",
        Kind::Calendar => "calendar",
        Kind::Countdown => "countdown",
        Kind::Progress => "progress",
        Kind::Todo => "todo",
        Kind::Weather => "weather",
    }
}

pub fn load_settings() -> Settings {
    let mut settings = Settings::default();
    match SETTINGS.with(read) {
        Reading::Text(text) => {
            // Lines it can't make sense of won't survive the next save, and old positions are about to be converted.
            // Without that copy the file mustn't be replaced, which the next save then tries again.
            if !parse_settings(&text, &mut settings) || !settings.zoom_saved {
                SETTINGS.with(|tracked| {
                    if !back_up(tracked.name) {
                        tracked.uncopied.set(true);
                    }
                });
            }
        }
        Reading::Missing => {}
        Reading::Refused => settings.zoom_saved = true, // Nothing to bring up to date, and nothing may be written.
    }
    settings
}

/// True while the settings can't be read, so their place on the internet isn't known either.
pub fn settings_unknown() -> bool {
    SETTINGS.with(|tracked| tracked.guarded.get())
}

fn gone(tracked: &Tracked) -> bool {
    let missing = fs::metadata(folder().join(tracked.name)).is_err_and(|error| error.kind() == ErrorKind::NotFound);
    missing && !tracked.guarded.get()
}

/// True if the file has been deleted, which leaves what's in memory as the only copy.
pub fn settings_gone() -> bool {
    SETTINGS.with(gone)
}

/// The settings, once a file that couldn't be read can be after all.
pub fn settings_back() -> Option<Settings> {
    settings_unknown().then(load_settings).filter(|_| !settings_unknown())
}

// False if any of it made no sense, which the next save would drop.
fn parse_settings(text: &str, settings: &mut Settings) -> bool {
    let mut understood = true;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            understood = false;
            continue;
        };
        understood &= parse_setting(key.trim(), value.trim(), settings);
    }
    // The menu lives on the cards, so with none showing there'd be no way back.
    if !settings.places.iter().any(|place| place.visible) {
        settings.places[0].visible = true;
    }
    understood
}

fn parse_setting(key: &str, value: &str, settings: &mut Settings) -> bool {
    match key {
        "locked" => {
            let Some(locked) = flag(value) else { return false };
            settings.locked = locked;
        }
        "zoom" => {
            settings.zoom_saved = true; // Even a bad value says the positions are already at some zoom.
            let Ok(zoom) = value.parse::<u32>() else { return false };
            settings.zoom = zoom.clamp(50, 150);
        }
        "countdown.title" => settings.countdown_title = value.to_string(),
        "weather.place" => {
            let mut parts = value.split('|');
            let (Some(name), Some(latitude), Some(longitude), Some(manual)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                return false;
            };
            let (Ok(latitude), Ok(longitude)) = (latitude.parse(), longitude.parse()) else { return false };
            let name = name.to_string();
            settings.weather_place = Some(WeatherPlace { name, latitude, longitude, manual: manual == "1" });
            return flag(manual).is_some() && parts.next().is_none();
        }
        "countdown.mode" => match value {
            "up" => settings.count_up = true,
            "down" => settings.count_up = false,
            _ => return false,
        },
        "countdown.date" => {
            let Some(date) = crate::date::parse(value) else { return false };
            settings.countdown_date = date;
        }
        _ => {
            let Some(name) = key.strip_prefix("card.") else { return false };
            let Some(index) = KINDS.iter().position(|kind| key_of(*kind) == name) else { return false };
            let mut parts = value.split(',').map(str::trim);
            let (Some(x), Some(y), Some(visible)) = (parts.next(), parts.next(), parts.next()) else { return false };
            let place = &mut settings.places[index];
            let mut understood = true;
            // A note after the value shouldn't hide the card, so anything but a plain 0 leaves it showing.
            match flag(visible) {
                Some(visible) => place.visible = visible,
                None => understood = false,
            }
            if let Some(key) = parts.next() {
                match Size::from_key(key).filter(|size| KINDS[index].sizes().contains(size)) {
                    Some(size) => place.size = size,
                    None => understood = false,
                }
            }
            match (x.parse(), y.parse()) {
                (Ok(x), Ok(y)) => (place.x, place.y, place.placed) = (x, y, true),
                _ => understood &= x == "-" && y == "-", // A card on the default layout.
            }
            return understood && parts.next().is_none();
        }
    }
    true
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn settings_text(settings: &Settings) -> String {
    let (year, month, day) = settings.countdown_date;
    let mut text = format!(
        "locked={}\nzoom={}\ncountdown.title={}\ncountdown.date={year:04}-{month:02}-{day:02}\ncountdown.mode={}\n",
        u8::from(settings.locked),
        settings.zoom,
        settings.countdown_title,
        if settings.count_up { "up" } else { "down" },
    );
    if let Some(place) = &settings.weather_place {
        let name = place.name.replace(['|', '\r', '\n'], " ");
        text.push_str(&format!("weather.place={name}|{}|{}|{}\n", place.latitude, place.longitude, u8::from(place.manual)));
    }
    for (kind, place) in KINDS.iter().zip(&settings.places) {
        // A card still on the default layout has no position worth keeping.
        let position = if place.placed { format!("{},{}", place.x, place.y) } else { "-,-".to_string() };
        text.push_str(&format!("card.{}={position},{},{}\n", key_of(*kind), u8::from(place.visible), place.size.key()));
    }
    text
}

/// False if it didn't reach the disk.
pub fn save_settings(settings: &Settings) -> bool {
    SETTINGS.with(|tracked| save(tracked, &settings_text(settings)))
}

pub fn set_aside_settings(settings: &Settings) {
    set_aside(SETTINGS_SET_ASIDE, &settings_text(settings));
}

fn parse_todos(text: &str) -> Vec<Todo> {
    text.lines()
        .filter_map(|line| {
            let (done, rest) = match line.get(..4) {
                Some("[x] ") => (true, &line[4..]),
                Some("[ ] ") => (false, &line[4..]),
                _ => (false, line),
            };
            let text = rest.trim();
            (!text.is_empty()).then(|| Todo { done, text: text.to_string() })
        })
        .collect()
}

pub fn load_todos() -> Vec<Todo> {
    match TODOS.with(read) {
        Reading::Text(text) => parse_todos(&text),
        Reading::Missing | Reading::Refused => Vec::new(),
    }
}

/// True if the file has been deleted, which leaves what's in memory as the only copy.
pub fn todos_gone() -> bool {
    TODOS.with(gone)
}

/// True while the list couldn't be read yet, so the card doesn't show what's in the file.
pub fn todos_unreadable() -> bool {
    TODOS.with(|tracked| tracked.guarded.get())
}

/// The list as it is on disk if someone changed it since it was last read or written, for instance in Notepad, or if
/// it could only be read now. A file that's gone or out of reach gives nothing: a disk can drop out and come back, and
/// the list in memory is still good, so the next save simply puts it back.
pub fn todos_changed_outside() -> Option<Vec<Todo>> {
    TODOS.with(|tracked| {
        let guarded = tracked.guarded.get();
        if !guarded && fs::read(folder().join(tracked.name)).ok() == *tracked.known.borrow() {
            return None;
        }
        match read(tracked) {
            Reading::Text(text) => Some(parse_todos(&text)),
            Reading::Missing => None,
            Reading::Refused => {
                tracked.guarded.set(guarded);
                None
            }
        }
    })
}

fn todos_text(todos: &[Todo]) -> String {
    todos.iter().map(|todo| format!("[{}] {}\n", if todo.done { 'x' } else { ' ' }, todo.text)).collect()
}

/// False if it didn't reach the disk.
pub fn save_todos(todos: &[Todo]) -> bool {
    TODOS.with(|tracked| save(tracked, &todos_text(todos)))
}

pub fn set_aside_todos(todos: &[Todo]) {
    set_aside(TODO_SET_ASIDE, &todos_text(todos));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_utf8_needs_no_care() {
        assert_eq!(decode("新年".as_bytes()), ("新年".to_string(), false));
    }

    #[test]
    fn a_bom_is_not_part_of_the_text() {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice("[x] done".as_bytes());
        let todos = parse_todos(&decode(&bytes).0);
        assert_eq!((todos[0].done, todos[0].text.as_str()), (true, "done"));
    }

    #[test]
    fn utf16_is_read_both_ways_round() {
        let little: Vec<u8> = [0xff, 0xfe].into_iter().chain("locked=1\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let big: Vec<u8> = [0xfe, 0xff].into_iter().chain("locked=1\n".encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(decode(&little).0, "locked=1\n");
        assert_eq!(decode(&big).0, "locked=1\n");
        assert!(decode(&little).1);
    }

    #[test]
    fn utf16_without_a_bom_is_recognised() {
        let little: Vec<u8> = "[ ] milk\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let big: Vec<u8> = "[ ] milk\n".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode(&little), ("[ ] milk\n".to_string(), true));
        assert_eq!(decode(&big), ("[ ] milk\n".to_string(), true));
    }

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn utf16_without_a_bom_and_without_latin_letters_is_recognised() {
        // 上 is 0A 4E, which holds a line end's byte without being one.
        assert_eq!(decode(&utf16("上午開會\r\n")).0, "上午開會\r\n");
        assert_eq!(decode(&utf16("上午 ab")).0, "上午 ab");
        assert_eq!(decode(&utf16("上午 ab\r\n買牛奶\r\n")).0, "上午 ab\r\n買牛奶\r\n");
    }

    #[test]
    fn utf16_added_to_the_end_of_utf8_keeps_both() {
        // What `>>` does in Windows PowerShell, with the first part both even and odd in length.
        for first in ["[ ] 買牛奶\n[x] 繳電話費\n", "[ ] a\n", "[ ] ab\n"] {
            for added in ["buy eggs\r\n", "買雞蛋\r\n", "a\r\nb\r\n"] {
                let mut bytes = first.as_bytes().to_vec();
                bytes.extend(utf16(added));
                assert_eq!(decode(&bytes), (format!("{first}{added}"), true), "{first:?} {added:?}");
            }
        }
    }

    #[test]
    fn added_lines_that_start_with_a_character_holding_a_line_end_byte() {
        // 上 and 《 are 0A 4E and 0A 30 in UTF-16, which looks like a line end before the real start.
        for added in ["上課\r\n", "《原子習慣》\r\n", "訊息 ok\r\n", "上\r\n下\r\n"] {
            let mut bytes = "[ ] 買牛奶\n".as_bytes().to_vec();
            bytes.extend(utf16(added));
            assert_eq!(decode(&bytes).0, format!("[ ] 買牛奶\n{added}"), "{added:?}");
        }
    }

    #[test]
    fn added_lines_after_a_last_line_without_its_line_end() {
        // What Notepad leaves when the last line wasn't ended, followed by `>>`.
        for first in ["[ ] 買牛奶\n[x] ab\n[ ] 繳費x", "[ ] 繳費", "[ ] 買牛奶\n繳費"] {
            let mut bytes = first.as_bytes().to_vec();
            bytes.extend(utf16("call mom\r\n"));
            assert_eq!(decode(&bytes).0, format!("{first}call mom\r\n"), "{first:?}");
        }
    }

    #[test]
    fn many_lists_with_lines_added_by_powershell() {
        // Lists as the program writes them, with one to three lines added the way `>>` does, from a mix of characters
        // that includes the troublesome ones.
        let pool: Vec<char> = "上課《》訊尊權舊刊伊吊買牛奶繳電話費原子習慣中午abc xyz123-".chars().collect();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |below: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below as u64) as usize
        };
        let mut failures = Vec::new();
        for round in 0..3000 {
            let item = |next: &mut dyn FnMut(usize) -> usize| (0..1 + next(8)).map(|_| pool[next(pool.len())]).collect::<String>().trim().to_string();
            let todos: Vec<Todo> =
                (0..1 + next(5)).map(|_| Todo { done: next(3) == 0, text: item(&mut next) }).filter(|todo| !todo.text.is_empty()).collect();
            let mut expected = todos_text(&todos);
            let mut bytes = expected.as_bytes().to_vec();
            for _ in 0..1 + next(3) {
                let line = format!("{}\r\n", if next(2) == 0 { format!("[ ] {}", item(&mut next)) } else { item(&mut next) });
                bytes.extend(utf16(&line));
                expected.push_str(&line);
            }
            if decode(&bytes).0 != expected {
                failures.push((round, expected));
            }
        }
        assert!(failures.is_empty(), "{} of 3000, first {:?}", failures.len(), failures.first());
    }

    #[test]
    fn nuls_after_a_bom_are_dealt_with_too() {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend("[ ] 買牛奶\r\n".as_bytes());
        bytes.extend(utf16("買雞蛋\r\n"));
        assert_eq!(decode(&bytes), ("[ ] 買牛奶\r\n買雞蛋\r\n".to_string(), true));
        let mut padded = vec![0xff, 0xfe];
        padded.extend(utf16("[ ] a\r\n"));
        padded.extend([0; 4]);
        assert_eq!(decode(&padded).0, "[ ] a\r\n");
    }

    #[test]
    fn lines_added_in_the_old_code_page_keep_the_utf8_ones() {
        // Add-Content in Windows PowerShell writes Big5 here. Only where the system's code page is Big5.
        let big5 = [0xb6, 0x52, 0xc2, 0xfb, 0xb3, 0x4a, 0x0d, 0x0a];
        if from_code_page(&big5).as_deref() != Some("買雞蛋\r\n") {
            return;
        }
        let mut bytes = "[ ] 買牛奶\n".as_bytes().to_vec();
        bytes.extend(big5);
        assert_eq!(decode(&bytes), ("[ ] 買牛奶\n買雞蛋\r\n".to_string(), true));
        assert_eq!(decode(&big5).0, "買雞蛋\r\n");
    }

    #[test]
    fn nuls_at_the_end_are_only_padding() {
        for padding in [1, 2, 3, 16] {
            let mut bytes = "[ ] 買牛奶\n[x] 繳電話費\n".as_bytes().to_vec();
            bytes.extend(vec![0; padding]);
            assert_eq!(decode(&bytes), ("[ ] 買牛奶\n[x] 繳電話費\n".to_string(), true), "{padding}");
        }
        assert_eq!(decode(b"[ ] eggs\0").0, "[ ] eggs");
    }

    #[test]
    fn a_stray_nul_costs_nothing_but_itself() {
        assert_eq!(decode(b"[ ] one\n[ ] t\0wo\n[ ] three\n").0, "[ ] one\n[ ] two\n[ ] three\n");
        assert_eq!(decode(b"[ ] one\0\n[ ] two\n").0, "[ ] one\n[ ] two\n");
    }

    #[test]
    fn settings_padded_with_nuls_are_still_the_settings() {
        let mut bytes = b"locked=1\nzoom=100\ncountdown.title=x\n".to_vec();
        bytes.extend([0; 8]);
        let mut settings = Settings::default();
        assert!(parse_settings(&decode(&bytes).0, &mut settings));
        assert_eq!((settings.locked, settings.zoom, settings.zoom_saved), (true, 100, true));
    }

    #[test]
    fn other_encodings_keep_every_line_and_ask_for_a_copy() {
        // Not UTF-8, and not valid in most code pages either: the ASCII parts have to come through whatever happens.
        let (text, keep) = decode(b"[x] one\n[ ] \xff\xfe\xfd\n[ ] three\n");
        assert!(keep);
        assert_eq!(parse_todos(&text).len(), 3);
    }

    #[test]
    fn broken_text_keeps_the_positions_and_the_manual_place() {
        let (text, _) = decode(b"locked=1\nzoom=75\ncountdown.title=\xff\xfe\nweather.place=x|25.5|121.5|1\ncard.clock=100,200,1,m\n");
        let mut settings = Settings::default();
        parse_settings(&text, &mut settings);
        assert!(settings.locked);
        assert_eq!(settings.zoom, 75);
        assert!(settings.weather_place.is_some_and(|place| place.manual && place.latitude == 25.5));
        assert_eq!((settings.places[0].x, settings.places[0].y, settings.places[0].placed), (100, 200, true));
    }

    #[test]
    fn a_bad_zoom_does_not_scale_the_positions_again() {
        for value in ["85.0", "85%", "", "-1"] {
            let mut settings = Settings::default();
            parse_settings(&format!("zoom={value}\n"), &mut settings);
            assert!(settings.zoom_saved, "{value}");
            assert_eq!(settings.zoom, 85);
        }
    }

    #[test]
    fn settings_from_before_the_zoom_are_recognised() {
        let mut settings = Settings::default();
        parse_settings("locked=0\n", &mut settings);
        assert!(!settings.zoom_saved);
    }

    #[test]
    fn hiding_every_card_leaves_one() {
        let lines: String = KINDS.iter().map(|kind| format!("card.{}=-,-,0,s\n", key_of(*kind))).collect();
        let mut settings = Settings::default();
        parse_settings(&lines, &mut settings);
        assert_eq!(settings.places.iter().filter(|place| place.visible).count(), 1);
    }

    #[test]
    fn what_the_program_writes_itself_counts_as_understood() {
        let mut settings = Settings::default();
        settings.places[1] = Place { x: 10, y: 20, placed: true, visible: true, size: Size::Medium };
        settings.weather_place = Some(WeatherPlace { name: "新北".to_string(), latitude: 25.0, longitude: 121.5, manual: false });
        assert!(parse_settings(&settings_text(&settings), &mut Settings::default()));
    }

    #[test]
    fn lines_that_make_no_sense_are_noticed() {
        for text in [
            "# a note\n",
            "zoom=85%\n",
            "colour=red\n",
            "weather.place=x|y\n",
            "weather.place=x|1|2|maybe\n",
            "countdown.date=soon\n",
            "locked=yes\n",
            "locked=1 # do not move\n",
            "card.clock=a,b,1\n",
            "card.clock=100,200,1 # note\n",
            "card.clock=100,200,2,m\n",
            "card.clock=100,200,1,xl\n",
            "card.clock=100,200,1,m,extra\n",
        ] {
            assert!(!parse_settings(text, &mut Settings::default()), "{text}");
        }
    }

    #[test]
    fn a_note_after_a_value_does_not_hide_the_card() {
        let mut settings = Settings::default();
        parse_settings("card.clock=100,200,1 # note\n", &mut settings);
        assert!(settings.places[0].visible);
        assert_eq!((settings.places[0].x, settings.places[0].placed), (100, true));
    }

    // A real folder for the file handling. Each test runs on a thread of its own, and all the state is per thread.
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("desktop-widgets-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TEST_FOLDER.with(|folder| *folder.borrow_mut() = Some(path.clone()));
        path
    }

    fn todo(text: &str) -> Todo {
        Todo { done: false, text: text.to_string() }
    }

    #[test]
    fn an_edit_of_the_same_length_is_noticed() {
        let path = scratch("same-length");
        assert!(save_todos(&[todo("a")]));
        fs::write(path.join(TODO_FILE), "[x] a\n").unwrap();
        assert!(todos_changed_outside().is_some_and(|todos| todos[0].done));
        assert!(todos_changed_outside().is_none());
    }

    #[test]
    fn a_file_that_went_away_leaves_the_list_alone() {
        let path = scratch("gone");
        assert!(save_todos(&[todo("a"), todo("b")]));
        fs::remove_file(path.join(TODO_FILE)).unwrap();
        assert!(todos_changed_outside().is_none());
        assert!(save_todos(&[todo("a"), todo("b"), todo("c")]));
        assert_eq!(fs::read_to_string(path.join(TODO_FILE)).unwrap(), "[ ] a\n[ ] b\n[ ] c\n");
    }

    #[test]
    fn what_powershell_adds_after_a_list_already_read_is_told_apart_exactly() {
        // The cases a guess alone gets wrong: a last line without its line end, followed by a line starting with 上,
        // and a list in the old code page.
        let path = scratch("appended");
        let mut first = "[ ] 買牛奶\n[ ] 繳費".as_bytes().to_vec();
        fs::write(path.join(TODO_FILE), &first).unwrap();
        assert_eq!(load_todos().len(), 2);
        first.extend(utf16("上課\r\n"));
        fs::write(path.join(TODO_FILE), &first).unwrap();
        let todos = todos_changed_outside().unwrap();
        assert_eq!(todos.iter().map(|todo| todo.text.as_str()).collect::<Vec<_>>(), ["買牛奶", "繳費上課"]);
        let big5 = [0x5b, 0x20, 0x5d, 0x20, 0xb6, 0x52, 0x0a];
        if from_code_page(&big5).as_deref() == Some("[ ] 買\n") {
            fs::write(path.join(TODO_FILE), big5).unwrap();
            todos_changed_outside().unwrap();
            let mut more = big5.to_vec();
            more.extend(utf16("上課\r\n"));
            fs::write(path.join(TODO_FILE), &more).unwrap();
            let todos = todos_changed_outside().unwrap();
            assert_eq!(todos.iter().map(|todo| todo.text.as_str()).collect::<Vec<_>>(), ["買", "上課"]);
        }
    }

    #[test]
    fn settings_that_need_a_copy_are_not_replaced_without_one() {
        let path = scratch("no-copy");
        fs::write(path.join(SETTINGS_FILE), "# mine\nzoom=85\n").unwrap();
        let slots: Vec<PathBuf> = (1..=BACKUPS).map(|n| path.join(if n == 1 { "widgets.ini.bak".to_string() } else { format!("widgets.ini.bak{n}") })).collect();
        for slot in &slots {
            fs::write(slot, "other").unwrap();
            let mut permissions = fs::metadata(slot).unwrap().permissions();
            permissions.set_readonly(true);
            fs::set_permissions(slot, permissions).unwrap();
        }
        let settings = load_settings();
        assert!(!save_settings(&settings));
        assert_eq!(fs::read_to_string(path.join(SETTINGS_FILE)).unwrap(), "# mine\nzoom=85\n");
        for slot in &slots {
            let mut permissions = fs::metadata(slot).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(slot, permissions).unwrap();
        }
        assert!(save_settings(&settings));
    }

    #[test]
    fn an_earlier_set_aside_that_cannot_be_copied_stays() {
        let path = scratch("aside-twice");
        fs::write(path.join(TODO_SET_ASIDE), "[ ] last time\n").unwrap();
        for n in 1..=BACKUPS {
            let slot = path.join(if n == 1 { format!("{TODO_SET_ASIDE}.bak") } else { format!("{TODO_SET_ASIDE}.bak{n}") });
            fs::write(&slot, format!("other {n}")).unwrap();
            let mut permissions = fs::metadata(&slot).unwrap().permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&slot, permissions).unwrap();
        }
        set_aside_todos(&[todo("this time")]);
        assert_eq!(fs::read_to_string(path.join(TODO_SET_ASIDE)).unwrap(), "[ ] last time\n");
        assert_eq!(fs::read_to_string(path.join("todo.unsaved.2.txt")).unwrap(), "[ ] this time\n");
    }

    #[test]
    fn latin_letters_added_by_powershell_after_a_list_already_read_need_no_guess() {
        let path = scratch("ascii-appended");
        let mut bytes = "[ ] 買牛奶\n[ ] 繳費".as_bytes().to_vec();
        fs::write(path.join(TODO_FILE), &bytes).unwrap();
        load_todos();
        bytes.extend(utf16("buy eggs\r\n"));
        fs::write(path.join(TODO_FILE), &bytes).unwrap();
        let todos = todos_changed_outside().unwrap();
        assert_eq!(todos.iter().map(|todo| todo.text.as_str()).collect::<Vec<_>>(), ["買牛奶", "繳費buy eggs"]);
    }

    #[test]
    fn a_large_file_with_utf16_added_reads_quickly() {
        let mut bytes: Vec<u8> = (0..2000).flat_map(|n| format!("[ ] 第 {n} 件事\n").into_bytes()).collect();
        bytes.extend(utf16("buy eggs\r\n"));
        let started = std::time::Instant::now();
        let text = decode(&bytes).0;
        assert!(text.ends_with("[ ] 第 1999 件事\nbuy eggs\r\n"));
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn a_long_line_of_chinese_added_by_powershell_is_read_whole() {
        // Far more characters before the added part's first NUL than any limit on the cuts tried would allow.
        for count in [10, 128, 129, 300, 2000] {
            let added = format!("{}\r\n", "買".repeat(count));
            let mut bytes = "[ ] a\n[ ] b\n".as_bytes().to_vec();
            bytes.extend(utf16(&added));
            assert_eq!(decode(&bytes).0, format!("[ ] a\n[ ] b\n{added}"), "{count}");
        }
    }

    #[test]
    fn a_large_file_with_a_long_chinese_addition_reads_quickly() {
        let mut bytes: Vec<u8> = (0..2000).flat_map(|n| format!("[ ] 第 {n} 件事\n").into_bytes()).collect();
        bytes.extend(utf16(&format!("{}\r\n", "買".repeat(3000))));
        let started = std::time::Instant::now();
        assert!(decode(&bytes).0.ends_with("買買\r\n"));
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn chinese_added_to_a_utf16_file_already_read_stays_utf16() {
        for little in [true, false] {
            let path = scratch(if little { "utf16-le-added" } else { "utf16-be-added" });
            let encode = |text: &str| -> Vec<u8> {
                text.encode_utf16().flat_map(|unit| if little { unit.to_le_bytes() } else { unit.to_be_bytes() }).collect()
            };
            let mut bytes = if little { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] };
            bytes.extend(encode("[ ] a\r\n"));
            fs::write(path.join(TODO_FILE), &bytes).unwrap();
            load_todos();
            bytes.extend(encode("買牛奶"));
            fs::write(path.join(TODO_FILE), &bytes).unwrap();
            let todos = todos_changed_outside().unwrap();
            assert_eq!(todos.iter().map(|todo| todo.text.as_str()).collect::<Vec<_>>(), ["a", "買牛奶"], "{little}");
        }
    }

    #[test]
    fn a_set_aside_file_that_cannot_be_written_is_left_and_this_goes_beside_it() {
        let path = scratch("aside-read-only");
        let target = path.join(TODO_SET_ASIDE);
        fs::write(&target, "[ ] last time\n").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions).unwrap();
        set_aside_todos(&[todo("this time")]);
        assert_eq!(fs::read_to_string(&target).unwrap(), "[ ] last time\n");
        assert_eq!(fs::read_to_string(path.join("todo.unsaved.2.txt")).unwrap(), "[ ] this time\n");
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&target, permissions).unwrap();
    }

    #[test]
    fn latin_letters_added_without_a_line_end() {
        // Out-File -NoNewline and the like: no line end in what was added.
        let mut bytes = "[ ] a\n[ ] 繳費\n".as_bytes().to_vec();
        bytes.extend(utf16("buy eggs"));
        assert_eq!(decode(&bytes).0, "[ ] a\n[ ] 繳費\nbuy eggs");
    }

    #[test]
    fn a_long_single_line_with_utf16_added_reads_quickly() {
        let mut bytes = format!("[ ] {}", "買".repeat(2700)).into_bytes();
        bytes.extend(utf16("buy eggs\r\n"));
        let started = std::time::Instant::now();
        let text = decode(&bytes).0;
        assert!(text.ends_with("買買buy eggs\r\n"), "{}", &text[text.len() - 30..]);
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[test]
    fn saving_what_is_already_there_writes_nothing() {
        let path = scratch("unchanged");
        assert!(save_todos(&[todo("a")]));
        let file = path.join(TODO_FILE);
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(save_todos(&[todo("a")]));
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
        assert!(save_todos(&[todo("b")]));
        assert_eq!(fs::read_to_string(&file).unwrap(), "[ ] b\n");
    }

    #[test]
    fn a_deleted_file_is_noticed() {
        let path = scratch("deleted");
        assert!(save_todos(&[todo("a")]));
        assert!(!todos_gone());
        fs::remove_file(path.join(TODO_FILE)).unwrap();
        assert!(todos_gone());
        TODOS.with(|tracked| tracked.guarded.set(true));
        assert!(!todos_gone());
    }

    #[test]
    fn a_change_from_outside_gets_a_copy_before_it_is_replaced() {
        let path = scratch("copy");
        assert!(save_todos(&[todo("a")]));
        fs::write(path.join(TODO_FILE), "[ ] from notepad\n").unwrap();
        assert!(save_todos(&[todo("b")]));
        assert_eq!(fs::read_to_string(path.join("todo.txt.bak")).unwrap(), "[ ] from notepad\n");
    }

    #[test]
    fn a_list_unreadable_at_the_start_comes_back() {
        let path = scratch("guarded");
        fs::write(path.join(TODO_FILE), "[ ] kept\n").unwrap();
        TODOS.with(|tracked| tracked.guarded.set(true)); // As a read that was refused leaves it.
        assert!(!save_todos(&[todo("typed meanwhile")]));
        assert_eq!(fs::read_to_string(path.join(TODO_FILE)).unwrap(), "[ ] kept\n");
        assert!(todos_changed_outside().is_some_and(|todos| todos[0].text == "kept"));
        assert!(!todos_unreadable());
        assert!(save_todos(&[todo("kept"), todo("typed meanwhile")]));
    }

    #[test]
    fn settings_unreadable_at_the_start_come_back() {
        let path = scratch("settings");
        fs::write(path.join(SETTINGS_FILE), "zoom=75\nlocked=1\n").unwrap();
        SETTINGS.with(|tracked| tracked.guarded.set(true));
        assert!(!save_settings(&Settings::default()));
        let settings = settings_back().unwrap();
        assert_eq!((settings.zoom, settings.locked), (75, true));
        assert!(!settings_unknown() && settings_back().is_none());
    }

    #[test]
    fn with_every_slot_taken_the_oldest_copy_goes() {
        let path = scratch("rotate");
        let slot = |number: usize| path.join(if number == 1 { "x.bak".to_string() } else { format!("x.bak{number}") });
        for number in 1..=BACKUPS {
            fs::write(slot(number), format!("old {number}")).unwrap();
            // Oldest in the middle, so that neither end being picked by accident can pass.
            let age = if number == 5 { 500 } else { number as u64 };
            let file = fs::File::options().write(true).open(slot(number)).unwrap();
            file.set_modified(std::time::SystemTime::now() - Duration::from_secs(age * 10)).unwrap();
        }
        fs::write(path.join("x"), "newest").unwrap();
        assert!(back_up("x"));
        assert_eq!(fs::read_to_string(slot(5)).unwrap(), "newest");
        assert_eq!(fs::read_to_string(slot(4)).unwrap(), "old 4");
    }

    #[test]
    fn hand_edits_that_would_be_dropped_get_a_copy() {
        let path = scratch("hand-edit");
        fs::write(path.join(SETTINGS_FILE), "# mine\nzoom=85\n").unwrap();
        load_settings();
        assert_eq!(fs::read_to_string(path.join("widgets.ini.bak")).unwrap(), "# mine\nzoom=85\n");
    }

    #[test]
    fn what_cannot_be_saved_is_set_aside_without_touching_the_file() {
        let path = scratch("aside");
        fs::write(path.join(TODO_FILE), "[ ] kept\n").unwrap();
        set_aside_todos(&[todo("typed")]);
        assert_eq!(fs::read_to_string(path.join(TODO_SET_ASIDE)).unwrap(), "[ ] typed\n");
        assert_eq!(fs::read_to_string(path.join(TODO_FILE)).unwrap(), "[ ] kept\n");
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut settings = Settings::default();
        settings.locked = true;
        settings.zoom = 100;
        settings.countdown_title = "畢業".to_string();
        settings.countdown_date = (2028, 6, 30);
        settings.count_up = true;
        settings.weather_place = Some(WeatherPlace { name: "新北".to_string(), latitude: 25.05, longitude: 121.52, manual: true });
        settings.places[2] = Place { x: -40, y: 60, placed: true, visible: false, size: Size::Medium };
        let mut again = Settings::default();
        parse_settings(&settings_text(&settings), &mut again);
        assert_eq!(settings_text(&again), settings_text(&settings));
    }
}
