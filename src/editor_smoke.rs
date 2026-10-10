//! Windows smoke test for the editor: spawns it inside a real parent window the way a host does,
//! clicks the pad, and checks the click reaches the host as a begin/set/end gesture.
//!
//! Opens a window on the desktop for about a second, so it is `#[ignore]`d by default:
//!
//! ```shell
//! cargo test --release -p ase-project editor_smoke -- --ignored --nocapture
//! # optional: EDITOR_SMOKE_SCREENSHOT=editor.bmp saves what the window drew
//! ```

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nih_plug::prelude::*;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetClientRect, GetWindow, GetWindowRect, PeekMessageW,
    PostMessageW, SetWindowPos, TranslateMessage, GW_CHILD, MSG, PM_REMOVE, SWP_NOMOVE, SWP_NOZORDER,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use crate::editor::{create, EditorShared, HEIGHT, WIDTH};

/// `MK_LBUTTON` lives behind another windows-sys feature; it is just this bit.
const MK_LBUTTON: usize = 0x0001;
use crate::SpatializerParams;

/// Records what the editor asks the host to do.
#[derive(Default)]
struct MockContext {
    events: Mutex<Vec<String>>,
}

impl MockContext {
    fn push(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
}

impl GuiContext for MockContext {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn request_resize(&self) -> bool {
        false
    }

    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        self.push(format!("begin {}", param.name()));
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        self.push(format!("set {} {normalized:.4}", param.name()));
    }

    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        self.push(format!("end {}", param.name()));
    }

    fn get_state(&self) -> PluginState {
        unimplemented!("the editor never reads plugin state")
    }

    fn set_state(&self, _state: PluginState) {
        unimplemented!("the editor never writes plugin state")
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Run the thread's message loop, as the host would, for `duration`.
fn pump(duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        unsafe {
            let mut msg: MSG = zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn point(x: f32, y: f32) -> LPARAM {
    (((y as i32) & 0xffff) << 16 | ((x as i32) & 0xffff)) as LPARAM
}

#[test]
#[ignore = "opens a real window; run with --ignored on a desktop session"]
fn editor_smoke() {
    unsafe {
        // Behave like a per-monitor DPI aware host. It has to be the process setting: with an
        // OpenGL context baseview follows the process awareness, not the thread's.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let class = wide("STATIC");
        let title = wide("Spatializer editor smoke test");
        let parent = CreateWindowExW(
            WS_EX_TOPMOST,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            40,
            40,
            200,
            200,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        );
        assert!(!parent.is_null(), "could not create the parent window");

        // Like a host: report the monitor scale, then size the frame to `size() * scale`.
        let scale = GetDpiForWindow(parent) as f32 / 96.0;
        let params = Arc::new(SpatializerParams::default());
        let context = Arc::new(MockContext::default());
        let editor = create(params, Arc::new(AtomicF32::new(0.25)), Arc::new(EditorShared::default())).unwrap();
        assert!(editor.set_scale_factor(scale));
        let (w, h) = ((WIDTH as f32 * scale) as i32, (HEIGHT as f32 * scale) as i32);
        SetWindowPos(parent, null_mut(), 0, 0, w + 40, h + 80, SWP_NOMOVE | SWP_NOZORDER);

        let handle = editor.spawn(ParentWindowHandle::Win32Hwnd(parent), context.clone());
        pump(Duration::from_millis(1000));

        let child = GetWindow(parent, GW_CHILD);
        assert!(!child.is_null(), "the editor did not create a child window");
        let mut rect: RECT = zeroed();
        GetClientRect(child, &mut rect);
        println!("monitor scale {scale}, editor client area {}x{} px", rect.right, rect.bottom);
        assert_eq!((rect.right, rect.bottom), (w, h), "editor does not fill the size reported to the host");

        if let Ok(path) = std::env::var("EDITOR_SMOKE_SCREENSHOT") {
            capture(child, &path);
            println!("saved {path}");
        }

        // Click on the left edge of the disc: azimuth should become ~90° left. All three messages
        // are queued before pumping: the first move makes baseview call TrackMouseEvent, and since
        // the real cursor is elsewhere Windows answers with WM_MOUSELEAVE, which egui-baseview
        // treats as "pointer gone" and drops any button event after it.
        let (x, y) = (68.0 * scale, 234.0 * scale);
        PostMessageW(child, WM_MOUSEMOVE, 0, point(x, y));
        PostMessageW(child, WM_LBUTTONDOWN, MK_LBUTTON, point(x, y));
        PostMessageW(child, WM_LBUTTONUP, 0, point(x, y));
        pump(Duration::from_millis(300));

        drop(handle);
        pump(Duration::from_millis(50));
        DestroyWindow(parent);

        let events = context.events.lock().unwrap().clone();
        println!("host saw: {events:?}");
        let set = events
            .iter()
            .find_map(|e| e.strip_prefix("set Azimuth "))
            .expect("the click did not set the azimuth");
        let azimuth = set.parse::<f32>().unwrap() * 360.0 - 180.0;
        assert!((70.0..110.0).contains(&azimuth), "expected ~90° left, got {azimuth}°");
        let count = |prefix: &str| events.iter().filter(|e| e.starts_with(prefix)).count();
        assert_eq!(count("begin"), count("end"), "unbalanced automation gestures");
    }
}

/// Save the window's on-screen pixels as a top-down 32-bit BMP.
unsafe fn capture(hwnd: HWND, path: &str) {
    let mut r: RECT = zeroed();
    GetWindowRect(hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let screen = GetDC(null_mut());
    let mem = CreateCompatibleDC(screen);
    let bitmap = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bitmap);
    BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY);

    let mut info: BITMAPINFO = zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = w;
    info.bmiHeader.biHeight = -h;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    GetDIBits(mem, bitmap, 0, h as u32, pixels.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
    SelectObject(mem, old);
    DeleteObject(bitmap);
    DeleteDC(mem);
    ReleaseDC(null_mut(), screen);

    let mut file = Vec::with_capacity(54 + pixels.len());
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&w.to_le_bytes());
    file.extend_from_slice(&(-h).to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&32u16.to_le_bytes());
    file.extend_from_slice(&[0; 24]);
    file.extend_from_slice(&pixels);
    std::fs::write(path, file).expect("write screenshot");
}
