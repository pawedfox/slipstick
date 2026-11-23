//! The window shell.
//!
//! A frameless, always-on-top window, by default at the bottom-right of the
//! main screen's work area. It can be dragged, and remembers where it was
//! left. A global hotkey summons or dismisses it, and Esc or the minimise
//! button hides it to the tray.
//!
//! The OS window never changes size. It is a fixed, fully transparent
//! rectangle, and the visible shell is drawn at its bottom edge. Resizing a
//! window from its top edge makes the web content flicker, so instead the
//! window is clipped to the part that is in use (see `clip`): everything
//! above the shell is not part of the window at all, and clicks there reach
//! whatever is underneath.

pub mod calc;
pub mod claude_code;
pub mod solver;
mod settings;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, RunEvent, WebviewWindow,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use claude_code::ClaudeCode;
use settings::{Correction, Model, Route, Settings, Theme};

/// Passed by Windows when it starts the app at sign-in ("Start with
/// Windows"), so the window stays out of the way until it is summoned.
const START_HIDDEN: &str = "--hidden";

/// Width of the visible shell plus the transparent margin on both sides.
/// The margin must match `--margin` in styles.css and SHELL_MARGIN in main.ts.
const SHELL_WIDTH: f64 = 420.0;
const SHELL_MARGIN: f64 = 12.0;
const WINDOW_WIDTH: f64 = SHELL_WIDTH + SHELL_MARGIN * 2.0;

/// Fixed height of the OS window. Must match WINDOW_HEIGHT in main.ts and the
/// window height in tauri.conf.json.
const WINDOW_HEIGHT: f64 = 660.0;

/// Height in use before the web content has reported its own.
const INITIAL_VISIBLE_HEIGHT: f64 = 110.0;

struct AppState {
    /// Whether the global hotkey is registered. It is not when another
    /// program already holds the combination.
    hotkey_registered: AtomicBool,
    settings: Mutex<Settings>,
    /// Settings have changed since they were last written to disk.
    unsaved: AtomicBool,
    /// The user's installed Claude Code, once it has been found. It may be
    /// installed after the app has started, so it is looked for again
    /// whenever it is needed (`look_for_claude`).
    claude: OnceLock<Arc<ClaudeCode>>,
}

impl AppState {
    /// Claude Code, but only if the user has agreed to use it.
    fn connected_claude(&self) -> Option<Arc<ClaudeCode>> {
        let connected = self.settings.lock().is_ok_and(|settings| settings.claude == Route::ClaudeCode);
        self.claude.get().cloned().filter(|_| connected)
    }

    /// The significant figures the user has chosen.
    fn figures(&self) -> u8 {
        self.settings.lock().map_or(calc::DEFAULT_SIG_FIGS, |settings| settings.figures)
    }

    /// The shortcut in the settings, if it can be read as one.
    fn hotkey(&self) -> Option<Shortcut> {
        self.settings.lock().ok().and_then(|settings| settings.hotkey.parse().ok())
    }
}

/// How long Claude Code is kept ready after the window is hidden.
const REST_AFTER: Duration = Duration::from_secs(90);

#[derive(Serialize)]
struct ClaudeStatus {
    /// Claude Code is installed on this PC.
    found: bool,
    /// The user has agreed to use it.
    connected: bool,
}

/// The settings as the window shows them.
#[derive(Serialize)]
struct SettingsView {
    /// The shortcut as people write it: `Ctrl+Alt+C`.
    hotkey: String,
    /// False when another program holds the shortcut, so it does nothing.
    hotkey_registered: bool,
    start_with_windows: bool,
    theme: Theme,
    figures: u8,
    correction: Correction,
    model: Model,
}

