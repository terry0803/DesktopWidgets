//! Typing on a card. Layered windows can't host controls, so a borderless edit box sits exactly over the spot, in the
//! card's own font and colours. It's Windows' edit control underneath, so Chinese input methods just work.

use std::cell::Cell;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateFontW, CreatePatternBrush, DeleteObject, SetBkMode, SetTextColor, UpdateWindow,
    ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HBITMAP, HBRUSH, HDC,
    HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::Ime::{
    ImmGetCompositionStringW, ImmGetContext, ImmNotifyIME, ImmReleaseContext, CPS_CANCEL, GCS_COMPSTR, NI_COMPOSITIONSTR,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW,
    GetWindowTextLengthW, GetWindowTextW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetForegroundWindow, ShowWindow, TranslateMessage, MSG, SW_SHOWNA, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE,
    WM_CLOSE, WM_CTLCOLOREDIT, WM_KEYDOWN, WM_KEYUP, WM_NULL, WM_QUIT, WM_SETFONT, WNDCLASSW, WS_CHILD, WS_EX_TOOLWINDOW,
    WS_POPUP, WS_VISIBLE,
};
use windows::core::{w, HSTRING, PCWSTR};

const EM_SETSEL: u32 = 0x00B1;
const EM_SETMARGINS: u32 = 0x00D3;
const EM_REPLACESEL: u32 = 0x00C2;
const EC_LEFTMARGIN_RIGHTMARGIN: usize = 3;
const ES_AUTOHSCROLL: u32 = 0x0080;
const KEY_WAS_DOWN: isize = 1 << 30;

/// Where to type, in screen pixels, and how it should look.
pub struct Field {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub font_size: f32,
    pub semibold: bool,
    pub text: [u8; 3],
    /// What's under the box, as BGRA rows: it shows through, so no edge can be seen against the card.
    pub backdrop: Vec<u8>,
}

/// How typing ended. The text may be empty.
#[derive(PartialEq, Debug)]
pub enum Answer {
    /// Enter.
    Entered(String),
    /// A click somewhere else, which keeps what was typed.
    Left(String),
    /// Esc, or no box could be opened.
    Cancelled,
}

/// What a field takes over from, which decides where the keyboard goes when it closes.
pub enum After {
    /// Whatever is in front right now.
    Foreground,
    /// This window: for when the program is in front only because of the menu that led here.
    Window(HWND),
    /// A field that's still showing, which goes as soon as this one is up.
    Field(Open),
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Closed,
    Open,
    Entered,
    Left,
    Cancelled,
}

thread_local! {
    static STATE: Cell<State> = const { Cell::new(State::Closed) };
    static BRUSH: Cell<HBRUSH> = const { Cell::new(HBRUSH(std::ptr::null_mut())) };
    static TEXT: Cell<COLORREF> = const { Cell::new(COLORREF(0)) };
    // The newest field and its edit control. While one field replaces another both exist for a moment, and only
    // this one losing the keyboard means anything.
    static WINDOW: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static EDIT: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static OPENED: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// A field on screen. The caller keeps it until the card under it shows the result, so the text never blinks away.
pub struct Open {
    window: HWND,
    font: HFONT,
    brush: HBRUSH,
    bitmap: HBITMAP,
    back_to: HWND,
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            if !self.window.is_invalid() {
                // Closing would otherwise hand the foreground to the card, which can't use it. Not back to the
                // desktop though: activating it ends Show Desktop and brings every window back.
                if GetForegroundWindow() == self.window && !crate::is_desktop(self.back_to) {
                    let _ = SetForegroundWindow(self.back_to);
                }
                let _ = DestroyWindow(self.window);
            }
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.brush.into());
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

fn colorref([r, g, b]: [u8; 3]) -> COLORREF {
    COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16)
}

pub fn is_open() -> bool {
    STATE.get() == State::Open
}

/// True right after a field opened: the second click of a double click isn't a click away from it.
pub fn just_opened() -> bool {
    let patience = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }));
    OPENED.get().is_some_and(|opened| opened.elapsed() < patience)
}

// What an input method is still composing isn't in the control's text, and would go with the window. Bopomofo finishes
// the word itself when a card is clicked, but not when the click is on another program. So what's left is read out and
// typed in from here, and the composition dropped, which also leaves the method ready for the next field.
fn keep_composition() {
    unsafe {
        let edit = EDIT.get();
        let context = ImmGetContext(edit);
        if context.is_invalid() {
            return;
        }
        let size = ImmGetCompositionStringW(context, GCS_COMPSTR, None, 0);
        if size > 0 {
            let mut units = vec![0u16; size as usize / 2 + 1];
            let read = ImmGetCompositionStringW(context, GCS_COMPSTR, Some(units.as_mut_ptr().cast()), size as u32);
            let _ = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
            if read > 0 {
                units.truncate(read as usize / 2);
                units.push(0);
                SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(0)), Some(LPARAM(units.as_ptr() as isize)));
            }
        }
        let _ = ImmReleaseContext(edit, context);
    }
}

/// What's in the field, for a session that's ending, which won't wait for the field to close the normal way. The
/// field counts as cancelled after this: what it held is the caller's to keep.
pub fn take() -> Option<String> {
    // Left: closed by a click away a moment ago, with the loop below yet to hand the text over.
    match STATE.get() {
        State::Open => keep_composition(),
        State::Left | State::Entered => {}
        State::Closed | State::Cancelled => return None,
    }
    let text = text_of(EDIT.get());
    STATE.set(State::Cancelled);
    let _ = unsafe { PostMessageW(None, WM_NULL, WPARAM(0), LPARAM(0)) };
    Some(text)
}

