//! Low-power desktop widgets. One thread that sleeps until the next minute, and not even that while the screen is
//! off. Cards are layered windows, so Windows keeps their pictures and nothing is drawn unless a card changed.

#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod cards;
mod date;
mod gfx;
mod http;
mod icons;
mod prompt;
mod snap;
mod store;
mod wallpaper;
mod weather;

use cards::{Data, Editing, Flip, Kind, Now, Size, TodoHit, WeatherView, GAP, KINDS};
use prompt::{After, Answer};
use snap::{Rect, Rules};
use gfx::Gfx;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use store::{Settings, Todo};
use weather::Search;
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ};
use windows::Win32::UI::Shell::{
    IShellLinkW, ShellLink, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4, SLGP_RAWPATH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, POWERBROADCAST_SETTING, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime, GetSystemTimeAsFileTime, GetTickCount64};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetGuiResources, GetProcessHandleCount, ProcessPowerThrottling,
    SetProcessInformation, GR_GDIOBJECTS, GR_USEROBJECTS, PROCESS_POWER_THROTTLING_STATE,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, ReleaseCapture, SetCapture, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    DispatchMessageW, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXSMICON, WM_CONTEXTMENU,
    FindWindowW, GetClassNameW, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindow,
    GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindow, IsWindowVisible, KillTimer,
    LoadCursorW, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetCoalescableTimer, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TrackPopupMenu, TranslateMessage,
    DEVICE_NOTIFY_WINDOW_HANDLE, EVENT_SYSTEM_FOREGROUND, GWLP_HWNDPARENT, GWLP_USERDATA, GWL_EXSTYLE, GW_HWNDPREV, HMENU, HWND_BOTTOM, IDC_ARROW, MA_NOACTIVATE, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, SC_CLOSE, SM_CXDRAG, SM_CXSCREEN, SM_CYDRAG, SM_CYSCREEN, SPI_GETWORKAREA, SPI_SETDESKWALLPAPER, SPI_SETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_APP, WM_CAPTURECHANGED, WM_ENDSESSION, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_SYSCOMMAND, WM_TIMECHANGE, WM_TIMER,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{w, GUID, HSTRING, PCWSTR};

const TICK: usize = 1;
const ZOOMS: [u32; 3] = [75, 85, 100];
static ZOOM: AtomicU32 = AtomicU32::new(85);
const WM_WEATHER: u32 = WM_APP + 1;
const WM_RELAYOUT: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 3;
const WM_DPI_LATER: u32 = WM_APP + 4;
const TRAY_ID: u32 = 1;
const NIN_SELECT: u32 = 0x0400;
const NIN_KEYSELECT: u32 = 0x0401;
const APP_NAME: &str = "桌面小工具";
const ICON_TRAY_DARK: u16 = 2;
const ICON_TRAY_LIGHT: u16 = 3;
const MK_LBUTTON: usize = 1;
const MOST_WINDOWS: usize = 5000;
const WEATHER_MINUTES: i64 = 30;
const LOCATE_MINUTES: i64 = 6 * 60;
const STALE_MINUTES: i64 = 3 * 60;
const DAY_MINUTES: i64 = 24 * 60;
const FLIP: usize = 2;
const GLIDE: usize = 3;
const FLIP_MS: f32 = 280.0;
const FRAME_MS: u32 = 33;
const TICK_SLACK_MS: u32 = 300; // Lets Windows wake us together with whatever else is due.
const GLIDE_MS: f32 = 160.0;
const GLIDE_FRAME_MS: u32 = 16;
const SNAP_REACH: f32 = 34.0;
const EDGE_MARGIN: f32 = 22.0;

const DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
const PBT_APMRESUMESUSPEND: usize = 0x7;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
// Where Windows Settings keeps the switch for each entry under Run: absent or even means on, odd means off.
const APPROVED_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const RUN_VALUE: PCWSTR = w!("DesktopWidgets");

const MENU_SHOW: usize = 1; // One id per card from here.
const MENU_LOCK: usize = 20;
const MENU_STARTUP: usize = 21;
const MENU_COUNTDOWN: usize = 22;
const MENU_ADD: usize = 23;
const MENU_DELETE: usize = 24;
const MENU_RESET: usize = 25;
const MENU_EXIT: usize = 26;
const MENU_PLACE: usize = 27;
const MENU_SIZE: usize = 40; // One id per size from here.
const MENU_ZOOM: usize = 50; // One id per zoom from here.
const MENU_AUTO_PLACE: usize = 28;
const MENU_COUNT_UP: usize = 29;

struct Slot {
    kind: Kind,
    window: HWND,
    key: String,
}

struct Drag {
    index: usize,
    cursor: POINT,
    origin: POINT,
    /// The pointer went too far for this to be a click, even if a lock keeps the card where it is.
    travelled: bool,
    moved: bool,
    target: POINT,
}

#[derive(Default)]
struct Weather {
    observation: Option<weather::Observation>,
    updated: i64,
    /// The same moment by a clock nobody sets, for how old the weather is: setting the clock back mustn't make it new.
    updated_tick: u64,
    next: i64,
    located: i64,
    failures: u32,
    in_flight: bool,
    search: Option<String>,
    /// A typed name that got no answer, to ask for again with the next update.
    retry: Option<String>,
    /// A typed name there's no such place for.
    not_found: Option<String>,
    /// Goes up whenever the place is changed by hand, which makes anything still on its way out of date.
    revision: u32,
}

struct Glide {
    index: usize,
    from: POINT,
    to: POINT,
    started: Instant,
}

struct Flipping {
    index: usize,
    old: (String, String),
    started: Instant,
}

struct App {
    gfx: Gfx,
    hidden: HWND,
    slots: Vec<Slot>,
    settings: Settings,
    todos: Vec<Todo>,
    display_on: bool,
    session_locked: bool,
    flipping: Option<Flipping>,
    drag: Option<Drag>,
    shown_clock: Option<(String, String)>,
    taskbar_created: u32,
    desktop_in_front: bool,
    guide: HWND,
    glide: Option<Glide>,
    editing: Option<Editing>,
    dark: bool,
    weather: Weather,
    icons: Vec<Rect>,
    /// How far the to-do list was scrolled.
    scroll: usize,
    /// When a to-do was last ticked, and which row it was in.
    last_toggle: Option<(Instant, usize)>,
    /// When the list was last paged through with the note at its end.
    last_page: Option<Instant>,
    /// Part of a wheel notch, from a touchpad.
    wheel_rest: i32,
    unsaved_settings: bool,
    unsaved_todos: bool,
    /// When a countdown date that isn't a date was left behind, for as long as the card says so.
    date_refused: Option<i64>,
    relayout_pending: bool,
    /// The countdown's name before an edit that's still under way, which is what gets saved until it's done.
    title_before: Option<String>,
    /// The date the countdown's date field opened with, to tell whether it was changed.
    date_offered: Option<String>,
    /// The same for the countdown's name field.
    title_offered: Option<String>,
    /// The screen changed while a card was held, which has to wait until it's let go.
    relayout_missed: bool,
    /// The size the wallpaper was cropped for.
    screen: (i32, i32),
    tray: HICON,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Nothing is held across a nested message loop (menus, the text box), so this normally can't be refused.
fn with<R>(action: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(action)
    })
}

fn now() -> Now {
    let time = unsafe { GetLocalTime() };
    Now {
        year: i32::from(time.wYear),
        month: u32::from(time.wMonth),
        day: u32::from(time.wDay),
        hour: u32::from(time.wHour),
        minute: u32::from(time.wMinute),
    }
}

fn wants_dark() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    result != ERROR_SUCCESS || value == 0
}

fn on_battery() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).is_ok() && status.ACLineStatus == 0 }
}

fn zoom() -> f32 {
    ZOOM.load(Ordering::Relaxed) as f32 / 100.0
}

// Everything goes through these two, so the zoom reaches the cards, the text and all the spacing alike.
fn scale_of(window: HWND) -> f32 {
    unsafe { GetDpiForWindow(window) as f32 / 96.0 * zoom() }
}

/// The main screen's right now. GetDpiForSystem alone would wait for the next sign-in to notice a change.
pub fn primary_dpi() -> u32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (0, 0);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_ok() { x } else { GetDpiForSystem() }
    }
}

fn system_scale() -> f32 {
    primary_dpi() as f32 / 96.0 * zoom()
}

fn rules_at(scale: f32) -> Rules {
    Rules {
        gap: (GAP * scale).round() as i32,
        margin: (EDGE_MARGIN * scale).round() as i32,
        reach: (SNAP_REACH * scale).round() as i32,
    }
}

// How far the pointer has to go before a press is a drag. Windows gives the same few pixels at any scale, however it's
// asked, so at 200% a hand that slips a little would otherwise drag.
fn drag_threshold(window: HWND) -> (i32, i32) {
    unsafe {
        let dpi = GetDpiForWindow(window) as i32;
        let scaled = |metric| GetSystemMetricsForDpi(metric, 96) * dpi / 96;
        (scaled(SM_CXDRAG), scaled(SM_CYDRAG))
    }
}

fn double_click_time() -> Duration {
    Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }))
}

fn index_of(kind: Kind) -> usize {
    KINDS.iter().position(|candidate| *candidate == kind).unwrap_or(0)
}

fn rect_from(bounds: RECT) -> Rect {
    Rect { x: bounds.left, y: bounds.top, w: bounds.right - bounds.left, h: bounds.bottom - bounds.top }
}

fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// The usable part of the screen a card is on, or nearest to.
fn area_of(card: Rect) -> Rect {
    let bounds = RECT { left: card.x, top: card.y, right: card.x + card.w, bottom: card.y + card.h };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let found = unsafe { GetMonitorInfoW(MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool();
    rect_from(if found { info.rcWork } else { work_area() })
}

// Keeps a layout together when everything changes size: positions shrink towards the top right of their screen.
fn rescale_places(settings: &mut Settings, ratio: f32) {
    for place in settings.places.iter_mut().filter(|place| place.placed) {
        let area = area_of(Rect { x: place.x, y: place.y, w: 1, h: 1 });
        let right = area.x + area.w;
        place.x = right - ((right - place.x) as f32 * ratio).round() as i32;
        place.y = area.y + ((place.y - area.y) as f32 * ratio).round() as i32;
    }
}

// Settings from before there was a zoom were laid out at 100%. True if they changed.
fn bring_up_to_date(settings: &mut Settings) -> bool {
    if settings.zoom_saved {
        return false;
    }
    let ratio = settings.zoom as f32 / 100.0;
    rescale_places(settings, ratio);
    settings.zoom_saved = true;
    true
}

fn pixels(size: Size, scale: f32) -> (i32, i32) {
    let (width, height) = size.dimensions();
    ((width * scale).ceil() as i32, (height * scale).ceil() as i32)
}

fn work_area() -> RECT {
    let mut area = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut area).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    area
}

/// The default column along the right of the main screen, clear of a Dock at the top.
fn default_position(kind: Kind) -> POINT {
    let scale = system_scale();
    let area = work_area();
    let left = area.right as f32 - (cards::column_width() + EDGE_MARGIN) * scale;
    let top = area.top as f32 + 44.0 * scale;
    let (x, y) = kind.slot();
    POINT { x: (left + x * scale).round() as i32, y: (top + y * scale).round() as i32 }
}

fn exe_command() -> Option<String> {
    std::env::current_exe().ok().map(|path| format!("\"{}\"", path.display()))
}

fn run_entry() -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size))
    };
    (result == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
}