/// One change made in the settings screen.
#[derive(Deserialize)]
#[serde(tag = "name", content = "value", rename_all = "snake_case")]
enum Change {
    /// Modifiers and a key code joined with `+`, as in `Ctrl+Alt+KeyC`.
    Hotkey(String),
    StartWithWindows(bool),
    Theme(Theme),
    Figures(u8),
    Correction(Correction),
    Model(Model),
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

// ---- where the window sits --------------------------------------------------

/// The default place: bottom-right corner of the main screen's work area, so
/// the shell sits just above the taskbar.
fn default_position(window: &WebviewWindow) -> Option<PhysicalPosition<i32>> {
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width = (WINDOW_WIDTH * scale).round() as i32;
    let height = (WINDOW_HEIGHT * scale).round() as i32;
    Some(PhysicalPosition::new(
        area.position.x + area.size.width as i32 - width,
        area.position.y + area.size.height as i32 - height,
    ))
}

/// Whether enough of the shell would be on a screen to see and grab it. The
/// shell is at the bottom of the window, so only that strip is checked.
fn on_screen(window: &WebviewWindow, position: PhysicalPosition<i32>) -> bool {
    let (Ok(monitors), Ok(scale)) = (window.available_monitors(), window.scale_factor()) else {
        return false;
    };
    let width = (WINDOW_WIDTH * scale) as i32;
    let bottom = position.y + (WINDOW_HEIGHT * scale) as i32;
    let top = bottom - (100.0 * scale) as i32;

    monitors.iter().any(|monitor| {
        let area = monitor.work_area();
        let (left, right) = (area.position.x, area.position.x + area.size.width as i32);
        let (up, down) = (area.position.y, area.position.y + area.size.height as i32);
        let across = (position.x + width).min(right) - position.x.max(left);
        let along = bottom.min(down) - top.max(up);
        across >= (120.0 * scale) as i32 && along >= (50.0 * scale) as i32
    })
}

/// Put the window where the user left it, or in the default corner if they
/// never moved it or that place is no longer on any screen.
fn place(app: &AppHandle, window: &WebviewWindow) {
    let _ = window.set_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));

    let remembered = app
        .state::<AppState>()
        .settings
        .lock()
        .ok()
        .and_then(|settings| settings.window_position)
        .map(|(x, y)| PhysicalPosition::new(x, y))
        .filter(|&position| on_screen(window, position));

    if let Some(position) = remembered.or_else(|| default_position(window))
        && window.outer_position().ok() != Some(position)
    {
        let _ = window.set_position(position);
    }
}

/// Called whenever the window moves. A position other than the default
/// corner is one the user chose, and is remembered.
fn remember_position(app: &AppHandle, position: PhysicalPosition<i32>) {
    let Some(window) = main_window(app) else { return };
    // Windows reports a far-off position for windows that are not on screen.
    if position.x <= -30_000 || position.y <= -30_000 {
        return;
    }
    let chosen = (Some(position) != default_position(&window)).then_some((position.x, position.y));

    let state = app.state::<AppState>();
    if let Ok(mut settings) = state.settings.lock()
        && settings.window_position != chosen
    {
        settings.window_position = chosen;
        state.unsaved.store(true, Ordering::Relaxed);
    }
}

fn save_settings(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.unsaved.swap(false, Ordering::Relaxed)
        && let Ok(settings) = state.settings.lock()
    {
        settings::save(app, &settings);
    }
}

fn reset_position(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut settings) = state.settings.lock() {
        settings.window_position = None;
        state.unsaved.store(true, Ordering::Relaxed);
    }
    if let Some(window) = main_window(app) {
        place(app, &window);
    }
    save_settings(app);
    summon(app);
}

/// Clip the window to its bottom `visible_height` logical pixels.
#[cfg(windows)]
fn clip(window: &WebviewWindow, visible_height: f64) -> tauri::Result<()> {
    use std::ffi::c_void;

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> *mut c_void;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowRgn(hwnd: *mut c_void, region: *mut c_void, redraw: i32) -> i32;
    }

    let scale = window.scale_factor()?;
    let size = window.outer_size()?;
    let visible = (visible_height.clamp(0.0, WINDOW_HEIGHT) * scale).round() as i32;
    let (width, height) = (size.width as i32, size.height as i32);
    let top = (height - visible).max(0);

    // The handle is passed as an integer so the closure can cross threads;
    // the call itself must happen on the thread that owns the window.
    let hwnd = window.hwnd()?.0 as isize;
    window.run_on_main_thread(move || unsafe {
        let region = CreateRectRgn(0, top, width, height);
        // On success the system owns the region; it must not be freed here.
        SetWindowRgn(hwnd as *mut c_void, region, 1);
    })
}

#[cfg(not(windows))]
fn clip(_window: &WebviewWindow, _visible_height: f64) -> tauri::Result<()> {
    Ok(())
}

