//! This PC's master's tray icon (migration M4c): shown whenever the master runs, so it is never a hidden process.
//! "Open Qanuvi" (or a click on the icon) shows the app's window, starting the app when none runs; "Quit Qanuvi"
//! closes the windows and stops the master. The icon needs the main thread (macOS asks so), so `serve` takes the
//! connections on another thread while this runs. Part of the `qanuvi-master` program only, so the library and
//! its tests never link the tray.

use qanuvi_lib::master::Master;
use std::sync::Arc;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

enum Ev {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
}

/// A tray needs a desktop to show on: on Linux, a display (a master opened over SSH has none).
pub fn can_show() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    } else {
        true
    }
}

fn tooltip() -> &'static str {
    if qanuvi_lib::data_location::from_source() {
        "Qanuvi (test build)"
    } else {
        "Qanuvi"
    }
}

/// The app's own icon, decoded for the tray.
fn icon() -> Option<tray_icon::Icon> {
    let bytes: &[u8] = include_bytes!("../icons/32x32.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    tray_icon::Icon::from_rgba(rgba, info.width, info.height).ok()
}

/// Shows the tray icon and answers it until the master ends.
pub fn run(master: Arc<Master>) -> ! {
    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    #[cfg(target_os = "macos")]
    let mut event_loop = event_loop;
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // A tray icon only: no Dock icon for the master.
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(Ev::Menu(e));
    }));
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(Ev::Tray(e));
    }));
    let open = MenuItem::new("Open Qanuvi", true, None);
    let quit = MenuItem::new("Quit Qanuvi", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[&open, &quit]);
    let mut tray = None;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // Made once the loop runs (Linux needs it so).
            Event::NewEvents(StartCause::Init) => {
                let mut b = TrayIconBuilder::new()
                    .with_menu(Box::new(menu.clone()))
                    .with_menu_on_left_click(false)
                    .with_tooltip(tooltip());
                if let Some(i) = icon() {
                    b = b.with_icon(i);
                }
                match b.build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => eprintln!("The tray icon could not be shown: {e}"),
                }
            }
            Event::UserEvent(Ev::Menu(e)) if e.id == *open.id() => {
                let m = master.clone();
                std::thread::spawn(move || m.open_app());
            }
            Event::UserEvent(Ev::Menu(e)) if e.id == *quit.id() => {
                let m = master.clone();
                std::thread::spawn(move || m.quit_all());
            }
            Event::UserEvent(Ev::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            })) => {
                let m = master.clone();
                std::thread::spawn(move || m.open_app());
            }
            _ => {}
        }
        // Kept while the loop runs: dropping it removes the icon.
        let _ = &tray;
    })
}