fn switched_off() -> bool {
    let mut value = [0u8; 12];
    let mut size = value.len() as u32;
    let result = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, APPROVED_KEY, RUN_VALUE, RRF_RT_REG_BINARY, None, Some(value.as_mut_ptr().cast()), Some(&mut size))
    };
    result == ERROR_SUCCESS && size > 0 && value[0] & 1 == 1
}

fn write_value(path: PCWSTR, kind: windows::Win32::System::Registry::REG_VALUE_TYPE, bytes: &[u8]) {
    unsafe {
        let mut key = HKEY::default();
        let opened = RegCreateKeyExW(HKEY_CURRENT_USER, path, None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None);
        if opened == ERROR_SUCCESS {
            let _ = RegSetValueExW(key, RUN_VALUE, None, kind, Some(bytes));
            let _ = RegCloseKey(key);
        }
    }
}

fn write_run_entry(command: &str) {
    let wide: Vec<u16> = command.encode_utf16().chain([0]).collect();
    let bytes: Vec<u8> = wide.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    write_value(RUN_KEY, REG_SZ, &bytes);
}

// The same record Windows Settings writes: 2 for on; 3 and the time for off.
fn write_switch(on: bool) {
    let mut value = [0u8; 12];
    value[0] = if on { 2 } else { 3 };
    if !on {
        let time = unsafe { GetSystemTimeAsFileTime() };
        value[4..8].copy_from_slice(&time.dwLowDateTime.to_le_bytes());
        value[8..12].copy_from_slice(&time.dwHighDateTime.to_le_bytes());
    }
    write_value(APPROVED_KEY, REG_BINARY, &value);
}

fn starts_with_windows() -> bool {
    run_entry().is_some() && !switched_off()
}

fn set_starts_with_windows(enabled: bool) {
    if let Some(command) = exe_command() {
        write_run_entry(&command);
    }
    write_switch(enabled);
}

// The entry Windows Settings lists under Startup apps, where it can be switched on and off. A first start adds it
// switched off, so that starting with Windows is the owner's choice. It follows the program only once the program it
// names is gone: another copy started now and then (a build in target\, an old copy on a drive that gets unplugged)
// must not take startup away from the installed one.
fn register_startup() {
    let Some(command) = exe_command() else { return };
    match run_entry() {
        None => {
            write_switch(false);
            write_run_entry(&command);
        }
        Some(known) if takes_over(&known, &command, |path| std::path::Path::new(path).is_file()) => {
            write_run_entry(&command)
        }
        Some(_) => {}
    }
}

fn takes_over(known: &str, command: &str, exists: impl Fn(&str) -> bool) -> bool {
    !known.eq_ignore_ascii_case(command) && !exists(known.trim().trim_matches('"'))
}

// So it can be found and started from the Start menu like any app. Only made when it isn't there.
fn add_to_start_menu() {
    let Some(target) = std::env::current_exe().ok() else { return };
    let Some(programs) = std::env::var_os("APPDATA").map(|folder| {
        std::path::PathBuf::from(folder).join("Microsoft\\Windows\\Start Menu\\Programs").join(format!("{APP_NAME}.lnk"))
    }) else {
        return;
    };
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return };
        // One already there is left alone while the program it opens is still there, like the startup entry; one
        // left pointing at a program that moved or was removed is made again for this one.
        if programs.exists() {
            let mut known = [0u16; 1024];
            let opens = windows::core::Interface::cast::<IPersistFile>(&link)
                .and_then(|file| file.Load(&HSTRING::from(programs.as_os_str()), STGM_READ))
                .and_then(|_| link.GetPath(&mut known, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32));
            let known = String::from_utf16_lossy(&known[..known.iter().position(|&unit| unit == 0).unwrap_or(0)]);
            if opens.is_err() || !takes_over(&known, &target.to_string_lossy(), |path| std::path::Path::new(path).is_file()) {
                return;
            }
        }
        let path = HSTRING::from(target.as_os_str());
        let _ = link.SetPath(&path);
        if let Some(folder) = target.parent() {
            let _ = link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()));
        }
        let _ = link.SetDescription(&HSTRING::from("Mac 風格的低耗電桌面小工具"));
        if let Ok(file) = windows::core::Interface::cast::<IPersistFile>(&link) {
            let _ = file.Save(&HSTRING::from(programs.as_os_str()), true);
        }
    }
}

// The taskbar's own colour, which can differ from the apps'.
fn taskbar_is_light() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    result == ERROR_SUCCESS && value != 0
}

fn tray_icon() -> HICON {
    unsafe {
        let side = GetSystemMetricsForDpi(SM_CXSMICON, primary_dpi());
        let id = if taskbar_is_light() { ICON_TRAY_LIGHT } else { ICON_TRAY_DARK };
        let module = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(Some(module.into()), PCWSTR(id as usize as *const u16), IMAGE_ICON, side, side, LR_DEFAULTCOLOR)
            .map(|handle| HICON(handle.0))
            .unwrap_or_default()
    }
}