/// Give the window its large icon, which is the one the taskbar shows.
///
/// Tauri sets only the small icon. With no large one, the taskbar falls back
/// to what Windows has cached for the program file, and that cache can hold
/// a blank icon after the program has been replaced by an update.
#[cfg(windows)]
fn set_taskbar_icon(window: &WebviewWindow) -> tauri::Result<()> {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadImageW(
            instance: *mut c_void,
            name: *const u16,
            kind: u32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> *mut c_void;
        fn SendMessageW(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }

    const IMAGE_ICON: u32 = 1;
    const LR_SHARED: u32 = 0x8000;
    const SM_CXICON: i32 = 11;
    const SM_CYICON: i32 = 12;
    const WM_SETICON: u32 = 0x0080;
    const ICON_BIG: usize = 1;
    // The build embeds the app icon in the program under this resource id.
    let resource = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID as usize;

    let hwnd = window.hwnd()?.0 as isize;
    window.run_on_main_thread(move || unsafe {
        let icon = LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            resource as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXICON),
            GetSystemMetrics(SM_CYICON),
            LR_SHARED,
        );
        if !icon.is_null() {
            SendMessageW(hwnd as *mut c_void, WM_SETICON, ICON_BIG, icon as isize);
        }
    })
}

#[cfg(not(windows))]
fn set_taskbar_icon(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

// ---- showing and hiding -----------------------------------------------------

fn summon(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    if !window.is_visible().unwrap_or(false) {
        // Screens can change while hidden (resolution, taskbar, unplugging).
        place(app, &window);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    let _ = window.emit("summoned", ());
}

fn dismiss(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        let _ = window.hide();
    }
    save_settings(app);

    // Do not leave Claude Code running behind a hidden window for long.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REST_AFTER);
        let hidden = main_window(&app).is_none_or(|window| !window.is_visible().unwrap_or(false));
        if hidden && let Some(claude) = app.state::<AppState>().claude.get() {
            claude.rest();
        }
    });
}

/// The hotkey summons the window, or dismisses it if it already has focus.
fn toggle(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    let visible = window.is_visible().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    if visible && focused {
        dismiss(app);
    } else {
        summon(app);
    }
}

// ---- commands the window calls ----------------------------------------------

/// Called by the web content whenever the height it is using changes.
#[tauri::command]
fn set_visible_height(window: WebviewWindow, height: f64) -> Result<(), String> {
    clip(&window, height).map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    dismiss(&app);
}

/// The instant path. Returns nothing unless the input is plain maths.
#[tauri::command]
fn evaluate(state: tauri::State<'_, AppState>, input: String) -> Option<calc::Answer> {
    calc::with_sig_figs(state.figures(), || calc::evaluate(&input))
}

/// Why the instant path has no answer for this input, in one plain line,
/// where the reason is certain. Asked for when Enter is pressed.
#[tauri::command]
fn explain(input: String) -> Option<String> {
    calc::why_not(&input)
}

/// Looks for the user's Claude Code, unless it has been found already.
/// Finding it does not use it: that waits until they agree.
fn look_for_claude(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.claude.get().is_some() {
        return;
    }
    let found = claude_code::find_program().and_then(|program| {
        let storage = app.path().app_cache_dir().ok()?;
        let (file, folder) = claude_code::prepare(&storage, solver::INSTRUCTIONS)?;
        Some(Arc::new(ClaudeCode::new(program, file, folder)))
    });
    if let Some(claude) = found {
        let model = state.settings.lock().map_or(Model::default(), |settings| settings.model);
        let (name, effort) = model.as_claude_code();
        claude.set_model(name, effort);
        let _ = state.claude.set(claude);
    }
}

#[tauri::command]
fn claude_status(app: AppHandle, state: tauri::State<'_, AppState>) -> ClaudeStatus {
    look_for_claude(&app);
    ClaudeStatus { found: state.claude.get().is_some(), connected: state.connected_claude().is_some() }
}

/// The user has agreed to use their Claude Code for problems that need it.
#[tauri::command]
fn connect_claude(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    look_for_claude(&app);
    let Some(claude) = state.claude.get() else {
        return Err("Claude Code was not found on this PC. Install it and sign in, then try again.".into());
    };
    if let Ok(mut settings) = state.settings.lock() {
        settings.claude = Route::ClaudeCode;
        state.unsaved.store(true, Ordering::Relaxed);
    }
    save_settings(&app);
    claude.warm();
    Ok(())
}

/// Stops using Claude Code. Problems that need Claude get the offer again.
#[tauri::command]
fn disconnect_claude(app: AppHandle) {
    forget_claude(&app);
}