/// Keeps what was typed and closes, as clicking elsewhere does.
pub fn finish() {
    if is_open() {
        keep_composition();
        STATE.set(State::Left);
        // This can run inside a message sent to us, and the loop below only wakes up for a posted one.
        let _ = unsafe { PostMessageW(None, WM_NULL, WPARAM(0), LPARAM(0)) };
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_CTLCOLOREDIT => {
            let dc = HDC(wparam.0 as *mut _);
            SetTextColor(dc, TEXT.get());
            SetBkMode(dc, TRANSPARENT);
            LRESULT(BRUSH.get().0 as isize)
        }
        WM_ACTIVATE if wparam.0 & 0xffff == 0 => {
            if window == WINDOW.get() {
                finish();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if window == WINDOW.get() {
                finish();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

/// Waits for a line of text. The field stays up until the `Open` that comes back is dropped.
pub fn ask(field: &Field, initial: &str, after: After) -> (Answer, Option<Open>) {
    if STATE.get() != State::Closed {
        return (Answer::Cancelled, None);
    }
    STATE.set(State::Open);
    let result = show(field, initial, after);
    STATE.set(State::Closed);
    result
}

fn text_of(edit: HWND) -> String {
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
        let length = GetWindowTextW(edit, &mut buffer);
        String::from_utf16_lossy(&buffer[..length as usize]).trim().to_string()
    }
}

fn show(field: &Field, initial: &str, after: After) -> (Answer, Option<Open>) {
    let (back_to, replaced) = match after {
        After::Foreground => (unsafe { GetForegroundWindow() }, None),
        After::Window(window) => (window, None),
        After::Field(open) => (open.back_to, Some(open)),
    };
    let Some(open) = build(field, initial, back_to) else {
        return (Answer::Cancelled, replaced);
    };
    let edit = EDIT.get();
    unsafe {
        // Painted before anything else: taking the foreground loads the input method, which is a noticeable wait.
        let _ = ShowWindow(open.window, SW_SHOWNA);
        let _ = UpdateWindow(open.window);
        let _ = UpdateWindow(edit);
        let in_front = SetForegroundWindow(open.window).as_bool();
        drop(replaced);
        if !in_front {
            // Windows refused. It would never lose a focus it never got, so nothing could close it.
            return (Answer::Cancelled, Some(open));
        }
        let _ = SetFocus(Some(edit));
        OPENED.set(Some(Instant::now()));

        let mut pressed = None;
        let mut message = MSG::default();
        while is_open() {
            let result = GetMessageW(&mut message, None, 0, 0);
            if result.0 <= 0 {
                // The program is closing, by its menu or to be updated: what was typed is kept, as a click away keeps it.
                if message.message == WM_QUIT {
                    keep_composition();
                    STATE.set(State::Left);
                    PostQuitMessage(message.wParam.0 as i32);
                } else {
                    STATE.set(State::Cancelled);
                }
                break;
            }
            // These count when they come back up, and only after going down here. Any sooner, and a key held a moment
            // too long repeats into whatever is next: the next field, or the program in front.
            let key = message.wParam.0;
            let enter = key == usize::from(VK_RETURN.0);
            if message.message == WM_KEYDOWN && (enter || key == usize::from(VK_ESCAPE.0)) {
                if message.lParam.0 & KEY_WAS_DOWN == 0 {
                    pressed = Some(key);
                }
                continue;
            }
            if message.message == WM_KEYUP && pressed == Some(key) {
                STATE.set(if enter { State::Entered } else { State::Cancelled });
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let answer = match STATE.get() {
        State::Entered => Answer::Entered(text_of(edit)),
        State::Left => Answer::Left(text_of(edit)),
        _ => Answer::Cancelled,
    };
    (answer, Some(open))
}

// Everything is in place before anything shows, so the box never appears empty.
fn build(field: &Field, initial: &str, back_to: HWND) -> Option<Open> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = w!("DesktopWidgets.Field");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        }); // Fails harmlessly from the second time on.

        TEXT.set(colorref(field.text));
        let bitmap = CreateBitmap(field.width, field.height, 1, 32, Some(field.backdrop.as_ptr().cast()));
        let brush = CreatePatternBrush(bitmap);
        BRUSH.set(brush);
        let family = w!("Noto Sans TC");
        let font: HFONT = CreateFontW(
            -(field.font_size.round() as i32),
            0,
            0,
            0,
            if field.semibold { FW_SEMIBOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY, // The card's text is grayscale too, not ClearType.
            FF_DONTCARE.0 as u32,
            family,
        );
        let mut open = Open { window: HWND::default(), font, brush, bitmap, back_to };

        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            None,
            WS_POPUP,
            field.x,
            field.y,
            field.width,
            field.height,
            None, // Owned by a card, activating it would lift all the cards above other apps.
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        open.window = window;
        let initial = HSTRING::from(initial);
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR(initial.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL),
            0,
            0,
            field.width,
            field.height,
            Some(window),
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        SendMessageW(edit, EM_SETMARGINS, Some(WPARAM(EC_LEFTMARGIN_RIGHTMARGIN)), Some(LPARAM(0)));
        // The caret at the end rather than everything selected: a selection is in the system's accent colour, and
        // the cards stay monochrome.
        let end = GetWindowTextLengthW(edit) as usize;
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
        WINDOW.set(window);
        EDIT.set(edit);
        Some(open)
    }
}
