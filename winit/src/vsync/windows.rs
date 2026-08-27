use core::ffi::c_void;
use std::time::{Duration, Instant};

use iced_program::{Program, graphics::shell::Notifier};
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput},
        Gdi::{
            DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, HMONITOR,
            MONITOR_DEFAULTTONEAREST, MonitorFromWindow,
        },
    },
};
use windows::core::PCWSTR;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::proxy;

const DEFAULT_INTERVAL: Duration = Duration::from_micros(16_667);

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

            let mut current: Option<(HMONITOR, IDXGIOutput, Duration)> = None;
            let mut last = Instant::now();

            loop {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

                if current.as_ref().map(|(m, _, _)| *m) != Some(monitor) {
                    current = find_output(&factory, monitor).map(|output| {
                        let interval = refresh_interval(&output);
                        (monitor, output, interval)
                    });
                }

                let Some((_, output, interval)) = current.as_ref() else {
                    std::thread::sleep(DEFAULT_INTERVAL);
                    last = Instant::now();
                    proxy.request_redraw();
                    continue;
                };

                let interval = *interval;

                if output.WaitForVBlank().is_err() {
                    current = None;
                    std::thread::sleep(interval);
                    continue;
                }

                let elapsed = last.elapsed();

                if elapsed < interval / 2 {
                    std::thread::sleep(interval - elapsed);
                }

                last = Instant::now();

                proxy.request_redraw();
            }
        }
    });
}

#[allow(unsafe_code)]
fn refresh_interval(output: &IDXGIOutput) -> Duration {
    unsafe {
        let Ok(desc) = output.GetDesc() else {
            return DEFAULT_INTERVAL;
        };

        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };

        if !EnumDisplaySettingsW(
            PCWSTR(desc.DeviceName.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut mode,
        )
        .as_bool()
        {
            return DEFAULT_INTERVAL;
        }

        match mode.dmDisplayFrequency {
            // 0 and 1 are documented as meaning "the hardware default", not a rate.
            0 | 1 => DEFAULT_INTERVAL,
            hz => Duration::from_secs_f64(1.0 / f64::from(hz)),
        }
    }
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