fn forget_claude(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut settings) = state.settings.lock() {
        settings.claude = Route::None;
        state.unsaved.store(true, Ordering::Relaxed);
    }
    save_settings(app);
    if let Some(claude) = state.claude.get() {
        claude.cancel();
        claude.rest();
    }
    let _ = app.emit("claude-changed", ());
}

/// Gets Claude Code started ahead of the question, while the user is still
/// typing. Does nothing unless the user has connected it.
#[tauri::command]
fn warm_claude(state: tauri::State<'_, AppState>) {
    if let Some(claude) = state.connected_claude() {
        std::thread::spawn(move || claude.warm());
    }
}

/// Asks Claude a problem and runs its reply through the engine.
#[tauri::command]
async fn ask_claude(
    app: AppHandle,
    input: String,
    images: Vec<claude_code::Image>,
) -> Result<solver::Solved, String> {
    let (claude, figures) = {
        let state = app.state::<AppState>();
        (state.connected_claude(), state.figures())
    };
    let Some(claude) = claude else {
        return Err("Claude is not connected.".into());
    };
    tauri::async_runtime::spawn_blocking(move || {
        let solved = calc::with_sig_figs(figures, || solver::ask_and_solve(&claude, &input, &images));
        // Have the next one ready before it is needed.
        claude.warm();
        solved
    })
    .await
    .map_err(|_| "Something went wrong while asking Claude.".to_string())?
}

#[tauri::command]
fn cancel_claude(state: tauri::State<'_, AppState>) {
    if let Some(claude) = state.claude.get() {
        claude.cancel();
    }
}

// ---- settings ---------------------------------------------------------------

fn settings_as_shown(app: &AppHandle) -> SettingsView {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().map(|settings| settings.clone()).unwrap_or_default();
    SettingsView {
        hotkey: settings::hotkey_label(&settings.hotkey),
        hotkey_registered: state.hotkey_registered.load(Ordering::Relaxed),
        // Windows keeps this one itself; ask it each time.
        start_with_windows: app.autolaunch().is_enabled().unwrap_or(false),
        theme: settings.theme,
        figures: settings.figures,
        correction: settings.correction,
        model: settings.model,
    }
}

#[tauri::command]
fn settings_view(app: AppHandle) -> SettingsView {
    settings_as_shown(&app)
}

const NEEDS_TWO: &str = "A shortcut needs two of Ctrl, Alt and Shift, such as Ctrl+Alt+C.";

