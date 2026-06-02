use iced_program::{Program, graphics::shell::Notifier};
use objc2::{
    AnyThread, DefinedClass as _, define_class, msg_send, rc::Retained, runtime::NSObject, sel,
};
use objc2_app_kit::NSView;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::proxy;

pub fn setup_vsync<P>(window: &winit::window::Window, proxy: &proxy::Proxy<P::Message>)
where
    P: Program,
{
    let Ok(handle) = window.window_handle() else {
        return;
    };

    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    #[allow(unsafe_code)]
    unsafe {
        let ptr = Retained::retain(handle.ns_view.as_ptr() as *mut NSView);
        let Some(ns_view) = ptr else {
            return;
        };

        let target = LinkTarget::new::<P>(proxy);
        let link = ns_view.displayLinkWithTarget_selector(&target, sel!(vblank:));
        link.setPreferredFrameRateRange(CAFrameRateRange::new(60.0, 180.0, 180.0));

        let raw_link = Retained::into_raw(link);
        let addr = raw_link as usize;

        let _ = std::thread::spawn(move || {
            let link = Retained::from_raw(addr as *mut CADisplayLink).unwrap();
            let run_loop = NSRunLoop::currentRunLoop();
            link.addToRunLoop_forMode(&run_loop, NSRunLoopCommonModes);
            run_loop.run();
        });
    };
}

struct Ivars {
    redraw: Box<dyn Fn() -> ()>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[name = "LinkTarget"]
  #[thread_kind = AnyThread]
  #[ivars = Ivars]
  struct LinkTarget;

  impl LinkTarget {
    #[unsafe(method(vblank:))]
    fn vblank(&self, _link: &CADisplayLink) {
      (self.ivars().redraw)();
    }
  }
);

impl LinkTarget {
    fn new<P>(proxy: &proxy::Proxy<P::Message>) -> Retained<Self>
    where
        P: Program,
    {
        let proxy = proxy.clone();
        let this = Self::alloc().set_ivars(Ivars {
            redraw: Box::new(move || {
                proxy.request_redraw();
            }),
        });
        #[allow(unsafe_code)]
        unsafe {
            msg_send![super(this), init]
        }
    }
}
