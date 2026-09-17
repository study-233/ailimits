//! Read shell geometry on one COM worker; never block the window event loop.
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::*;

type Bounds = (i32, i32, i32, i32);
#[derive(Default)]
struct State {
    target: Option<(isize, Bounds)>,
    measured: Option<(Instant, Vec<(i32, i32)>)>,
}
type Shared = Arc<(Mutex<State>, Condvar)>;
static SHARED: OnceLock<Shared> = OnceLock::new();

pub(crate) fn start(proxy: tao::event_loop::EventLoopProxy<crate::app::UserEvent>) {
    SHARED.get_or_init(|| {
        let shared: Shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::spawn(move || {
            let automation = unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    return;
                }
                let Ok(ui): windows::core::Result<IUIAutomation2> =
                    CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                else {
                    return;
                };
                let _ = ui.SetConnectionTimeout(750);
                let _ = ui.SetTransactionTimeout(750);
                ui
            };
            loop {
                let (lock, wake) = &*worker;
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                while state.target.is_none() {
                    state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                let target = state.target.unwrap();
                drop(state);
                let measured = unsafe { measure(&automation, target.0, target.1) }.ok();
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                if state.target == Some(target) {
                    let changed =
                        state.measured.as_ref().map(|(_, rects)| rects) != measured.as_ref();
                    state.measured = measured.map(|rects| (Instant::now(), rects));
                    if changed {
                        let _ = proxy.send_event(crate::app::UserEvent::TaskbarMoved);
                    }
                }
                // Periodic refresh covers XAML button moves without HWND events.
                // The window thread also expires old snapshots if UIA stalls.
                drop(wake.wait_timeout(state, Duration::from_millis(750)));
            }
        });
        shared
    });
}

pub(crate) fn deactivate() {
    if let Some(shared) = SHARED.get() {
        let mut state = shared.0.lock().unwrap_or_else(|e| e.into_inner());
        state.target = None;
        state.measured = None;
    }
}

pub(crate) fn occupied(hwnd: isize, bounds: Bounds) -> Option<Vec<(i32, i32)>> {
    let shared = SHARED.get()?;
    let mut state = shared.0.lock().unwrap_or_else(|e| e.into_inner());
    if state.target != Some((hwnd, bounds)) {
        state.target = Some((hwnd, bounds));
        state.measured = None;
        shared.1.notify_one();
    }
    state
        .measured
        .as_ref()
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(3))
        .map(|(_, rects)| rects.clone())
}

unsafe fn measure(
    ui: &IUIAutomation2,
    hwnd: isize,
    bounds: Bounds,
) -> windows::core::Result<Vec<(i32, i32)>> {
    let root = ui.ElementFromHandle(HWND(hwnd as _))?;
    let rect = root.CurrentBoundingRectangle()?;
    if (rect.left, rect.top, rect.right, rect.bottom) != bounds {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    let request = ui.CreateCacheRequest()?;
    for property in [
        UIA_ControlTypePropertyId,
        UIA_IsOffscreenPropertyId,
        UIA_BoundingRectanglePropertyId,
    ] {
        request.AddProperty(property)?;
    }
    let elements =
        root.FindAllBuildCache(TreeScope_Descendants, &ui.CreateTrueCondition()?, &request)?;
    let count = elements.Length()?;
    if count > 1024 {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    let mut occupied = Vec::new();
    for i in 0..count {
        let element = elements.GetElement(i)?;
        if element.CachedIsOffscreen()?.as_bool() {
            continue;
        }
        let kind = element.CachedControlType()?;
        // Container bounds span the whole taskbar; leaf controls are obstacles.
        if [
            UIA_PaneControlTypeId,
            UIA_WindowControlTypeId,
            UIA_GroupControlTypeId,
            UIA_ToolBarControlTypeId,
        ]
        .contains(&kind)
        {
            continue;
        }
        let RECT {
            left,
            top,
            right,
            bottom,
        } = element.CachedBoundingRectangle()?;
        if bottom > bounds.1 && top < bounds.3 && right > left && bottom > top {
            occupied.push((left.max(bounds.0), right.min(bounds.2)));
        }
    }
    if occupied.is_empty() {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    occupied.sort_unstable();
    occupied.dedup();
    Ok(occupied)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "reads the interactive Windows taskbar via UI Automation"]
    fn live_taskbar_space() {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            let ui: IUIAutomation2 =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).unwrap();
            let bar =
                super::super::taskbar_slot(crate::config::schema::PanelDisplay::Primary).unwrap();
            let rects = measure(
                &ui,
                bar.hwnd,
                (bar.left, bar.top, bar.right, bar.top + bar.height),
            )
            .unwrap();
            let width = (151.0 * super::super::taskbar_geom::bar_scale(bar.height)) as i32;
            let x = super::super::taskbar_geom::free_panel_x(
                bar.left,
                bar.tray_left,
                width,
                bar.tray_left - width - 10,
                12,
                &rects,
            );
            if let Some(x) = x {
                assert!(rects.iter().all(|&(a, b)| x + width <= a || x >= b));
            }
            println!(
                "taskbar width={} panel width={} safe x={x:?} obstacles={}",
                bar.right - bar.left,
                width,
                rects.len()
            );
        }
    }
}