/// Replaces the shortcut that summons the window. If the new one cannot be
/// had, the old one is put back and stays in the settings.
fn change_hotkey(app: &AppHandle, keys: &str) -> Result<(), String> {
    let new: Shortcut = keys.parse().map_err(|_| "That key cannot be used in a shortcut.".to_string())?;
    // A global shortcut takes its keys away from every other program, so one
    // modifier is not enough: Ctrl+C here would break copying everywhere.
    let held = [Modifiers::CONTROL, Modifiers::ALT, Modifiers::SHIFT, Modifiers::SUPER]
        .into_iter()
        .filter(|modifier| new.mods.contains(*modifier))
        .count();
    if held < 2 || !new.mods.intersects(Modifiers::CONTROL | Modifiers::ALT) {
        return Err(NEEDS_TWO.into());
    }

    let state = app.state::<AppState>();
    let shortcuts = app.global_shortcut();
    let old = state.hotkey();
    if let Some(old) = old {
        // Fails harmlessly if it is not registered at the moment.
        let _ = shortcuts.unregister(old);
    }
    if shortcuts.register(new).is_err() {
        let restored = old.is_some_and(|old| shortcuts.register(old).is_ok());
        state.hotkey_registered.store(restored, Ordering::Relaxed);
        return Err("That shortcut is in use by another program.".into());
    }

    state.hotkey_registered.store(true, Ordering::Relaxed);
    if let Ok(mut settings) = state.settings.lock() {
        settings.hotkey = keys.to_string();
        state.unsaved.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Switches the shortcut off while the settings screen waits for a new one
/// to be pressed, and back on afterwards. Otherwise pressing the current
/// shortcut there would hide the window.
#[tauri::command]
fn pause_hotkey(app: AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    let Some(hotkey) = state.hotkey() else { return };
    let shortcuts = app.global_shortcut();
    if paused {
        let _ = shortcuts.unregister(hotkey);
    } else if !shortcuts.is_registered(hotkey) {
        let registered = shortcuts.register(hotkey).is_ok();
        state.hotkey_registered.store(registered, Ordering::Relaxed);
    }
}

/// Makes one change from the settings screen and returns the settings as
/// they now stand.
#[tauri::command]
fn change_setting(app: AppHandle, change: Change) -> Result<SettingsView, String> {
    let state = app.state::<AppState>();
    match change {
        Change::Hotkey(keys) => change_hotkey(&app, &keys)?,
        Change::StartWithWindows(on) => {
            let windows = app.autolaunch();
            let done = if on { windows.enable() } else { windows.disable() };
            done.map_err(|_| "Windows would not change that setting.".to_string())?;
        }
        Change::Figures(figures) if !calc::SIG_FIGS_RANGE.contains(&figures) => {
            return Err("Significant figures can be 3 to 6.".into());
        }
        other => {
            if let Ok(mut settings) = state.settings.lock() {
                match other {
                    Change::Theme(theme) => settings.theme = theme,
                    Change::Figures(figures) => settings.figures = figures,
                    Change::Correction(correction) => settings.correction = correction,
                    Change::Model(model) => settings.model = model,
                    Change::Hotkey(_) | Change::StartWithWindows(_) => {}
                }
                state.unsaved.store(true, Ordering::Relaxed);
            }
            if let (Change::Model(model), Some(claude)) = (&other, state.claude.get()) {
                let (name, effort) = model.as_claude_code();
                claude.set_model(name, effort);
                // Have one ready with the new model, as after connecting.
                if let Some(claude) = state.connected_claude() {
                    std::thread::spawn(move || claude.warm());
                }
            }
        }
    }
    save_settings(&app);
    Ok(settings_as_shown(&app))
}

/// Shows the window with the settings screen open.
fn open_settings(app: &AppHandle) {
    summon(app);
    let _ = app.emit("open-settings", ());
}

// ---- tray -------------------------------------------------------------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Slipstick", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", "Move back to the corner", true, None::<&str>)?;
    let disconnect = MenuItem::with_id(app, "disconnect", "Disconnect Claude", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &reset, &disconnect, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Slipstick")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => summon(app),
            "settings" => open_settings(app),
            "reset" => reset_position(app),
            "disconnect" => forget_claude(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                summon(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch (for example from the taskbar pin)
        // summons the running window instead of starting another copy.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == START_HIDDEN) {
                summon(app);
            }
        }))
        // "Start with Windows". Windows keeps the entry; the app is started
        // from it with the window hidden.
        .plugin(tauri_plugin_autostart::Builder::new().arg(START_HIDDEN).build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_visible_height,
            hide_window,
            evaluate,
            explain,
            claude_status,
            connect_claude,
            warm_claude,
            ask_claude,
            cancel_claude,
            disconnect_claude,
            settings_view,
            change_setting,
            pause_hotkey
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let mut settings = settings::load(&handle);

            // Registration fails if another program already holds the
            // combination; the window then says so instead of failing silently.
            // A shortcut in the file that cannot be read gives way to the
            // usual one.
            let hotkey: Shortcut = match settings.hotkey.parse() {
                Ok(hotkey) => hotkey,
                Err(_) => {
                    settings.hotkey = settings::DEFAULT_HOTKEY.to_string();
                    settings.hotkey.parse().expect("the usual shortcut can be read")
                }
            };
            let registered = handle.global_shortcut().register(hotkey).is_ok();

            app.manage(AppState {
                hotkey_registered: AtomicBool::new(registered),
                settings: Mutex::new(settings),
                unsaved: AtomicBool::new(false),
                claude: OnceLock::new(),
            });
            look_for_claude(&handle);

            build_tray(&handle)?;

            if let Some(window) = main_window(&handle) {
                place(&handle, &window);
                clip(&window, INITIAL_VISIBLE_HEIGHT)?;
                set_taskbar_icon(&window)?;

                let for_events = handle.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Moved(position) = event {
                        remember_position(&for_events, *position);
                    }
                });
            }
            // Started by Windows at sign-in: wait in the tray until summoned.
            if !std::env::args().any(|arg| arg == START_HIDDEN) {
                summon(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Slipstick")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                save_settings(app);
                if let Some(claude) = app.state::<AppState>().claude.get() {
                    claude.cancel();
                    claude.rest();
                }
            }
        });
}