// The icon by the clock, with the same menu as the cards. Added again when Explorer restarts.
fn show_tray(hidden: HWND, icon: HICON, how: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hidden,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip(APP_NAME.encode_utf16()) {
        *slot = unit;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    unsafe {
        let _ = Shell_NotifyIconW(how, &data);
        if how == NIM_ADD {
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }
}

fn hide_tray(hidden: HWND) {
    let data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hidden, uID: TRAY_ID, ..Default::default() };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

impl App {
    fn active(&self) -> bool {
        self.display_on && !self.session_locked
    }

    /// One shot, aligned to the next minute. Nothing at all is scheduled while nobody can see the cards.
    fn schedule(&self) {
        unsafe {
            if !self.active() {
                let _ = KillTimer(Some(self.hidden), TICK);
                return;
            }
            let time = GetLocalTime();
            let into_minute = u32::from(time.wSecond) * 1000 + u32::from(time.wMilliseconds);
            let wait = 60_000 - into_minute.min(59_999) + 20;
            SetCoalescableTimer(Some(self.hidden), TICK, wait, None, TICK_SLACK_MS);
        }
    }

    fn current_palette(&self) -> gfx::Palette {
        if self.dark { gfx::DARK } else { gfx::LIGHT }
    }

    fn load_wallpaper(&mut self) {
        self.screen = screen_size();
        let (width, height) = self.screen;
        self.gfx.set_wallpaper(wallpaper::load(width as u32, height as u32));
    }

    // While the program runs nothing here is lost: the next minute tries again, and leaving sets it aside.
    fn save_settings(&mut self) {
        self.unsaved_settings = !store::save_settings(&self.settings_for_disk());
    }

    // While the countdown's date is being typed, its new name is only on the card: Esc can still take it back, so
    // anything saved meanwhile, by the weather for one, keeps the old name.
    fn settings_for_disk(&self) -> std::borrow::Cow<'_, Settings> {
        match &self.title_before {
            Some(title) => {
                let mut settings = self.settings.clone();
                settings.countdown_title = title.clone();
                std::borrow::Cow::Owned(settings)
            }
            None => std::borrow::Cow::Borrowed(&self.settings),
        }
    }

    fn save_todos(&mut self) {
        let unsaved = !store::save_todos(&self.todos);
        if unsaved != self.unsaved_todos {
            self.unsaved_todos = unsaved;
            self.refresh(false); // The card says so.
        }
    }

    fn retry_saves(&mut self) {
        if self.unsaved_settings {
            self.save_settings();
        }
        if self.unsaved_todos {
            self.save_todos();
        }
    }

    // A file that was out of reach at the start may not be any more.
    fn retry_reads(&mut self) {
        if store::todos_unreadable() {
            if let Some(mut todos) = store::todos_changed_outside() {
                todos.append(&mut self.todos); // Whatever was added meanwhile goes after what was there.
                self.todos = todos;
            }
        }
        if let Some(settings) = store::settings_back() {
            self.settings_came_back(settings);
        }
    }

    // The real settings after all, instead of the defaults this run started with.
    fn settings_came_back(&mut self, mut settings: Settings) {
        self.unsaved_settings = bring_up_to_date(&mut settings);
        // A countdown name being typed stays on the card; the one to keep until it's done is the real one now.
        if self.title_before.is_some() {
            let typed = std::mem::replace(&mut settings.countdown_title, self.settings.countdown_title.clone());
            self.title_before = Some(typed);
        }
        ZOOM.store(settings.zoom, Ordering::Relaxed);
        self.settings = settings;
        for index in 0..self.slots.len() {
            self.settle(index);
        }
        let wanted = self.wanted_positions();
        self.arrange(&wanted, &[], false, false);
        self.refresh(true);
    }

    // On the way out. Whatever still can't go where it belongs goes next to it, under a name of its own.
    fn last_saves(&mut self) {
        self.retry_reads();
        // A file deleted since it was last written: nothing asked for a save, but this is the last chance.
        self.unsaved_settings |= store::settings_gone();
        self.unsaved_todos |= !self.todos.is_empty() && store::todos_gone();
        self.retry_saves();
        if self.unsaved_settings {
            store::set_aside_settings(&self.settings_for_disk());
        }
        if self.unsaved_todos {
            store::set_aside_todos(&self.todos);
        }
    }

    fn switch_count(&mut self) {
        let settings = &mut self.settings;
        settings.count_up = !settings.count_up;
        let moment = now();
        let today = date::days_from_civil(moment.year, moment.month, moment.day);
        let (year, month, day) = settings.countdown_date;
        // Counting up from a day still to come would only count down: today instead, ready to be changed.
        if settings.count_up && date::days_from_civil(year, month, day) > today {
            settings.countdown_date = (moment.year, moment.month, moment.day);
        }
        self.save_settings();
        self.refresh(false);
    }

    // What was being typed when the session ended, kept as clicking away would have.
    fn keep_draft(&mut self, text: String) {
        match self.editing {
            Some(Editing::NewTodo) if !text.is_empty() => {
                self.sync_todos(); // As adding does: an edit made in Notepad meanwhile isn't overwritten.
                self.todos.push(Todo { done: false, text });
                self.unsaved_todos = true;
            }
            // Left as it was offered, the name isn't written either: the settings may have been read meanwhile.
            Some(Editing::CountdownTitle) if !text.is_empty() && self.title_offered.as_deref() != Some(text.as_str()) => {
                self.title_offered = None;
                self.settings.countdown_title = text.replace(['\r', '\n'], " ");
                self.unsaved_settings = true;
            }
            Some(Editing::CountdownDate) => {
                self.title_before = None; // Not cancelled, so the new name counts.
                // Left as it was offered, the date isn't written, the same as when the field closes normally.
                let offered = self.date_offered.take();
                if let Some(date) = date::parse(&text).filter(|_| offered.as_deref() != Some(text.as_str())) {
                    self.settings.countdown_date = date;
                }
                self.unsaved_settings = true; // The name from the step before, at least.
            }
            _ => {}
        }
    }

    fn forget_refused_date(&mut self) {
        if self.date_refused.is_some_and(|at| now().stamp() > at + 1) {
            self.date_refused = None;
        }
    }

    fn data(&self, moment: Now) -> Data<'_> {
        data_of(&self.settings, &self.todos, &self.weather, (self.scroll, self.unsaved_todos, self.date_refused.is_some()), moment)
    }

    fn render(&mut self, index: usize, flip: Option<&Flip>) {
        let card = self.rect_of(index);
        self.render_at(index, flip, POINT { x: card.x, y: card.y });
    }

    /// Draws a card and puts it at `position`, both in one step, so it's never seen in one place with the picture for
    /// another. This is also what gives a card its size.
    fn render_at(&mut self, index: usize, flip: Option<&Flip>, position: POINT) {
        let kind = self.slots[index].kind;
        let window = self.slots[index].window;
        if window.is_invalid() {
            return;
        }
        let scale = scale_of(window);
        let size = self.settings.places[index].size;
        let (width, height) = pixels(size, scale);
        let moment = now();
        let backdrop = backdrop_at(position, scale);
        let palette = self.current_palette();
        let editing = self.editing;
        let marks = (self.scroll, self.unsaved_todos, self.date_refused.is_some());
        let App { gfx, settings, todos, weather, .. } = self;
        gfx.backdrop = Some(backdrop);
        gfx.palette = palette;
        let data = data_of(settings, todos, weather, marks, moment);
        let painted = gfx.paint(width, height, scale, |gfx| cards::draw(gfx, kind, size, moment, &data, flip, editing));
        let shown = painted.is_ok() && gfx.present(window, Some(position), width, height).is_ok();
        // Not shown means the old picture is still up, and the next refresh should try again.
        let key = if shown { cards::content_key(kind, moment, &data) } else { String::new() };
        self.slots[index].key = key;
        if shown && kind == Kind::Clock {
            self.shown_clock = Some(cards::clock_digits(moment));
        }
    }

    /// Draws the cards whose content changed, or all of them with `force`.
    fn refresh(&mut self, force: bool) {
        let moment = now();
        for index in 0..self.slots.len() {
            if !self.settings.places[index].visible {
                continue;
            }
            let kind = self.slots[index].kind;
            let key = cards::content_key(kind, moment, &self.data(moment));
            if !force && key == self.slots[index].key {
                continue;
            }
            self.slots[index].key = key;
            let wanted = kind == Kind::Clock && !force && !on_battery() && !self.covered(index);
            let old = self.shown_clock.clone().filter(|old| wanted && *old != cards::clock_digits(moment));
            match old {
                Some(old) => self.start_flip(index, old),
                None => self.render(index, None),
            }
        }
    }

    // Behind a window nobody sees the flip, and it's the one thing here that costs anything. This only runs when a
    // flip is due, once a minute. The limit is because the windows can be reordered while they're gone through.
    fn covered(&self, index: usize) -> bool {
        let card = self.rect_of(index);
        let mut above = self.slots[index].window;
        for _ in 0..MOST_WINDOWS {
            let Ok(window) = (unsafe { GetWindow(above, GW_HWNDPREV) }) else { return false };
            above = window;
            let mut bounds = RECT::default();
            if hides_what_is_behind(window) && unsafe { GetWindowRect(window, &mut bounds) }.is_ok() && rect_from(bounds).holds(card) {
                return true;
            }
        }
        false
    }

    /// Starts a weather update if one is due. Called from the minute tick, which doesn't run with the screen off.
    fn check_weather(&mut self) {
        let index = index_of(Kind::Weather);
        let due = self.weather.search.is_some() || now().stamp() >= self.weather.next;
        // Without the settings there's no telling whether a place was typed in so that the address wouldn't be sent.
        if !due || self.weather.in_flight || !self.active() || !self.settings.places[index].visible || store::settings_unknown() {
            return;
        }
        let stamp = now().stamp();
        let search = self.weather.search.take().or_else(|| self.weather.retry.take());
        let request = weather::Request {
            place: self.settings.weather_place.clone(),
            locate: stamp - self.weather.located >= LOCATE_MINUTES,
            search: search.clone(),
            revision: self.weather.revision,
        };
        self.weather.in_flight = true;
        let hidden = self.hidden.0 as isize;
        let started = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || {
            let outcome = Box::into_raw(Box::new(weather::fetch(request)));
            let window = HWND(hidden as *mut _);
            if unsafe { PostMessageW(Some(window), WM_WEATHER, WPARAM(0), LPARAM(outcome as isize)) }.is_err() {
                drop(unsafe { Box::from_raw(outcome) });
            }
        });
        if started.is_err() {
            self.weather.in_flight = false;
            self.weather.retry = search;
        }
    }

    fn weather_arrived(&mut self, outcome: weather::Outcome) {
        let stamp = now().stamp();
        let state = &mut self.weather;
        state.in_flight = false;
        if outcome.revision != state.revision {
            // The place was changed while this was on its way, so all it knows is about the wrong one.
            state.next = 0;
            self.check_weather();
            return;
        }
        if outcome.located {
            state.located = stamp;
        }
        let unanswered = matches!(outcome.search, Search::Unanswered(_));
        match outcome.search {
            Search::Done => state.not_found = None,
            Search::NotFound(name) => state.not_found = Some(name),
            Search::Unanswered(name) => state.retry = Some(name),
        }
        if outcome.place != self.settings.weather_place {
            if !same_location(outcome.place.as_ref(), self.settings.weather_place.as_ref()) {
                self.weather.observation = None; // Better "loading" than the old place's weather under the new name.
            }
            self.settings.weather_place = outcome.place;
            self.save_settings();
        }
        let state = &mut self.weather;
        let failed = outcome.observation.is_none() || unanswered;
        if let Some(observation) = outcome.observation {
            state.observation = Some(observation);
            state.updated = stamp;
            state.updated_tick = unsafe { GetTickCount64() };
        }
        if failed {
            // Often just no network yet: try again soon, then less and less.
            state.failures = state.failures.saturating_add(1);
            state.next = stamp + (1i64 << state.failures.min(5)).min(WEATHER_MINUTES);
        } else {
            state.failures = 0;
            state.next = stamp + WEATHER_MINUTES;
        }
        self.refresh(false);
        self.check_weather(); // A place may have been typed in meanwhile.
    }

    // What's on its way now was asked for the place as it was.
    fn place_changed(&mut self) {
        let state = &mut self.weather;
        state.revision = state.revision.wrapping_add(1);
        state.retry = None;
        state.not_found = None;
    }

    fn use_automatic_place(&mut self) {
        if let Some(place) = self.settings.weather_place.as_mut() {
            place.manual = false;
        }
        self.place_changed();
        self.weather.search = None;
        self.weather.located = 0;
        self.weather.next = 0;
        self.save_settings();
        self.refresh(false);
        self.check_weather();
    }

    fn place_typed(&mut self, name: String) {
        let shown = self.settings.weather_place.as_mut().filter(|place| place.name == name);
        if let Some(place) = shown {
            // The place on the card, typed in: nothing to look up, but from now on it stays. That also takes back any
            // other name still waiting for an answer.
            let newly_fixed = !std::mem::replace(&mut place.manual, true);
            self.place_changed();
            self.weather.search = None;
            if newly_fixed {
                self.save_settings();
            }
            self.refresh(false);
            return;
        }
        self.place_changed();
        self.weather.search = Some(name);
        self.refresh(false);
        self.check_weather();
    }

    // Stamps are local time. Once the clock goes back they lie in the future, and nothing would come due for hours.
    fn clock_changed(&mut self) {
        let stamp = now().stamp();
        let state = &mut self.weather;
        state.next = state.next.min(stamp + WEATHER_MINUTES);
        state.located = state.located.min(stamp);
    }

    fn start_flip(&mut self, index: usize, old: (String, String)) {
        self.flipping = Some(Flipping { index, old, started: Instant::now() });
        unsafe { SetTimer(Some(self.hidden), FLIP, FRAME_MS, None) };
    }

    fn flip_frame(&mut self) {
        let Some(flipping) = self.flipping.take() else {
            unsafe { let _ = KillTimer(Some(self.hidden), FLIP); }
            return;
        };
        let progress = flipping.started.elapsed().as_secs_f32() * 1000.0 / FLIP_MS;
        if progress >= 1.0 || !self.active() {
            unsafe { let _ = KillTimer(Some(self.hidden), FLIP); }
            self.render(flipping.index, None);
            return;
        }
        let frame = Flip { old: flipping.old.clone(), progress };
        self.render(flipping.index, Some(&frame));
        self.shown_clock = Some(flipping.old.clone()); // Still on its way there.
        self.flipping = Some(flipping);
    }

    fn position_of(&self, index: usize) -> POINT {
        let place = &self.settings.places[index];
        let kind = KINDS[index];
        if place.placed {
            let scale = system_scale();
            let (width, height) = pixels(place.size, scale);
            let bounds = RECT { left: place.x, top: place.y, right: place.x + width, bottom: place.y + height };
            // A screen that's gone would leave the card out of reach.
            if !unsafe { MonitorFromRect(&bounds, MONITOR_DEFAULTTONULL) }.is_invalid() {
                return POINT { x: place.x, y: place.y };
            }
        }
        default_position(kind)
    }

    fn wanted_positions(&self) -> Vec<POINT> {
        (0..self.slots.len()).map(|index| self.position_of(index)).collect()
    }

    fn create_card(&mut self, index: usize) {
        let position = self.position_of(index);
        unsafe {
            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("DesktopWidgets.Card"),
                w!("DesktopWidgets"),
                WS_POPUP,
                position.x,
                position.y,
                1,
                1,
                Some(desktop()),
                None,
                GetModuleHandleW(None).ok().map(Into::into),
                None,
            );
            let Ok(window) = window else { return };
            SetWindowLongPtrW(window, GWLP_USERDATA, index as isize + 1);
            self.slots[index].window = window;
            self.slots[index].key.clear();
            self.settle(index);
        }
    }

    // Owned by the desktop, sitting right above it: under every normal window, and staying put when shown. To the
    // bottom first, because a window that's new is on top of everything.
    fn settle(&mut self, index: usize) {
        let window = self.slots[index].window;
        let visible = self.settings.places[index].visible;
        if visible {
            self.render(index, None);
        }
        unsafe {
            let _ = SetWindowPos(window, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = ShowWindow(window, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    fn send_to_bottom(&self) {
        for slot in &self.slots {
            unsafe {
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                let _ = SetWindowPos(slot.window, Some(HWND_BOTTOM), 0, 0, 0, 0, flags);
            }
        }
    }

    // Above the other cards and still under every normal window: they're put below this one, it doesn't move up.
    fn lift(&self, index: usize) {
        let window = self.slots[index].window;
        for (other, slot) in self.slots.iter().enumerate() {
            if other != index {
                unsafe {
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                    let _ = SetWindowPos(slot.window, Some(window), 0, 0, 0, 0, flags);
                }
            }
        }
    }

    /// The desktop's owner window goes when Explorer restarts, and takes the cards with it.
    fn recover_cards(&mut self) {
        show_tray(self.hidden, self.tray, NIM_ADD);
        if !unsafe { IsWindow(Some(self.guide)) }.as_bool() {
            self.guide = create_guide();
        }
        for index in 0..self.slots.len() {
            let window = self.slots[index].window;
            if unsafe { IsWindow(Some(window)) }.as_bool() {
                unsafe { SetWindowLongPtrW(window, GWLP_HWNDPARENT, desktop().0 as isize) };
                self.settle(index);
            } else {
                self.create_card(index);
            }
        }
    }

    fn reset_positions(&mut self, icons: &[Rect]) {
        for index in 0..self.slots.len() {
            self.settings.places[index].placed = false;
            self.settings.places[index].size = KINDS[index].default_size();
        }
        let wanted = self.wanted_positions();
        self.arrange(&wanted, icons, true, true);
        self.save_settings();
    }

    /// What changing a card's size would do: where it would go, and where each card it then covers would.
    fn plan_size(&self, index: usize, size: Size, icons: &[Rect]) -> (Rect, Vec<(usize, Rect)>) {
        let scale = scale_of(self.slots[index].window);
        let rules = rules_at(scale);
        let old = self.rect_of(index);
        let area = area_of(old);
        let (width, height) = pixels(size, scale);
        let shown: Vec<usize> = (0..self.slots.len()).filter(|other| self.settings.places[*other].visible).collect();
        let mut blockers: Vec<Rect> = shown.iter().filter(|other| **other != index).map(|other| self.rect_of(*other)).collect();
        blockers.extend_from_slice(icons);
        let card = snap::free_spot(old, width, height, &blockers, area, &rules).unwrap_or_else(|| {
            // Other cards can make room below, but icons don't move.
            let squeezed = snap::squeezed(Rect { w: width, h: height, ..old }, area, &rules);
            let (x, y) = snap::keep_clear(squeezed, &[], icons, area, &rules);
            Rect { x, y, ..squeezed }
        });

        let before: Vec<Rect> = shown.iter().map(|other| if *other == index { card } else { self.rect_of(*other) }).collect();
        let mut after = before.clone();
        let grown = shown.iter().position(|other| *other == index).unwrap_or(0);
        snap::make_room(&mut after, grown, icons, area, &rules);
        let moved = (0..shown.len()).filter(|position| after[*position] != before[*position]);
        (card, moved.map(|position| (shown[position], after[position])).collect())
    }

    // Puts a card down for good.
    fn place(&mut self, index: usize, card: Rect) {
        let place = &mut self.settings.places[index];
        (place.x, place.y, place.placed) = (card.x, card.y, true);
        self.render_at(index, None, POINT { x: card.x, y: card.y });
    }

    /// Changes a card's size, keeps it on the screen and moves whatever it now covers.
    fn set_size(&mut self, index: usize, size: Size, icons: &[Rect]) {
        if self.settings.places[index].size == size {
            return;
        }
        let (card, moved) = self.plan_size(index, size, icons);
        self.settings.places[index].size = size;
        self.place(index, card);
        for (other, rect) in moved {
            self.place(other, rect);
        }
        self.save_settings();
    }

    fn set_zoom(&mut self, percent: u32, icons: &[Rect]) {
        let before = self.settings.zoom;
        if percent == before {
            return;
        }
        rescale_places(&mut self.settings, percent as f32 / before as f32);
        self.settings.zoom = percent;
        ZOOM.store(percent, Ordering::Relaxed);
        let wanted = self.wanted_positions();
        self.arrange(&wanted, icons, true, true);
        self.save_settings();
    }

    /// Puts each shown card as near as it can get to where it's wanted, clear of the ones before it. `pull_in` also
    /// keeps them whole on their screen. `keep` makes the result the saved layout: a passing change of screen shouldn't,
    /// so that the cards go back when the screen does.
    fn arrange(&mut self, wanted: &[POINT], icons: &[Rect], pull_in: bool, keep: bool) {
        let mut settled: Vec<Rect> = Vec::new();
        let mut changed = false;
        for index in 0..self.slots.len() {
            if !self.settings.places[index].visible {
                continue;
            }
            let scale = scale_of(self.slots[index].window);
            let (w, h) = pixels(self.settings.places[index].size, scale);
            let spot = Rect { x: wanted[index].x, y: wanted[index].y, w, h };
            let area = area_of(spot);
            let card = if pull_in { snap::inside(spot, area) } else { spot };
            let (x, y) = snap::keep_clear(card, &settled, icons, area, &rules_at(scale));
            let card = Rect { x, y, ..card };
            if card != self.rect_of(index) {
                self.render_at(index, None, POINT { x, y });
            }
            if keep && card != spot {
                let place = &mut self.settings.places[index];
                (place.x, place.y, place.placed) = (x, y, true);
                changed = true;
            }
            settled.push(card);
        }
        if changed {
            self.save_settings();
        }
    }

    // Several messages can come for one change. Dealing with them later also lets all the cards get theirs first.
    fn ask_for_relayout(&mut self) {
        if !self.relayout_pending {
            self.relayout_pending = true;
            let _ = unsafe { PostMessageW(Some(self.hidden), WM_RELAYOUT, WPARAM(0), LPARAM(0)) };
        }
    }

    fn catch_up_on_relayout(&mut self) {
        if std::mem::take(&mut self.relayout_missed) {
            self.ask_for_relayout();
        }
    }

    /// The screen changed: cards go back to where they belong, as far as this screen allows. That's not saved.
    fn relayout(&mut self, icons: &[Rect]) {
        self.relayout_pending = false;
        if self.drag.is_some() || self.glide.is_some() {
            self.relayout_missed = true;
            return;
        }
        let (width, height) = screen_size();
        let (old_width, old_height) = self.screen;
        if i64::from(width) * i64::from(old_height) != i64::from(height) * i64::from(old_width) {
            self.load_wallpaper(); // A different shape crops it differently. Costly, and rarely needed.
        }
        self.screen = (width, height);
        let wanted = self.wanted_positions();
        self.arrange(&wanted, icons, true, false);
        self.refresh(true);
    }

    fn toggle_card(&mut self, index: usize, icons: &[Rect]) {
        let showing = !self.settings.places[index].visible;
        self.settings.places[index].visible = showing;
        if showing {
            // Its spot may have been taken since. A card hidden from the start has no real size yet, so it's worked out.
            let window = self.slots[index].window;
            let scale = scale_of(window);
            let (w, h) = pixels(self.settings.places[index].size, scale);
            let position = self.position_of(index);
            let spot = Rect { x: position.x, y: position.y, w, h };
            let area = area_of(spot);
            let others: Vec<Rect> = (0..self.slots.len())
                .filter(|other| *other != index && self.settings.places[*other].visible)
                .map(|other| self.rect_of(other))
                .collect();
            let card = snap::inside(spot, area);
            let (x, y) = snap::keep_clear(card, &others, icons, area, &rules_at(scale));
            if (x, y) != (spot.x, spot.y) {
                let place = &mut self.settings.places[index];
                (place.x, place.y, place.placed) = (x, y, true);
            }
            unsafe {
                let _ = SetWindowPos(window, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
        self.settle(index);
        if showing {
            self.lift(index); // If there was no room at all, it at least shouldn't look as if nothing happened.
        }
        self.save_settings();
        self.check_weather();
    }

    // Whether the pointer just went from clicking to dragging. Worth a look at the icons then, unless nothing can move.
    fn pointer_travelled(&mut self, window: HWND, cursor: POINT) -> bool {
        let (limit_x, limit_y) = drag_threshold(window);
        let far = |from: POINT| (cursor.x - from.x).abs() >= limit_x || (cursor.y - from.y).abs() >= limit_y;
        let locked = self.settings.locked;
        if let Some(drag) = self.drag.as_mut() {
            let first = !drag.travelled && far(drag.cursor);
            drag.travelled |= first;
            return first && !locked;
        }
        false
    }

    fn follow_pointer(&mut self, window: HWND, cursor: POINT) {
        let locked = self.settings.locked;
        let Some(drag) = self.drag.as_mut().filter(|drag| drag.travelled && !locked) else { return };
        let starting = !drag.moved;
        drag.moved = true;
        let index = drag.index;
        let (x, y) = (drag.origin.x + cursor.x - drag.cursor.x, drag.origin.y + cursor.y - drag.cursor.y);
        unsafe {
            let _ = SetWindowPos(window, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        if starting {
            self.lift(index); // Otherwise it would slide under the cards it passes.
            self.paint_guide(index, self.settings.places[index].size);
        }
        let target = self.snap_target(index);
        if let Some(drag) = self.drag.as_mut() {
            drag.target = target;
        }
        self.show_guide(index, target);
    }

    // Lets go of a drag that ended, however it ended. Some(true) if the pointer went far enough not to be a click.
    fn finish_drag(&mut self) -> Option<bool> {
        let drag = self.drag.take()?;
        if drag.moved {
            unsafe {
                let _ = ShowWindow(self.guide, SW_HIDE);
            }
            // Ctrl may have gone down or up since the pointer last moved.
            let target = self.snap_target(drag.index);
            self.start_glide(drag.index, target);
        }
        self.catch_up_on_relayout();
        Some(drag.travelled)
    }

    fn rect_of(&self, index: usize) -> Rect {
        let mut bounds = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.slots[index].window, &mut bounds);
        }
        rect_from(bounds)
    }

    /// Where the card being dragged would settle if it were let go now: whole on the screen, wherever the pointer is.
    fn snap_target(&self, index: usize) -> POINT {
        let moving = self.rect_of(index);
        let others: Vec<Rect> = (0..self.slots.len())
            .filter(|other| *other != index && self.settings.places[*other].visible)
            .map(|other| self.rect_of(other))
            .collect();
        let rules = rules_at(scale_of(self.slots[index].window));
        let area = area_of(moving);
        let moving = snap::inside(moving, area);
        // Ctrl turns off the alignment, as Command does on the Mac. Cards still can't overlap.
        let free = unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) } < 0;
        let (x, y) = if free {
            snap::keep_clear(moving, &others, &self.icons, area, &rules)
        } else {
            snap::settle(moving, &others, &self.icons, area, &rules)
        };
        POINT { x, y }
    }

    // The outline of where the card will land, under the card itself.
    fn show_guide(&self, index: usize, target: POINT) {
        let current = self.rect_of(index);
        unsafe {
            if target.x == current.x && target.y == current.y {
                let _ = ShowWindow(self.guide, SW_HIDE);
                return;
            }
            let flags = SWP_NOSIZE | SWP_NOACTIVATE;
            let _ = SetWindowPos(self.guide, Some(self.slots[index].window), target.x, target.y, 0, 0, flags);
            let _ = ShowWindow(self.guide, SW_SHOWNOACTIVATE);
        }
    }

    fn paint_guide(&mut self, index: usize, size: Size) {
        let scale = scale_of(self.slots[index].window);
        let (width, height) = pixels(size, scale);
        let guide = self.guide;
        if self.gfx.paint(width, height, scale, |gfx| cards::draw_guide(gfx, size)).is_ok() {
            let _ = self.gfx.present(guide, None, width, height);
        }
    }

    fn start_glide(&mut self, index: usize, to: POINT) {
        let rect = self.rect_of(index);
        self.glide = Some(Glide { index, from: POINT { x: rect.x, y: rect.y }, to, started: Instant::now() });
        if (rect.x, rect.y) == (to.x, to.y) {
            self.finish_glide(); // Already there.
        } else {
            unsafe { SetTimer(Some(self.hidden), GLIDE, GLIDE_FRAME_MS, None) };
        }
    }

    fn glide_frame(&mut self) {
        let Some(glide) = self.glide.as_ref() else {
            unsafe { let _ = KillTimer(Some(self.hidden), GLIDE); }
            return;
        };
        let linear = glide.started.elapsed().as_secs_f32() * 1000.0 / GLIDE_MS;
        if linear >= 1.0 {
            self.finish_glide();
            return;
        }
        let eased = 1.0 - (1.0 - linear).powi(3);
        let between = |a: i32, b: i32| a + ((b - a) as f32 * eased).round() as i32;
        let (x, y) = (between(glide.from.x, glide.to.x), between(glide.from.y, glide.to.y));
        unsafe {
            let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
            let _ = SetWindowPos(self.slots[glide.index].window, None, x, y, 0, 0, flags);
        }
    }

    // Lands a gliding card, however far along it is: halfway it could be on top of another card, and unsaved.
    fn finish_glide(&mut self) {
        let Some(glide) = self.glide.take() else { return };
        unsafe { let _ = KillTimer(Some(self.hidden), GLIDE); }
        let place = &mut self.settings.places[glide.index];
        (place.x, place.y, place.placed) = (glide.to.x, glide.to.y, true);
        self.render_at(glide.index, None, glide.to); // The wallpaper behind it changed.
        self.save_settings();
        self.catch_up_on_relayout();
    }

    fn todo_start(&self) -> usize {
        let size = self.settings.places[index_of(Kind::Todo)].size;
        cards::todo_start(self.todos.len(), size, self.scroll)
    }

    fn scroll_todos(&mut self, by: isize) {
        self.scroll = self.todo_start().saturating_add_signed(by);
        self.refresh(false);
    }

    fn next_todo_page(&mut self) {
        // The note changes under the pointer, so the second click of a double click would turn straight back.
        if self.last_page.is_some_and(|at| at.elapsed() < double_click_time()) {
            return;
        }
        self.last_page = Some(Instant::now());
        let size = self.settings.places[index_of(Kind::Todo)].size;
        self.scroll = cards::next_todo_page(self.todos.len(), size, self.todo_start());
        self.refresh(false);
    }

    fn wheel_moved(&mut self, delta: i32) {
        let (notches, rest) = cards::wheel_notches(self.wheel_rest, delta);
        self.wheel_rest = rest;
        if notches != 0 && !self.sync_todos() {
            self.scroll_todos(-notches as isize);
        }
    }

    // Picks up an edit made in another program, and shows it. True if there was one: what was clicked isn't where it
    // was any more. Not while the card has changes of its own to save: those go first, and the file gets a copy.
    fn sync_todos(&mut self) -> bool {
        if self.unsaved_todos {
            return false;
        }
        let Some(todos) = store::todos_changed_outside() else { return false };
        self.todos = todos;
        self.refresh(false);
        true
    }

    fn toggle_todo(&mut self, row: usize) {
        // The list reorders at once, so the second click of a double click would land on the next to-do.
        if self.last_toggle.is_some_and(|(at, known)| known == row && at.elapsed() < double_click_time()) {
            return;
        }
        if let Some(position) = cards::todo_order(&self.todos).get(row).copied() {
            self.todos[position].done = !self.todos[position].done;
            self.last_toggle = Some((Instant::now(), row));
            self.refresh(false); // Before the disk, which can take a moment.
            self.save_todos();
        }
    }

    // `then` is the list as it was when the menu opened. If it has changed since (an edit elsewhere, or the file read at
    // last while the menu was open), the row no longer says which to-do was meant, and nothing is deleted.
    fn delete_todo(&mut self, row: usize, then: &[store::Todo]) {
        if self.sync_todos() || self.todos != then {
            return;
        }
        if let Some(position) = cards::todo_order(&self.todos).get(row).copied() {
            self.todos.remove(position);
            self.refresh(false);
            self.save_todos();
        }
    }
}

// The part of the wallpaper behind a card at this position.
fn backdrop_at(position: POINT, scale: f32) -> gfx::Backdrop {
    let (width, height) = screen_size();
    gfx::Backdrop {
        left: position.x as f32 / width as f32,
        top: position.y as f32 / height as f32,
        screen_width: width as f32 / scale,
    }
}

fn data_of<'a>(
    settings: &'a Settings,
    todos: &'a [Todo],
    state: &'a Weather,
    (scroll, unsaved, date_refused): (usize, bool, bool),
    moment: Now,
) -> Data<'a> {
    let size = settings.places[index_of(Kind::Todo)].size;
    Data {
        settings,
        todos,
        todo_start: cards::todo_start(todos.len(), size, scroll),
        todos_unsaved: unsaved,
        todos_unreadable: store::todos_unreadable(),
        date_refused,
        weather: weather_view(settings, state, moment),
    }
}

// Minutes since 1970, in UTC.
fn utc_stamp() -> i64 {
    let time = unsafe { GetSystemTime() };
    let day = date::days_from_civil(i32::from(time.wYear), u32::from(time.wMonth), u32::from(time.wDay));
    day * DAY_MINUTES + i64::from(time.wHour) * 60 + i64::from(time.wMinute)
}

fn weather_view<'a>(settings: &'a Settings, state: &'a Weather, moment: Now) -> WeatherView<'a> {
    let age = (unsafe { GetTickCount64() }.saturating_sub(state.updated_tick) / 60_000) as i64;
    let stale = state.observation.is_some() && age >= STALE_MINUTES;
    let when = || {
        if age >= DAY_MINUTES {
            return format!("{} 天前", age / DAY_MINUTES); // A time alone would look like today's.
        }
        let minutes = state.updated.rem_euclid(DAY_MINUTES);
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    };
    let here = date::days_from_civil(moment.year, moment.month, moment.day);
    WeatherView {
        place: settings.weather_place.as_ref().map(|place| place.name.as_str()),
        observation: state.observation.as_ref(),
        stale_since: stale.then(when),
        not_found: state.not_found.as_deref(),
        failed: state.failures >= 2,
        today: state.observation.as_ref().map_or(here, |observation| observation.today(utc_stamp(), here)),
    }
}

fn same_location(a: Option<&weather::Place>, b: Option<&weather::Place>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.name == b.name && a.latitude == b.latitude && a.longitude == b.longitude,
        (None, None) => true,
        _ => false,
    }
}

fn is_desktop(window: HWND) -> bool {
    let mut class = [0u16; 16];
    let length = unsafe { GetClassNameW(window, &mut class) } as usize;
    let name = String::from_utf16_lossy(&class[..length]);
    name == "Progman" || name == "WorkerW"
}

// Whether a window that's over something really hides it. Some are only there on another virtual desktop, and some
// are see-through overlays.
fn hides_what_is_behind(window: HWND) -> bool {
    unsafe {
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(window, DWMWA_CLOAKED, (&raw mut cloaked).cast(), size_of::<u32>() as u32);
        let clear = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0;
        IsWindowVisible(window).as_bool() && !IsIconic(window).as_bool() && cloaked == 0 && !clear && !is_desktop(window)
    }
}

fn desktop() -> HWND {
    unsafe { FindWindowW(w!("Progman"), None) }.unwrap_or_default()
}

fn low_word(value: isize) -> i32 {
    i32::from((value & 0xffff) as u16 as i16)
}

fn high_word(value: isize) -> i32 {
    i32::from(((value >> 16) & 0xffff) as u16 as i16)
}

fn create_guide() -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("DesktopWidgets.Card"),
            w!("DesktopWidgets"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(desktop()),
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        )
    }
    .unwrap_or_default()
}

