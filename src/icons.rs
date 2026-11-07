//! Where the desktop icons are, so cards can stay off them. Only read when a card is dragged or resized, and never
//! changed: the arrangement is the user's.

use crate::snap::Rect;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IServiceProvider, CLSCTX_LOCAL_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    IEnumIDList, IFolderView, IFolderView2, IShellBrowser, IShellWindows, ShellWindows, CSIDL_DESKTOP, FWF_NOICONS,
    SID_STopLevelBrowser, SVGIO_ALLVIEW, SWC_DESKTOP, SWFO_NEEDDISPATCH,
};
use windows::core::{Interface, Result};

fn read() -> Result<Vec<Rect>> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let location = VARIANT::from(CSIDL_DESKTOP as i32);
        let root = VARIANT::default();
        let mut handle = 0;
        let dispatch = windows.FindWindowSW(&location, &root, SWC_DESKTOP, &mut handle, SWFO_NEEDDISPATCH)?;
        let provider: IServiceProvider = dispatch.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let folder: IFolderView = browser.QueryActiveShellView()?.cast()?;
        let details: IFolderView2 = folder.cast()?;
        if details.GetCurrentFolderFlags()? & FWF_NOICONS.0 as u32 != 0 {
            return Ok(Vec::new()); // "Show desktop icons" is off: there's nothing to keep clear of.
        }
        let mut cell = POINT::default();
        folder.GetSpacing(&mut cell)?;
        // A position is where the picture starts, and the label under it can use the whole width of the cell.
        let (mut mode, mut icon) = (Default::default(), 0);
        details.GetViewModeAndIconSize(&mut mode, &mut icon)?;
        let inset = ((cell.x - icon * crate::primary_dpi() as i32 / 96) / 2).max(0);
        let items: IEnumIDList = folder.Items(SVGIO_ALLVIEW)?;

        let mut icons = Vec::new();
        loop {
            let mut item = [std::ptr::null_mut()];
            let mut fetched = 0;
            if items.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            if let Ok(position) = folder.GetItemPosition(item[0]) {
                icons.push(Rect { x: position.x - inset, y: position.y, w: cell.x, h: cell.y });
            }
            CoTaskMemFree(Some(item[0].cast()));
        }
        Ok(icons)
    }
}

/// None of this is essential, so a failure just means no icons to avoid.
pub fn rects() -> Vec<Rect> {
    read().unwrap_or_default()
}
