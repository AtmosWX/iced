use core::ffi::c_void;
use std::time::Duration;

use iced_program::{Program, graphics::shell::Notifier};
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput},
        Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromWindow},
    },
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::proxy;

pub fn setup_vsync<P>(window: &winit::window::Window, proxy: &proxy::Proxy<P::Message>)
where
    P: Program,
{
    let Ok(handle) = window.window_handle() else {
        return;
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = handle.hwnd.get();
    let proxy = proxy.clone();

    let _ = std::thread::spawn(move || {
        #[allow(unsafe_code)]
        unsafe {
            let hwnd = HWND(hwnd as *mut c_void);

            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return;
            };

            let mut current: Option<(HMONITOR, IDXGIOutput)> = None;

            loop {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

                if current.as_ref().map(|(m, _)| *m) != Some(monitor) {
                    current = find_output(&factory, monitor).map(|o| (monitor, o));
                }

                let Some((_, output)) = current.as_ref() else {
                    std::thread::sleep(Duration::from_millis(16));
                    proxy.request_redraw();
                    continue;
                };

                if output.WaitForVBlank().is_err() {
                    current = None;
                    continue;
                }

                proxy.request_redraw();
            }
        }
    });
}

#[allow(unsafe_code)]
fn find_output(factory: &IDXGIFactory1, target: HMONITOR) -> Option<IDXGIOutput> {
    unsafe {
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                if let Ok(desc) = output.GetDesc() {
                    if desc.Monitor == target {
                        return Some(output);
                    }
                }
                o += 1;
            }
            a += 1;
        }
        None
    }
}