fn add_item(menu: HMENU, id: usize, text: &str, checked: bool, enabled: bool) {
    let label = HSTRING::from(text);
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if !enabled {
        flags |= MF_GRAYED;
    }
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr()));
    }
}

/// The menu, for a card, or with `None` for the icon by the clock, which has only what's about all of them.
fn show_menu(card: Option<usize>, row: Option<usize>) {
    unsafe {
        let Some((hidden, kind, size, locked, visible, manual, current_zoom, unsaved, count_up, todos_then)) = with(|app| {
            (
                app.hidden,
                card.map(|index| app.slots[index].kind),
                card.map(|index| app.settings.places[index].size),
                app.settings.locked,
                app.settings.places.iter().map(|place| place.visible).collect::<Vec<_>>(),
                app.settings.weather_place.as_ref().is_some_and(|place| place.manual),
                app.settings.zoom,
                app.unsaved_settings || app.unsaved_todos,
                app.settings.count_up,
                row.map(|_| app.todos.clone()).unwrap_or_default(),
            )
        }) else {
            return;
        };
        let Ok(menu) = CreatePopupMenu() else { return };
        let Ok(cards_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        let Ok(zoom_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(cards_menu);
            let _ = DestroyMenu(menu);
            return;
        };

        if unsaved {
            add_item(menu, 0, "有變更還沒存檔，稍後會重試", false, false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        let shown = visible.iter().filter(|value| **value).count();
        for (position, card) in KINDS.iter().enumerate() {
            // The menu lives on the cards, so the last one has to stay.
            let last = visible[position] && shown == 1;
            add_item(cards_menu, MENU_SHOW + position, card.title(), visible[position], !last);
        }
        if kind == Some(Kind::Todo) {
            add_item(menu, MENU_ADD, "新增待辦", false, true);
            if row.is_some() {
                add_item(menu, MENU_DELETE, "刪除這一項", false, true);
            }
        }
        if kind == Some(Kind::Countdown) {
            add_item(menu, MENU_COUNTDOWN, if count_up { "編輯正數" } else { "編輯倒數" }, false, true);
            add_item(menu, MENU_COUNT_UP, "改成正數（從某一天算起）", count_up, true);
        }
        if kind == Some(Kind::Weather) {
            add_item(menu, MENU_PLACE, "編輯地區", false, true);
            add_item(menu, MENU_AUTO_PLACE, "使用目前位置", !manual, true);
        }
        if matches!(kind, Some(Kind::Todo | Kind::Countdown | Kind::Weather)) {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        // Everything that moves cards around stops with the lock.
        if let (Some(kind), Some(size)) = (kind, size) {
            for (position, choice) in kind.sizes().iter().enumerate() {
                add_item(menu, MENU_SIZE + position, choice.title(), *choice == size, !locked);
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        for (position, percent) in ZOOMS.iter().enumerate() {
            add_item(zoom_menu, MENU_ZOOM + position, &format!("{percent}%"), *percent == current_zoom, true);
        }
        let zoom_flags = if locked { MF_POPUP | MF_GRAYED } else { MF_POPUP };
        let _ = AppendMenuW(menu, zoom_flags, zoom_menu.0 as usize, w!("整體大小"));
        let _ = AppendMenuW(menu, MF_POPUP, cards_menu.0 as usize, w!("顯示卡片"));
        add_item(menu, MENU_LOCK, "鎖定位置", locked, true);
        add_item(menu, MENU_RESET, "重設位置和大小", false, !locked);
        add_item(menu, MENU_STARTUP, "開機時啟動", starts_with_windows(), true);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        add_item(menu, MENU_EXIT, "結束", false, true);

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let previous = GetForegroundWindow();
        let _ = SetForegroundWindow(hidden); // Otherwise the menu won't close on a click elsewhere.
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, cursor.x, cursor.y, None, hidden, None);
        let _ = PostMessageW(Some(hidden), WM_NULL, WPARAM(0), LPARAM(0));
        let command = chosen.0 as usize;
        // Not back to the desktop: activating it ends Show Desktop and brings every window back. And not yet if a text
        // field comes next: it needs the foreground this has, and gives it back when it closes.
        let typing = matches!(command, MENU_ADD | MENU_COUNTDOWN | MENU_COUNT_UP | MENU_PLACE);
        if !typing && GetForegroundWindow() == hidden && !is_desktop(previous) {
            let _ = SetForegroundWindow(previous);
        }
        let _ = DestroyMenu(menu);
        run_command(command, card, row, &todos_then, previous);
    }
}

// `todos_then`: the to-do list when the menu opened, which `row` counts in.
fn run_command(command: usize, card: Option<usize>, row: Option<usize>, todos_then: &[store::Todo], previous: HWND) {
    match command {
        0 => {}
        MENU_LOCK => {
            with(|app| {
                app.settings.locked = !app.settings.locked;
                app.save_settings();
            });
        }
        MENU_RESET => {
            let icons = icons::rects();
            with(|app| app.reset_positions(&icons));
        }
        MENU_STARTUP => set_starts_with_windows(!starts_with_windows()),
        MENU_COUNTDOWN => edit_countdown(After::Window(previous)),
        MENU_COUNT_UP => {
            with(App::switch_count);
            edit_countdown(After::Window(previous)); // Straight on to the name and the day it counts from.
        }
        MENU_ADD => add_todo(After::Window(previous)),
        MENU_PLACE => edit_place(After::Window(previous)),
        MENU_AUTO_PLACE => {
            with(App::use_automatic_place);
        }
        MENU_DELETE => {
            if let Some(row) = row {
                with(|app| app.delete_todo(row, todos_then));
            }
        }
        MENU_EXIT => {
            with(|app| unsafe {
                let _ = DestroyWindow(app.hidden);
            });
        }
        choice if (MENU_ZOOM..MENU_ZOOM + ZOOMS.len()).contains(&choice) => {
            let icons = icons::rects();
            with(|app| app.set_zoom(ZOOMS[choice - MENU_ZOOM], &icons));
        }
        choice if (MENU_SIZE..MENU_SIZE + 3).contains(&choice) => {
            let Some(index) = card else { return };
            let icons = icons::rects();
            with(|app| {
                let sizes = app.slots[index].kind.sizes();
                if let Some(size) = sizes.get(choice - MENU_SIZE) {
                    app.set_size(index, *size, &icons);
                }
            });
        }
        card if (MENU_SHOW..MENU_SHOW + KINDS.len()).contains(&card) => {
            let icons = icons::rects();
            with(|app| app.toggle_card(card - MENU_SHOW, &icons));
        }
        _ => {}
    }
}

// Clears the spot on the card and works out the edit box that goes over it.
fn begin_edit(kind: Kind, editing: Editing) -> Option<prompt::Field> {
    with(|app| {
        let index = index_of(kind);
        app.editing = Some(editing);
        app.render(index, None);

        let window = app.slots[index].window;
        let scale = scale_of(window);
        let bounds = app.rect_of(index);
        let size = app.settings.places[index].size;
        let ((x, y, width, height), font) = cards::edit_spot(editing, size, &app.todos);
        let middle = y + height / 2.0;
        let line = (font * 1.5 * scale).ceil() as i32;
        let (field_left, field_top) = ((x * scale) as i32, (middle * scale) as i32 - line / 2);
        let field_width = (width * scale) as i32;
        let background = app.gfx.sample(field_left, field_left + field_width, (middle * scale) as i32);
        let backdrop = app.gfx.region(field_left, field_top, field_width, line);
        let primary = app.gfx.palette.primary;
        let blend = |value: f32, under: u8| (value * primary.a * 255.0 + f32::from(under) * (1.0 - primary.a)) as u8;
        prompt::Field {
            x: bounds.x + field_left,
            y: bounds.y + field_top,
            width: field_width,
            height: line,
            font_size: font * scale,
            semibold: false,
            text: [blend(primary.r, background[0]), blend(primary.g, background[1]), blend(primary.b, background[2])],
            backdrop,
        }
    })
}

fn end_edit(kind: Kind) {
    with(|app| {
        app.editing = None;
        app.send_to_bottom();
        app.render(index_of(kind), None);
        app.refresh(false);
    });
}

fn edit_place(after: After) {
    let current = with(|app| app.settings.weather_place.as_ref().map(|place| place.name.clone())).flatten();
    let Some(field) = begin_edit(Kind::Weather, Editing::Place) else { return };
    let (answer, field) = prompt::ask(&field, current.as_deref().unwrap_or(""), after);
    // The settings may have been read from the file while the field was open: then the name it was opened with, left as
    // it was, isn't a choice of the old place over the one there is now.
    let now = with(|app| app.settings.weather_place.as_ref().map(|place| place.name.clone())).flatten();
    let untouched_but_changed = |name: &String| Some(name) == current.as_ref() && now != current;
    match answer {
        Answer::Entered(name) if untouched_but_changed(&name) => {}
        Answer::Entered(name) if !name.is_empty() => {
            with(|app| app.place_typed(name));
        }
        // Clicking away from what was already there isn't asking for anything.
        Answer::Left(name) if !name.is_empty() && Some(&name) != current.as_ref() => {
            with(|app| app.place_typed(name));
        }
        _ => {}
    }
    end_edit(Kind::Weather);
    drop(field); // Only now, with the card drawn underneath it.
}

fn add_todo(after: After) {
    let Some(field) = begin_edit(Kind::Todo, Editing::NewTodo) else { return };
    let (answer, field) = prompt::ask(&field, "", after);
    let mut added = false;
    if let Answer::Entered(text) | Answer::Left(text) = answer {
        if !text.is_empty() {
            added = true;
            with(|app| {
                app.sync_todos();
                // Scrolled to where it was typed, so it stays in sight.
                let size = app.settings.places[index_of(Kind::Todo)].size;
                app.scroll = cards::adding_spot(&app.todos, size).0;
                app.todos.push(Todo { done: false, text });
            });
        }
    }
    end_edit(Kind::Todo);
    drop(field); // Only now, with the new row drawn underneath it.
    if added {
        with(App::save_todos);
    }
}

fn edit_countdown(after: After) {
    let Some((title, target)) = with(|app| {
        app.date_refused = None;
        app.title_offered = Some(app.settings.countdown_title.clone());
        (app.settings.countdown_title.clone(), app.settings.countdown_date)
    }) else {
        return;
    };
    let Some(field) = begin_edit(Kind::Countdown, Editing::CountdownTitle) else {
        with(|app| app.title_offered = None);
        return;
    };
    let (answer, mut open) = prompt::ask(&field, &title, after);
    with(|app| app.title_offered = None);
    // Nothing typed, or the name offered left as it was, keeps the name there is now, which may have been read from the
    // file meanwhile: the field still holds the one it was opened with.
    let named = |text: String| {
        if text.is_empty() || text == title {
            with(|app| app.settings.countdown_title.clone()).unwrap_or_else(|| title.clone())
        } else {
            text.replace(['\r', '\n'], " ")
        }
    };
    let new_title = match answer {
        Answer::Cancelled => {
            end_edit(Kind::Countdown);
            return;
        }
        // Clicking away means done, so it doesn't go on to a date the user has already left behind.
        Answer::Left(text) => {
            let name = named(text); // Not inside `with`, which `named` uses itself.
            with(|app| app.settings.countdown_title = name);
            end_edit(Kind::Countdown);
            drop(open);
            with(App::save_settings);
            return;
        }
        Answer::Entered(text) => named(text),
    };

    // The card shows the new name during the second step, but it only counts if that isn't cancelled. The name and
    // date to go back to are the ones there are now: the settings may have been read from the file while the name was
    // being typed, and what this edit began with would be out of date.
    let target = with(|app| {
        app.title_before = Some(app.settings.countdown_title.clone());
        app.settings.countdown_title = new_title;
        app.settings.countdown_date
    })
    .unwrap_or(target);
    let mut typed = format!("{:04}-{:02}-{:02}", target.0, target.1, target.2);
    let offered = typed.clone();
    with(|app| app.date_offered = Some(offered.clone()));
    let mut cancelled = false;
    loop {
        let Some(field) = begin_edit(Kind::Countdown, Editing::CountdownDate) else { break };
        let previous = open.take().map_or(After::Foreground, After::Field);
        let (answer, next) = prompt::ask(&field, &typed, previous);
        open = next;
        match answer {
            Answer::Cancelled => {
                cancelled = true;
                break;
            }
            // Not a date: back to it with the text kept, rather than quietly doing nothing.
            Answer::Entered(text) if !text.is_empty() && date::parse(&text).is_none() => typed = text,
            Answer::Entered(text) | Answer::Left(text) => {
                match date::parse(&text) {
                    // Left as it was offered, the date isn't written: whatever is in the settings by now stays.
                    Some(_) if text == offered => {}
                    Some(new_date) => {
                        with(|app| app.settings.countdown_date = new_date);
                    }
                    // The field can't stay once the click went elsewhere, so the card says what became of it.
                    None if !text.is_empty() => {
                        with(|app| app.date_refused = Some(now().stamp()));
                    }
                    None => {}
                }
                break;
            }
        }
    }
    with(|app| {
        // Cancelled, the name goes back to the one kept meanwhile: the settings may have been read since it began.
        let before = app.title_before.take();
        app.date_offered = None;
        if cancelled {
            app.settings.countdown_title = before.unwrap_or_else(|| title.clone());
        }
    });
    end_edit(Kind::Countdown);
    drop(open);
    if !cancelled {
        with(App::save_settings);
    }
}

fn click(index: usize, x: f32, y: f32) {
    if with(|app| app.slots[index].kind) != Some(Kind::Todo) {
        return;
    }
    let Some((reloaded, size, count, start)) = with(|app| {
        let reloaded = app.sync_todos();
        (reloaded, app.settings.places[index].size, app.todos.len(), app.todo_start())
    }) else {
        return;
    };
    match cards::todo_hit(x, y, size, count, start) {
        TodoHit::Add => add_todo(After::Foreground),
        // The row that was aimed at may not be there any more, so this click only showed the new list.
        _ if reloaded => {}
        TodoHit::Row(row) => {
            with(|app| app.toggle_todo(row));
        }
        TodoHit::More => {
            with(App::next_todo_page);
        }
        TodoHit::Nothing => {}
    }
}

fn client_point(window: HWND, lparam: LPARAM) -> (f32, f32) {
    let scale = scale_of(window);
    (low_word(lparam.0) as f32 / scale, high_word(lparam.0) as f32 / scale)
}

unsafe extern "system" fn card_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let stored = GetWindowLongPtrW(window, GWLP_USERDATA);
    if stored == 0 {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let index = stored as usize - 1;
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // A click on a card doesn't move the focus, so it wouldn't close a field by itself. Going by the button coming
        // up as well: an input method in the middle of a word takes the press for itself, to finish the word.
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP if prompt::is_open() => {
            if !prompt::just_opened() {
                prompt::finish();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            with(|app| {
                app.finish_glide();
                let card = app.rect_of(index);
                let origin = POINT { x: card.x, y: card.y };
                app.drag = Some(Drag { index, cursor, origin, travelled: false, moved: false, target: origin });
            });
            SetCapture(window);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with(|app| {
                app.finish_drag(); // Taken away mid-drag, by Alt+Tab for one.
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE if wparam.0 & MK_LBUTTON != 0 => {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            if with(|app| app.pointer_travelled(window, cursor)) == Some(true) {
                let found = icons::rects(); // This waits on Explorer, so not while `with` is holding the app.
                with(|app| app.icons = found);
            }
            with(|app| app.follow_pointer(window, cursor));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = client_point(window, lparam);
            let clicked = with(App::finish_drag) == Some(Some(false));
            let _ = ReleaseCapture();
            if clicked {
                click(index, x, y);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            with(App::finish_glide);
            let (x, y) = client_point(window, lparam);
            let row = with(|app| {
                if app.slots[index].kind != Kind::Todo {
                    return None;
                }
                // What the pointer was aimed at may have moved, so no deleting until the new list has been seen.
                if app.sync_todos() {
                    return None;
                }
                let size = app.settings.places[index].size;
                match cards::todo_hit(x, y, size, app.todos.len(), app.todo_start()) {
                    TodoHit::Row(row) => Some(row),
                    _ => None,
                }
            });
            show_menu(Some(index), row.flatten());
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = high_word(wparam.0 as isize);
            with(|app| {
                if app.slots[index].kind == Kind::Todo {
                    app.wheel_moved(delta);
                }
            });
            LRESULT(0)
        }
        // Sent from inside SetWindowPos while a drag holds the app, when that can't be dealt with yet: then later.
        WM_DPICHANGED | WM_DPI_LATER => {
            let dealt_with = with(|app| {
                if let Some(dragged) = app.drag.as_ref().map(|drag| drag.index) {
                    app.render(index, None); // Carried over to another screen. Letting go sorts out the rest.
                    if dragged == index {
                        let size = app.settings.places[index].size;
                        app.paint_guide(index, size); // The outline of where it lands, at the new size too.
                    }
                    app.relayout_missed = true;
                } else {
                    app.ask_for_relayout();
                }
            });
            if dealt_with.is_none() {
                let _ = PostMessageW(Some(window), WM_DPI_LATER, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn power_event(wparam: WPARAM, lparam: LPARAM) {
    match wparam.0 {
        PBT_POWERSETTINGCHANGE => {
            let setting = lparam.0 as *const POWERBROADCAST_SETTING;
            if (*setting).PowerSetting != DISPLAY_STATE {
                return;
            }
            let state = std::ptr::read_unaligned((&raw const (*setting).Data).cast::<u32>());
            with(|app| {
                let was_active = app.active();
                app.display_on = state != 0;
                if app.active() && !was_active {
                    app.refresh(false);
                    app.check_weather();
                }
                app.schedule();
            });
        }
        PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => {
            with(|app| {
                app.refresh(false);
                app.schedule();
            });
        }
        _ => {}
    }
}

unsafe extern "system" fn hidden_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = with(|app| app.taskbar_created).unwrap_or(0);
    match message {
        WM_TIMER if wparam.0 == TICK => {
            with(|app| {
                app.retry_reads();
                app.forget_refused_date();
                app.refresh(false);
                app.schedule();
                app.check_weather();
                app.retry_saves();
            });
            LRESULT(0)
        }
        WM_WEATHER => {
            let outcome = Box::from_raw(lparam.0 as *mut weather::Outcome);
            with(|app| app.weather_arrived(*outcome));
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == FLIP => {
            with(App::flip_frame);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == GLIDE => {
            with(App::glide_frame);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            power_event(wparam, lparam);
            LRESULT(1)
        }
        WM_WTSSESSION_CHANGE => {
            with(|app| {
                match wparam.0 {
                    WTS_SESSION_LOCK => app.session_locked = true,
                    WTS_SESSION_UNLOCK => {
                        app.session_locked = false;
                        app.refresh(false);
                        app.check_weather();
                    }
                    _ => return,
                }
                app.schedule();
            });
            LRESULT(0)
        }
        WM_TIMECHANGE => {
            with(|app| {
                app.clock_changed();
                app.refresh(false);
                app.schedule();
                app.check_weather();
            });
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            // This broadcast comes for all sorts of settings; only two of them show on the cards.
            let wallpaper_changed = wparam.0 == SPI_SETDESKWALLPAPER.0 as usize;
            // A dock or the taskbar took or gave back room: the cards go where this leaves them, unsaved.
            if wparam.0 == SPI_SETWORKAREA.0 as usize {
                with(App::ask_for_relayout);
            }
            with(|app| {
                let dark = wants_dark();
                if wallpaper_changed {
                    app.load_wallpaper();
                }
                let icon = tray_icon();
                if !icon.is_invalid() {
                    show_tray(app.hidden, icon, NIM_MODIFY);
                    let _ = unsafe { DestroyIcon(std::mem::replace(&mut app.tray, icon)) };
                }
                if wallpaper_changed || dark != app.dark {
                    app.dark = dark;
                    app.refresh(true);
                }
            });
            LRESULT(0)
        }
        // Version 4 puts what happened in the low word: a right click, or a left click or Enter.
        WM_TRAY if matches!(lparam.0 as u32 & 0xffff, WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT) => {
            show_menu(None, None);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            with(App::ask_for_relayout);
            LRESULT(0)
        }
        WM_RELAYOUT => {
            let found = icons::rects();
            with(|app| app.relayout(&found));
            LRESULT(0)
        }
        // Alt+F4 ends up here when this window was left in front of the desktop by a menu. It's meant for the desktop,
        // not this program, which has no window to bring it back from. WM_CLOSE stays, for Task Manager.
        WM_SYSCOMMAND if wparam.0 & 0xfff0 == SC_CLOSE as usize => LRESULT(0),
        // Signing out or shutting down: the process ends without coming back to the end of main. Asked first, so that
        // a field still open has the time to close the normal way.
        // The session may end as soon as this returns, before a field could close the normal way, so what it holds is
        // taken here.
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION if wparam.0 != 0 => {
            let draft = prompt::take();
            with(|app| {
                if let Some(text) = draft {
                    app.keep_draft(text);
                }
                app.last_saves();
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ if message != 0 && message == taskbar_created => {
            with(App::recover_cards);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

// Show Desktop lifts the desktop, and the cards with it, which is what keeps them on screen. The desktop sinks back
// afterwards but they don't, so they're pushed down once something else takes over. An event rather than a check:
// this only runs when the foreground window changes.
unsafe extern "system" fn foreground_changed(_hook: HWINEVENTHOOK, _event: u32, window: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    let front = is_desktop(window);
    with(|app| {
        if front {
            app.desktop_in_front = true;
        } else if app.desktop_in_front {
            app.desktop_in_front = false;
            app.send_to_bottom();
        }
    });
}

// Tells Windows this process never needs to hurry: it goes on efficient cores at an efficient clock.
fn ask_for_efficiency() {
    const EXECUTION_SPEED: u32 = 1;
    const IGNORE_TIMER_RESOLUTION: u32 = 4;
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: 1,
        ControlMask: EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION,
        StateMask: EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION,
    };
    unsafe {
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

fn sample_observation(code: u32, day: bool, moment: Now) -> weather::Observation {
    let today = date::days_from_civil(moment.year, moment.month, moment.day);
    let hour = |hour: u32, temperature: f32, code: u32| weather::Hour { hour, temperature, code };
    let date = |ahead: i64, weekday: u32, high: f32, low: f32, code: u32| weather::Day { date: today + ahead, weekday, high, low, code };
    weather::Observation {
        temperature: 22.8,
        code,
        high: 26.6,
        low: 21.2,
        day,
        hours: vec![hour(13, 25.0, 2), hour(14, 26.0, 1), hour(15, 26.0, 3), hour(16, 25.0, 61), hour(17, 24.0, 63), hour(18, 23.0, 3)],
        days: vec![date(0, 6, 26.6, 21.2, 2), date(1, 0, 25.0, 20.0, 61), date(2, 1, 23.0, 18.0, 63), date(3, 2, 22.0, 16.5, 3), date(4, 3, 24.0, 17.0, 0)],
        utc_offset: None,
    }
}

// The cards with nothing out of the ordinary to say, for the checks that draw them without a desktop.
fn plain_data<'a>(settings: &'a Settings, todos: &'a [Todo], weather: WeatherView<'a>) -> Data<'a> {
    Data { settings, todos, todo_start: 0, todos_unsaved: false, todos_unreadable: false, date_refused: false, weather }
}

/// Writes every card at every size to BMP files, to check the drawing without touching the desktop.
fn dump(folder: &str) {
    let mut gfx = Gfx::new().expect("graphics");
    gfx.set_wallpaper(wallpaper::load(2880, 1800));
    gfx.backdrop = Some(gfx::Backdrop { left: 0.5, top: 0.3, screen_width: 1440.0 });
    let moment = now();
    let settings = Settings::default();
    let todos = vec![
        Todo { done: false, text: "繳交期中報告".to_string() },
        Todo { done: false, text: "買牛奶".to_string() },
        Todo { done: false, text: "一個很長很長很長很長很長很長很長很長很長很長很長很長很長很長很長很長的項目".to_string() },
        Todo { done: true, text: "回覆老師的信".to_string() },
    ];
    let scale = 2.0 * zoom();
    let mut write = |name: String, kind: Kind, size: Size, flip: Option<&Flip>, list: &[Todo], sky: Option<&weather::Observation>| {
        let (width, height) = pixels(size, scale);
        let today = date::days_from_civil(moment.year, moment.month, moment.day);
        let view = WeatherView { place: Some("新北"), observation: sky, today, ..Default::default() };
        let data = plain_data(&settings, list, view);
        gfx.paint(width, height, scale, |gfx| cards::draw(gfx, kind, size, moment, &data, flip, None)).expect("paint");
        let rows = gfx.flattened(width, height, [12, 12, 12]);
        let mut file = Vec::with_capacity(54 + rows.len());
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&(54 + rows.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&height.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&24u16.to_le_bytes());
        file.extend_from_slice(&[0; 24]);
        file.extend_from_slice(&rows);
        std::fs::write(format!("{folder}/{name}.bmp"), file).expect("write");
    };
    let sky = sample_observation(2, true, moment);
    for kind in KINDS {
        for size in kind.sizes() {
            write(format!("{kind:?}-{}", size.key()), kind, *size, None, &todos, Some(&sky));
        }
    }
    let many: Vec<Todo> = (1..=14).map(|n| Todo { done: n % 5 == 0, text: format!("第 {n} 件事") }).collect();
    write("Todo-many-l".to_string(), Kind::Todo, Size::Large, None, &many, None);
    write("Todo-many-s".to_string(), Kind::Todo, Size::Small, None, &many, None);
}

// What the process holds right now: private memory, handles, GDI objects, USER objects.
fn holdings() -> (usize, u32, u32, u32) {
    unsafe {
        let process = GetCurrentProcess();
        let mut memory = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        let _ = GetProcessMemoryInfo(process, &mut memory, memory.cb);
        let mut handles = 0;
        let _ = GetProcessHandleCount(process, &mut handles);
        (memory.PagefileUsage, handles, GetGuiResources(process, GR_GDIOBJECTS), GetGuiResources(process, GR_USEROBJECTS))
    }
}

unsafe extern "system" fn soak_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

/// Draws and presents every card over and over, and runs failing requests against a closed local port, logging what
/// the process holds. Anything that grows here is a leak. No traffic leaves the machine.
fn soak(path: &str, rounds: usize) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(soak_procedure),
            hInstance: module.into(),
            lpszClassName: w!("DesktopWidgets.Soak"),
            ..Default::default()
        });
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("DesktopWidgets.Soak"),
            w!("soak"),
            WS_POPUP,
            -4000,
            -4000,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )
        .expect("window");
        let mut gfx = Gfx::new().expect("graphics");
        gfx.set_wallpaper(wallpaper::load(2880, 1800));
        gfx.backdrop = Some(gfx::Backdrop { left: 0.74, top: 0.25, screen_width: 1440.0 });
        let settings = Settings::default();
        let todos = vec![Todo { done: false, text: "測試".to_string() }; 6];
        let moment = now();
        let sky = sample_observation(95, false, moment);
        let mut log = String::from("round,private_kb,handles,gdi,user\n");
        for round in 0..=rounds {
            for kind in KINDS {
                for size in kind.sizes() {
                    let today = date::days_from_civil(moment.year, moment.month, moment.day);
                    let view = WeatherView { place: Some("測試"), observation: Some(&sky), today, ..Default::default() };
                    let data = plain_data(&settings, &todos, view);
                    let flip = Flip { old: ("00".to_string(), "00".to_string()), progress: (round % 10) as f32 / 10.0 };
                    let (width, height) = pixels(*size, 2.0);
                    gfx.paint(width, height, 2.0, |gfx| cards::draw(gfx, kind, *size, moment, &data, Some(&flip), None)).expect("paint");
                    let _ = gfx.present(window, None, width, height);
                }
            }
            if round % 50 == 0 {
                let _ = http::get_insecure("127.0.0.1", 9, "/");
            }
            if round % (rounds / 20).max(1) == 0 {
                let (private, handles, gdi, user) = holdings();
                log.push_str(&format!("{round},{},{handles},{gdi},{user}\n", private / 1024));
            }
        }
        let _ = std::fs::write(path, log);
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if let [_, flag, folder] = arguments.as_slice() {
        if flag == "--dump" {
            dump(folder);
            return;
        }
    }
    if let [_, flag, path] = arguments.as_slice() {
        if flag == "--icons" {
            let lines: Vec<String> = icons::rects().iter().map(|icon| format!("{},{} {}x{}", icon.x, icon.y, icon.w, icon.h)).collect();
            let _ = std::fs::write(path, lines.join("\n"));
            return;
        }
    }
    if let [_, flag, path, rounds] = arguments.as_slice() {
        if flag == "--soak" {
            soak(path, rounds.parse().unwrap_or(1000));
            return;
        }
    }

    unsafe {
        let _instance = CreateMutexW(None, false, w!("Local\\DesktopWidgets.Running"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        ask_for_efficiency();

        let Ok(module) = GetModuleHandleW(None) else { return };
        let arrow = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(card_procedure),
            hInstance: module.into(),
            lpszClassName: w!("DesktopWidgets.Card"),
            hCursor: arrow,
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(hidden_procedure),
            hInstance: module.into(),
            lpszClassName: w!("DesktopWidgets.Hidden"),
            ..Default::default()
        });
        // Top-level and never shown: message-only windows don't get the broadcasts this relies on.
        let Ok(hidden) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("DesktopWidgets.Hidden"),
            w!("DesktopWidgets"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };

        let Ok(gfx) = Gfx::new() else { return };
        let mut settings = store::load_settings();
        let unsaved_settings = bring_up_to_date(&mut settings) && !store::save_settings(&settings);
        ZOOM.store(settings.zoom, Ordering::Relaxed);
        let app = App {
            gfx,
            hidden,
            slots: KINDS.iter().map(|kind| Slot { kind: *kind, window: HWND::default(), key: String::new() }).collect(),
            settings,
            todos: store::load_todos(),
            display_on: true,
            session_locked: false,
            flipping: None,
            drag: None,
            shown_clock: None,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            desktop_in_front: is_desktop(GetForegroundWindow()),
            guide: HWND::default(),
            glide: None,
            editing: None,
            dark: wants_dark(),
            weather: Weather::default(),
            icons: Vec::new(),
            scroll: 0,
            last_toggle: None,
            last_page: None,
            wheel_rest: 0,
            unsaved_settings,
            unsaved_todos: false,
            date_refused: None,
            relayout_pending: false,
            relayout_missed: false,
            title_before: None,
            date_offered: None,
            title_offered: None,
            screen: (0, 0),
            tray: tray_icon(),
        };
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        with(|app| {
            app.load_wallpaper();
            app.guide = create_guide();
            for index in 0..app.slots.len() {
                app.create_card(index);
            }
            // Not pulled in: at sign-in the taskbar and any dock may not have settled yet, and neither have the icons.
            // Not saved either: it's worked out again at every start, and a smaller screen today shouldn't change the
            // layout for good.
            let wanted = app.wanted_positions();
            app.arrange(&wanted, &[], false, false);
            app.refresh(false);
            app.schedule();
            app.check_weather();
        });
        if let Some(icon) = with(|app| app.tray) {
            show_tray(hidden, icon, NIM_ADD);
        }
        register_startup();
        add_to_start_menu();
        let _ = RegisterPowerSettingNotification(HANDLE(hidden.0), &DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        let _ = WTSRegisterSessionNotification(hidden, 0);
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let _hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(foreground_changed), 0, 0, flags);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        // Before main returns: a thread-local would only be dropped during process exit, when the graphics libraries
        // can no longer be released and take the process down with them.
        with(|app| {
            app.last_saves();
            hide_tray(app.hidden);
            let _ = DestroyIcon(app.tray);
            for slot in &app.slots {
                let _ = DestroyWindow(slot.window);
            }
            let _ = DestroyWindow(app.guide);
        });
        APP.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    use super::takes_over;

    #[test]
    fn startup_stays_with_a_program_that_is_still_there() {
        let installed = r#""C:\Programs\DesktopWidgets\desktop-widgets.exe""#;
        let build = r#""D:\DesktopWidgets\target\release\desktop-widgets.exe""#;
        let there = |path: &str| path == r"C:\Programs\DesktopWidgets\desktop-widgets.exe";
        assert!(!takes_over(installed, build, there));
        assert!(!takes_over(installed, installed, there));
        assert!(takes_over(installed, build, |_| false));
        assert!(!takes_over(&installed.to_uppercase(), installed, |_| false));
    }
}
