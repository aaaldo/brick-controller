// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Brick Controller: the TrimUI Brick as a standard Bluetooth HID gamepad.
//!
//! Two identities: a generic PC gamepad, and an Xbox Wireless Controller for
//! XInput. The choice is kept per host and gives the host an identity it
//! already knows. The screen follows NextUI's theme and layout: a Menu for
//! pairing and host selection, and a controller view that shows the host and
//! profile. After a host connects the panel can switch off while reports keep
//! flowing, until Menu+Start wakes it.

mod bluez;
use brick::canvas;
mod display;
mod hid;
mod l2cap;
mod pad;
#[cfg(test)]
use brick::png;
mod rumble;
mod sdp;
mod ui;
mod xbox;
use brick::text;

use canvas::Canvas;
use l2cap::{Addr, Channel, Listener};
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use text::Fonts;
use ui::{Modal, Row, StatusInfo, Theme, View};

const NAME: &str = "Brick Controller";
/// The release, as `package.sh` builds it; "dev" for any other build.
const VERSION: &str = match option_env!("BRICK_CONTROLLER_VERSION") {
    Some(v) => v,
    None => "0.1.0",
};
const DATA_DIR: &str = "/mnt/SDCARD/.userdata/tg5040/brick-controller";

/// Holding Menu and Start together this long disconnects from the host and
/// returns to Devices, without sending either button to the host.
const DISCONNECT_HOLD: Duration = Duration::from_millis(1000);
/// How long the blocking "Connecting to…" popup waits before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// After a host connects, the panel stays on this long, then goes dark until
/// Menu+Start is pressed once.
const SCREEN_OFF_AFTER: Duration = Duration::from_secs(10);
/// Idle poll interval while the panel is off; nothing needs animation then.
const SCREEN_OFF_POLL: i32 = 250;
/// Between attempts to reach the host we were last paired with.
const RECONNECT_EVERY: Duration = Duration::from_secs(4);
/// How long after starting or losing the link to keep at that pace...
const RECONNECT_BRISKLY_FOR: Duration = Duration::from_secs(60);
/// ...and the pace after that.
const RECONNECT_SLOWLY: Duration = Duration::from_secs(30);
/// The least time between two reports. The XR829 takes reports faster than
/// it can send them and queues them inside the chip, past any limit the
/// kernel sets: with a PC also sending its sound, ~118 reports a second got
/// through, and at 125 a second plus one per change the lag grew by 30 ms
/// every second, to 884 ms after 30 s. GIMX, which stands in for a
/// DualShock 4 to a PS4, sends one report every 10 ms and never more; so does
/// Padpod.
const REPORT_GAP: Duration = Duration::from_millis(10);

fn data_dir() -> PathBuf {
    for key in ["BRICK_CONTROLLER_DATA", "PADPOD_DATA"] {
        if let Ok(path) = std::env::var(key) {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = std::env::var("USERDATA_PATH") {
        return PathBuf::from(path).join("brick-controller");
    }
    PathBuf::from(DATA_DIR)
}

/// The HID identity used with a host. A host remembers the controller type it
/// paired with, so the choice is stored per host and applied when connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    Pc,
    Xbox,
}

impl Profile {
    /// Every controller type offered by the selection screen. Add future
    /// identities here; the screen and its navigation are list-based.
    const ALL: [Self; 2] = [Profile::Pc, Profile::Xbox];

    fn label(self) -> &'static str {
        match self {
            Profile::Pc => "PC",
            Profile::Xbox => "Xbox",
        }
    }

    fn bt_name(self) -> &'static str {
        match self {
            Profile::Pc => NAME,
            Profile::Xbox => xbox::NAME,
        }
    }

    fn descriptor(self) -> &'static [u8] {
        match self {
            Profile::Pc => hid::DESCRIPTOR,
            Profile::Xbox => xbox::DESCRIPTOR,
        }
    }

    fn device_id(self) -> Option<String> {
        match self {
            Profile::Pc => None,
            Profile::Xbox => Some(format!(
                "usb:{:04X}:{:04X}:0408",
                xbox::VENDOR,
                xbox::PRODUCT
            )),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Profile::Pc => "pc",
            Profile::Xbox => "xbox",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "pc" => Some(Profile::Pc),
            "xbox" => Some(Profile::Xbox),
            _ => None,
        }
    }
}

/// The profile used by the last session. It is also the default for a host
/// that has never been seen, so a profile change survives a restart before
/// the host has paired again.
fn mode_path() -> PathBuf {
    data_dir().join("mode")
}

fn last_profile() -> Profile {
    std::fs::read_to_string(mode_path())
        .ok()
        .and_then(|text| Profile::from_key(&text))
        .unwrap_or(Profile::Pc)
}

fn save_last_profile(profile: Profile) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(mode_path(), profile.key());
}

fn profiles_path() -> PathBuf {
    data_dir().join("profiles")
}

fn load_profiles() -> HashMap<Addr, Profile> {
    let mut profiles = HashMap::new();
    let Ok(text) = std::fs::read_to_string(profiles_path()) else {
        return profiles;
    };
    for line in text.lines() {
        let Some((address, profile)) = line.split_once('=') else {
            continue;
        };
        let Some(address) = Addr::parse(address) else {
            continue;
        };
        if let Some(profile) = Profile::from_key(profile) {
            profiles.insert(address, profile);
        }
    }
    profiles
}

fn save_profiles(profiles: &HashMap<Addr, Profile>) {
    let mut entries: Vec<(&Addr, &Profile)> = profiles.iter().collect();
    entries.sort_by_key(|(address, _)| address.to_string());
    let text: String = entries
        .into_iter()
        .map(|(address, profile)| format!("{address}={}\n", profile.key()))
        .collect();
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(profiles_path(), text);
}

fn profile_for(address: Addr) -> Profile {
    load_profiles()
        .get(&address)
        .copied()
        .unwrap_or(Profile::Pc)
}

/// Remember the identity a host was actually paired with. Called when a link
/// comes up: a host new to the list keeps the profile the session is using,
/// which is what the host just cached. Existing entries are never rewritten.
fn remember_profile(address: Addr, profile: Profile) {
    let mut profiles = load_profiles();
    if profiles.contains_key(&address) {
        return;
    }
    profiles.insert(address, profile);
    save_profiles(&profiles);
}

/// The last host. A host caches the controller identity it first paired with,
/// so the profile is kept per host.
fn saved_host() -> Option<Addr> {
    std::fs::read_to_string(data_dir().join("host"))
        .ok()
        .and_then(|s| Addr::parse(&s))
}

fn save_host(addr: Option<Addr>) {
    let path = data_dir().join("host");
    match addr {
        Some(a) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, a.to_string());
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Link {
    control: Channel,
    interrupt: Channel,
    host: Addr,
    name: String,
}

/// Keep trying to reach the host last paired with, while nothing is connected.
/// A HID device is expected to reconnect on its own; a PC does not come
/// looking for its gamepad.
///
/// Briskly for the first minute after starting or losing the link, then only
/// now and then. Each attempt pages the host for seconds, and a radio busy
/// paging answers scans poorly: trying every few seconds for a host that is
/// switched off left the handheld hard to find for pairing anything else.
fn spawn_reconnector(
    target: Arc<Mutex<Option<Addr>>>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    out: mpsc::Sender<(Channel, Channel, Addr)>,
) {
    std::thread::spawn(move || {
        let mut since = Instant::now();
        let mut last_try: Option<Instant> = None;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            if connected.load(Ordering::Relaxed) {
                since = Instant::now();
                continue;
            }
            let every = if since.elapsed() < RECONNECT_BRISKLY_FOR {
                RECONNECT_EVERY
            } else {
                RECONNECT_SLOWLY
            };
            if last_try.is_some_and(|t| t.elapsed() < every) {
                continue;
            }
            last_try = Some(Instant::now());
            let Some(host) = *target.lock().unwrap() else {
                continue;
            };
            let Ok(control) = Channel::connect(host, sdp::PSM_CONTROL) else {
                continue;
            };
            // A session that ended while this was paging must not be handed
            // a link meant for a previous session.
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match Channel::connect(host, sdp::PSM_INTERRUPT) {
                Ok(interrupt) => {
                    eprintln!("link: reconnected to {host}");
                    if out.send((control, interrupt, host)).is_err() {
                        return;
                    }
                }
                Err(e) => eprintln!(
                    "link: {host} took the control channel but not the interrupt one: {e}"
                ),
            }
        }
    });
}

/// Ends the session's reconnector when the session ends, however it ends.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// What the screen should show next.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    view: View,
    status: StatusInfo,
    modal: Option<Modal>,
    /// Chord progress, quantised so an unchanged hold does not redraw.
    hold: Option<u8>,
}

impl Frame {
    /// Splash and connection popup use an indeterminate bar; the renderer
    /// keeps redrawing them so its segment moves.
    fn animates(&self) -> bool {
        matches!(&self.view, View::Splash { .. })
            || self.modal.as_ref().is_some_and(|modal| modal.loading)
    }
}

/// The screen, drawn on a thread of its own.
///
/// Drawing a frame takes long enough on this CPU that doing it between
/// reading the pad and sending the report put every redraw in the path of
/// the next input: a stick moving continuously queued its events behind the
/// frames it caused. The I/O loop now only posts the latest state here; a
/// frame that is still drawing when several more arrive is followed by the
/// newest only.
struct Renderer {
    slot: Arc<(Mutex<Option<Option<Frame>>>, std::sync::Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The shortest time between two frames. The screen is a status display; the
/// gamepad is what has to be quick. Twenty frames a second keeps the pressed
/// controls readable while leaving the CPU mostly to the HID loop.
const FRAME_EVERY: Duration = Duration::from_millis(50);
/// Redraw cadence for an indeterminate loading bar.
const ANIMATION_EVERY: Duration = Duration::from_millis(120);
/// One complete back-and-forth sweep of the indeterminate segment.
const LOADING_PHASE: Duration = Duration::from_millis(1500);

/// `gettid` is Linux-only; on a host build the process id is enough for the
/// priority call, which only ever runs on the device anyway.
#[cfg(target_os = "linux")]
fn current_tid() -> i32 {
    unsafe { libc::syscall(libc::SYS_gettid) as i32 }
}

#[cfg(not(target_os = "linux"))]
fn current_tid() -> i32 {
    unsafe { libc::getpid() }
}

fn draw_frame(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    frame: &Frame,
    phase: f32,
    drawn: &mut u32,
) {
    let started = Instant::now();
    ui::draw(
        canvas,
        fonts,
        theme,
        &frame.view,
        &frame.status,
        frame.modal.as_ref(),
        frame.hold.map(|step| step as f32 / 20.0),
        phase,
    );
    let _ = canvas.flush();
    // A few, to know what a frame costs on the device.
    *drawn += 1;
    if *drawn <= 5 {
        eprintln!(
            "screen: frame {} took {} ms",
            *drawn,
            started.elapsed().as_millis()
        );
    }
}

impl Renderer {
    fn spawn(mut canvas: Canvas, mut fonts: Fonts, theme: Theme) -> Self {
        let slot: Arc<(Mutex<Option<Option<Frame>>>, std::sync::Condvar)> = Arc::default();
        let shared = Arc::clone(&slot);
        let thread = std::thread::spawn(move || {
            // Below the I/O thread, so a frame never delays a report.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, current_tid() as _, 10) };
            let (lock, ready) = &*shared;
            let mut drawn = 0u32;
            let mut current: Option<Frame> = None;
            let animation_start = Instant::now();
            loop {
                let next = {
                    let mut pending = lock.lock().unwrap();
                    if pending.is_none() {
                        if current.as_ref().is_some_and(Frame::animates) {
                            let (guard, _) = ready.wait_timeout(pending, ANIMATION_EVERY).unwrap();
                            pending = guard;
                            if pending.is_none() {
                                drop(pending);
                                if let Some(frame) = current.as_ref() {
                                    let phase = (animation_start.elapsed().as_secs_f32()
                                        / LOADING_PHASE.as_secs_f32())
                                    .fract();
                                    draw_frame(
                                        &mut canvas,
                                        &mut fonts,
                                        &theme,
                                        frame,
                                        phase,
                                        &mut drawn,
                                    );
                                }
                                continue;
                            }
                        } else {
                            while pending.is_none() {
                                pending = ready.wait(pending).unwrap();
                            }
                        }
                    }
                    pending.take().unwrap()
                };
                let started = Instant::now();
                match next {
                    None => {
                        canvas.clear(theme.background);
                        let _ = canvas.flush();
                        return;
                    }
                    Some(frame) => {
                        let phase = (animation_start.elapsed().as_secs_f32()
                            / LOADING_PHASE.as_secs_f32())
                        .fract();
                        draw_frame(&mut canvas, &mut fonts, &theme, &frame, phase, &mut drawn);
                        current = Some(frame);
                    }
                }
                if let Some(rest) = FRAME_EVERY.checked_sub(started.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        });
        Self {
            slot,
            thread: Some(thread),
        }
    }

    fn show(&self, frame: Frame) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(Some(frame));
        ready.notify_one();
    }
}

impl Drop for Renderer {
    /// Blank the screen and wait for it, so the launcher never comes back
    /// over a half-drawn frame.
    fn drop(&mut self) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(None);
        ready.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("restore") {
        bluez::restore_system_daemon();
        rumble::stop();
        if let Err(e) = display::restore() {
            eprintln!("display: could not restore the panel: {e}");
        }
        return;
    }
    if let Err(e) = run() {
        eprintln!("brick-controller: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    eprintln!("brick-controller {VERSION}");
    let canvas = Canvas::open("/dev/fb0").map_err(|e| format!("framebuffer: {e}"))?;
    let theme = Theme::load();
    let fonts = Fonts::load(&ui::font_paths());
    let mut pad = pad::Pad::open(&pad::pad_path()).map_err(|e| format!("pad: {e}"))?;
    let screen = Renderer::spawn(canvas, fonts, theme);
    let mut profile = saved_host().map(profile_for).unwrap_or_else(last_profile);
    let mut start_in_menu = false;
    // No host is reached until the user asks for it from Devices.
    let mut connect = None;
    loop {
        save_last_profile(profile);
        match session(profile, start_in_menu, connect, &screen, &mut pad)? {
            SessionOutcome::Quit => break,
            SessionOutcome::SwitchProfile {
                profile: next,
                connect: next_connect,
            } => {
                profile = next;
                connect = next_connect;
                eprintln!("profile: {}", profile.label());
                // A profile change comes from the menu; reopen it across the
                // Bluetooth restart.
                start_in_menu = true;
            }
            SessionOutcome::Retry => {
                connect = None;
                start_in_menu = true;
            }
        }
    }
    drop(screen);
    Ok(())
}

/// The menu's data: paired hosts and the identity a new host would see.
#[derive(Clone, PartialEq, Eq)]
struct Settings {
    hosts: Vec<bluez::Host>,
    profiles: HashMap<Addr, Profile>,
    /// The identity a new host would pair with, when no host is known yet.
    pairing: Profile,
    error: Option<String>,
}

/// One focusable line in the main menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuRow {
    Host(usize),
    Pair,
}

/// The screen currently displayed above the Bluetooth session.
#[derive(Clone, PartialEq, Eq)]
enum Screen {
    Controller,
    Menu { cursor: usize },
    Profile { choice: Profile },
    Pairing { back: bool },
}

/// A modal waiting for A or B.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalState {
    Forget(usize),
    Connecting(Addr),
}

type DrawKey = (View, StatusInfo, Option<Modal>, Option<u8>);

impl Settings {
    fn load(bus: &bluez::Bus, pairing: Profile) -> Self {
        match bus.paired_hosts() {
            Ok(hosts) => {
                eprintln!("menu: {} paired host(s)", hosts.len());
                Self {
                    hosts,
                    profiles: load_profiles(),
                    pairing,
                    error: None,
                }
            }
            Err(error) => {
                eprintln!("menu: {error}");
                Self {
                    hosts: Vec::new(),
                    profiles: load_profiles(),
                    pairing,
                    error: Some(error),
                }
            }
        }
    }

    /// Devices always ends with "Pair a new device", even while a host is
    /// connected: connecting and pairing are explicit choices on this screen.
    fn rows(&self) -> Vec<MenuRow> {
        let mut rows: Vec<MenuRow> = (0..self.hosts.len()).map(MenuRow::Host).collect();
        rows.push(MenuRow::Pair);
        rows
    }

    fn profile_for(&self, address: Addr) -> Profile {
        self.profiles.get(&address).copied().unwrap_or(Profile::Pc)
    }
}

enum SessionOutcome {
    Quit,
    SwitchProfile {
        profile: Profile,
        connect: Option<Addr>,
    },
    Retry,
}

/// Release every button the host may still see held, in the profile's format.
fn send_neutral(link: &Link, profile: Profile) {
    let state = hid::State::default();
    let _ = match profile {
        Profile::Pc => link.interrupt.send(&hid::input_packet(&state)),
        Profile::Xbox => link
            .interrupt
            .send(&xbox::controls(&state))
            .and_then(|()| link.interrupt.send(&xbox::guide(&state))),
    };
}

/// Drive the handheld's single motor from an Xbox output report. Only the
/// Xbox profile creates a motor; `None` keeps the same path for PC reports.
fn drive_rumble(motor: Option<&rumble::Rumble>, report: &[u8]) {
    if let Some(level) = xbox::rumble(report) {
        if let Some(motor) = motor {
            motor.set(level);
        }
    }
}

/// End the active link, a half-open pairing channel and any connect attempt.
/// Devices is a chooser, so nothing may keep running while it is open.
fn stop_devices_session(
    link: &mut Option<Link>,
    half: &mut Option<(Channel, Addr)>,
    bus: &bluez::Bus,
    target: &Arc<Mutex<Option<Addr>>>,
    connected: &Arc<AtomicBool>,
    motor: Option<&rumble::Rumble>,
) {
    if let Some(l) = link.take() {
        let _ = bus.disconnect(l.host);
        eprintln!("link: {} disconnected for Devices", l.name);
    }
    half.take();
    *target.lock().unwrap() = None;
    // No address survives in the file: the next start must not look for a host.
    save_host(None);
    connected.store(false, Ordering::Relaxed);
    if let Some(motor) = motor {
        motor.set(0);
    }
}

#[allow(clippy::too_many_arguments)]
fn open_devices(
    link: &mut Option<Link>,
    half: &mut Option<(Channel, Addr)>,
    bus: &bluez::Bus,
    settings: &mut Settings,
    profile: Profile,
    target: &Arc<Mutex<Option<Addr>>>,
    connected: &Arc<AtomicBool>,
    motor: Option<&rumble::Rumble>,
) {
    stop_devices_session(link, half, bus, target, connected, motor);
    *settings = Settings::load(bus, profile);
}

/// Bluetooth up as the chosen controller, until Menu+Start is held or another
/// host profile is selected.
fn session(
    profile: Profile,
    start_in_settings: bool,
    connect: Option<Addr>,
    screen: &Renderer,
    pad: &mut pad::Pad,
) -> Result<SessionOutcome, String> {
    if !start_in_settings {
        screen.show(Frame {
            view: View::Splash {
                message: "Starting Bluetooth…".into(),
            },
            status: StatusInfo::read(false),
            modal: None,
            hold: None,
        });
    }

    let daemon = match bluez::Daemon::take_over(profile.bt_name(), profile.device_id()) {
        Ok(d) => d,
        Err(e) => return fail(screen, pad, e),
    };
    let bus = match bluez::Bus::open(&sdp::record(profile.bt_name(), profile.descriptor())) {
        Ok(b) => b,
        Err(e) => return fail(screen, pad, e),
    };
    let _ = bus.set_alias(profile.bt_name());
    let listeners =
        Listener::bind(sdp::PSM_CONTROL).and_then(|c| Ok((c, Listener::bind(sdp::PSM_INTERRUPT)?)));
    let (control_listener, interrupt_listener) = match listeners {
        Ok(pair) => pair,
        // A socket already in use is not a Bluetooth-off condition: another
        // instance or a stale listener owns the HID PSMs. Exit cleanly so the
        // wrapper restores the system instead of showing the recovery screen.
        Err(e) => return Err(format!("could not open the HID channels: {e}")),
    };
    let _ = control_listener.set_nonblocking();
    let _ = interrupt_listener.set_nonblocking();

    // Only the Xbox descriptor declares an output report; the PC profile has
    // no motor path. A missing GPIO is not an error: the gamepad still works.
    let motor = match profile {
        Profile::Xbox => {
            let motor = rumble::Rumble::open();
            if motor.is_none() {
                eprintln!("rumble: motor not available");
            }
            motor
        }
        Profile::Pc => None,
    };

    let target = Arc::new(Mutex::new(connect));
    let connected = Arc::new(AtomicBool::new(false));
    let (reconnected_tx, reconnected) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let _stop_reconnector = StopOnDrop(Arc::clone(&stop));
    spawn_reconnector(
        Arc::clone(&target),
        Arc::clone(&connected),
        stop,
        reconnected_tx,
    );

    let mut link: Option<Link> = None;
    // A control channel accepted, waiting for its interrupt channel.
    let mut half: Option<(Channel, Addr)> = None;
    let mut discoverable = None;
    let mut sent = None;
    let mut sent_at = Instant::now();
    let mut previous = pad.mapper.state;
    let mut disconnect_since: Option<Instant> = None;
    let mut combo_seen = false;
    let mut settings = Settings::load(&bus, profile);
    let mut state = Screen::Menu { cursor: 0 };
    let mut modal = connect.map(ModalState::Connecting);
    let mut connect_deadline = connect.map(|_| Instant::now() + CONNECT_TIMEOUT);
    // The profile to switch to. A switch always comes from the menu, so the
    // new session reopens it. The host is reconnected only when the switch was
    // part of an explicit Connect.
    let mut switch_to: Option<Profile> = None;
    let mut switch_connect: Option<Addr> = None;
    let mut drawn: Option<DrawKey> = None;
    let mut backlight = display::Backlight::open();
    // A dark panel is woken by Menu+Start; `wake_used` keeps the same held
    // chord from immediately starting the disconnect countdown.
    let mut screen_off = false;
    let mut wake_used = false;
    let mut screen_off_at: Option<Instant> = None;
    // A gamepad button used in a menu must not be sent to the host when the
    // controller view comes back; wait until every button is released.
    let mut resume_after_release = false;
    let mut last_ui_active = true;

    let outcome = 'session: loop {
        // Only the pairing screen advertises and accepts a new host. Devices
        // and its sub-screens stay quiet so nothing connects behind the
        // user's back; an explicit Connect goes through the reconnector.
        let pairing_screen = matches!(state, Screen::Pairing { .. });
        let want_discoverable = pairing_screen;
        if discoverable != Some(want_discoverable) {
            if let Err(e) = bus.set_discoverable(want_discoverable) {
                eprintln!("bluez: {e}");
            }
            discoverable = Some(want_discoverable);
        }

        let control_state = if matches!(state, Screen::Controller) && modal.is_none() {
            pad.mapper.state
        } else {
            hid::State::default()
        };

        let mut fds = vec![
            libc::pollfd {
                fd: pad.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: control_listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: interrupt_listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if let Some(l) = &link {
            fds.push(libc::pollfd {
                fd: l.control.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
            fds.push(libc::pollfd {
                fd: l.interrupt.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        // A change held back by the report gap is due when the gap ends;
        // everything else waits for something to happen. A dark panel has no
        // animation to feed, so it can sleep between input events.
        let wait = if link.is_some() && Some(pad.mapper.state) != sent {
            REPORT_GAP.saturating_sub(sent_at.elapsed()).as_millis() as i32
        } else if screen_off {
            SCREEN_OFF_POLL
        } else {
            100
        };
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, wait) };

        if fds[0].revents & libc::POLLIN != 0 {
            match pad.read() {
                Ok(true) => {}
                Ok(false) => {}
                Err(e) => return Err(format!("pad: {e}")),
            }
        }
        // A quick Menu+Start tap may be released before this poll, so the
        // mapper remembers that both buttons were down together.
        let combo_tapped = pad.mapper.take_combo_tap();

        // New connections: the host opens control first, then interrupt.
        // Outside the pairing screen the channels are accepted and dropped so
        // a stale peer cannot reconnect on its own.
        if let Ok(Some((control, host))) = control_listener.accept() {
            if pairing_screen {
                eprintln!("link: control channel from {host}");
                half = Some((control, host));
            } else {
                eprintln!("link: control channel from {host} outside pairing; dropped");
            }
        }
        if let Ok(Some((interrupt, host))) = interrupt_listener.accept() {
            if !pairing_screen {
                eprintln!("link: interrupt channel from {host} outside pairing; dropped");
            } else {
                match half.take() {
                    Some((control, from)) if from == host => {
                        link = Some(Link {
                            control,
                            interrupt,
                            name: String::new(),
                            host,
                        });
                    }
                    _ => eprintln!(
                        "link: interrupt channel from {host} with no control channel; dropped"
                    ),
                }
            }
        }
        if let Ok((control, interrupt, host)) = reconnected.try_recv() {
            // The user may have left Devices, or chosen another host, while
            // this attempt was paging: a stale link must not come up behind
            // the screen that cancelled it.
            if link.is_none() && *target.lock().unwrap() == Some(host) {
                link = Some(Link {
                    control,
                    interrupt,
                    name: String::new(),
                    host,
                });
            } else {
                eprintln!("link: stale reconnection to {host} dropped");
            }
        }
        if let Some(l) = link.as_mut().filter(|l| l.name.is_empty()) {
            // Just connected.
            bus.trust(l.host);
            remember_profile(l.host, profile);
            save_host(Some(l.host));
            *target.lock().unwrap() = Some(l.host);
            connected.store(true, Ordering::Relaxed);
            l.name = bus.name_of(l.host).unwrap_or_else(|| l.host.to_string());
            eprintln!("link: connected to {} ({})", l.name, l.host);
            sent = None;
            if let Some(motor) = &motor {
                motor.set(0);
            }
            // The controller view is the point of a connected host.
            settings = Settings::load(&bus, profile);
            state = Screen::Controller;
            modal = None;
            connect_deadline = None;
            screen_off_at = Some(Instant::now() + SCREEN_OFF_AFTER);
            resume_after_release = false;
            drawn = None;
            // A report the radio cannot take yet is dropped, not queued: the
            // next one carries the whole state.
            if let Err(e) = l.interrupt.shorten_queue() {
                eprintln!("link: could not shorten the send queue: {e}");
            }
        }

        // The host's side of the conversation.
        let mut lost = false;
        let mut unplugged = false;
        if let Some(l) = &link {
            loop {
                match l.control.recv() {
                    Ok(Some(message)) => match hid::control(&message, &control_state) {
                        hid::Control::Reply(bytes) => {
                            if let Some(report) = hid::set_report_output(&message) {
                                drive_rumble(motor.as_ref(), report);
                            }
                            if l.control.send(&bytes).is_err() {
                                lost = true;
                            }
                        }
                        hid::Control::Unplug => unplugged = true,
                        hid::Control::Ignore => {}
                    },
                    Ok(None) => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            // Output reports (rumble) arrive on the interrupt channel as
            // 0xA2 DATA transactions.
            loop {
                match l.interrupt.recv() {
                    Ok(Some(message)) => {
                        if let Some(report) = hid::data_output(&message) {
                            drive_rumble(motor.as_ref(), report);
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
        }
        if unplugged {
            if let Some(l) = link.take() {
                eprintln!("link: {} unpaired us", l.name);
                let _ = bus.remove(l.host);
            }
            save_host(None);
            *target.lock().unwrap() = None;
            lost = true;
        }
        if lost {
            if let Some(l) = link.take() {
                eprintln!("link: {} disconnected", l.name);
            }
            // A host that goes away must not leave the motor running, and
            // Devices must not keep paging it.
            open_devices(
                &mut link,
                &mut half,
                &bus,
                &mut settings,
                profile,
                &target,
                &connected,
                motor.as_ref(),
            );
            // The menu now shows the new state; a dark panel must come back
            // so the user can see why the link went away.
            if screen_off {
                backlight.on();
                screen_off = false;
                wake_used = false;
            }
            state = Screen::Menu { cursor: 0 };
            modal = None;
            connect_deadline = None;
            screen_off_at = None;
            drawn = None;
            lost = false;
        }

        // The popup blocks input, so it must release the user when the host
        // never answers. Clearing the target also drops a stale link attempt.
        if matches!(modal, Some(ModalState::Connecting(_)))
            && connect_deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            eprintln!("menu: connection timed out");
            stop_devices_session(
                &mut link,
                &mut half,
                &bus,
                &target,
                &connected,
                motor.as_ref(),
            );
            modal = None;
            connect_deadline = None;
            state = Screen::Menu { cursor: 0 };
            drawn = None;
        }

        let now = pad.mapper.state;
        let a_pressed = now.pressed(hid::button::EAST) && !previous.pressed(hid::button::EAST);
        let b_pressed = now.pressed(hid::button::SOUTH) && !previous.pressed(hid::button::SOUTH);
        let x_pressed = now.pressed(hid::button::NORTH) && !previous.pressed(hid::button::NORTH);
        let direction = nav_dir(previous.hat, now.hat);
        let combos = now.pressed(hid::button::HOME) && now.pressed(hid::button::START);
        let combo_was = previous.pressed(hid::button::HOME) && previous.pressed(hid::button::START);

        // Interacting with the chord keeps the panel on while it is held,
        // except for the hold that just woke it: that one must leave the
        // 10-second cycle armed.
        if !screen_off && combos && !wake_used {
            screen_off_at = None;
        }

        // A dark panel is woken by pressing Menu+Start once. The same held
        // chord must not start the disconnect countdown, and the buttons are
        // kept away from the host until they are released.
        if screen_off {
            if combos || combo_tapped {
                combo_seen = true;
                if !combo_was {
                    backlight.on();
                    screen_off = false;
                    wake_used = true;
                    screen_off_at = Some(Instant::now() + SCREEN_OFF_AFTER);
                    drawn = None;
                }
            } else {
                combo_seen = false;
            }
        } else if !wake_used {
            let combo_was_seen = combo_seen;
            if combos {
                combo_seen = true;
                let since = disconnect_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= DISCONNECT_HOLD && matches!(state, Screen::Controller) {
                    open_devices(
                        &mut link,
                        &mut half,
                        &bus,
                        &mut settings,
                        profile,
                        &target,
                        &connected,
                        motor.as_ref(),
                    );
                    state = Screen::Menu { cursor: 0 };
                    modal = None;
                    screen_off_at = None;
                    drawn = None;
                }
            } else {
                disconnect_since = None;
            }
            if !combos {
                combo_seen = false;
            }
            let combo_suppress = combo_seen || (combo_was_seen && !combos);
            let back = b_pressed;
            let confirm = a_pressed;

            if !combo_suppress {
                if let Some(pending) = modal {
                    match pending {
                        ModalState::Forget(index) => {
                            if confirm {
                                if let Some(host) = settings.hosts.get(index) {
                                    let addr = host.addr;
                                    eprintln!("menu: forget {addr}");
                                    stop_devices_session(
                                        &mut link,
                                        &mut half,
                                        &bus,
                                        &target,
                                        &connected,
                                        motor.as_ref(),
                                    );
                                    let _ = bus.remove(addr);
                                    settings.profiles.remove(&addr);
                                    save_profiles(&settings.profiles);
                                    settings = Settings::load(&bus, profile);
                                    state = Screen::Menu { cursor: 0 };
                                }
                                modal = None;
                                drawn = None;
                            } else if back {
                                modal = None;
                                drawn = None;
                            }
                        }
                        // The connection popup owns the screen until the link
                        // comes up or its timeout expires.
                        ModalState::Connecting(_) => {}
                    }
                } else {
                    match state.clone() {
                        Screen::Menu { cursor } => {
                            let rows = settings.rows();
                            let cursor = cursor.min(rows.len().saturating_sub(1));
                            if let Some(dir) = direction {
                                let delta = match dir {
                                    Dir::Up => -1,
                                    Dir::Down => 1,
                                    _ => 0,
                                };
                                if delta != 0 {
                                    state = Screen::Menu {
                                        cursor: move_cursor(cursor, rows.len(), delta),
                                    };
                                    drawn = None;
                                }
                            }
                            if b_pressed {
                                if let Some(l) = &link {
                                    send_neutral(l, profile);
                                }
                                break 'session SessionOutcome::Quit;
                            }
                            if x_pressed {
                                if let Some(MenuRow::Host(index)) = rows.get(cursor).copied() {
                                    // Forget is decided on this screen; a
                                    // pending Connect must not win the race.
                                    stop_devices_session(
                                        &mut link,
                                        &mut half,
                                        &bus,
                                        &target,
                                        &connected,
                                        motor.as_ref(),
                                    );
                                    modal = Some(ModalState::Forget(index));
                                    drawn = None;
                                }
                            }
                            if confirm {
                                if let Some(row) = rows.get(cursor).copied() {
                                    // Connect is always a fresh choice from
                                    // this screen, never a resumed link.
                                    stop_devices_session(
                                        &mut link,
                                        &mut half,
                                        &bus,
                                        &target,
                                        &connected,
                                        motor.as_ref(),
                                    );
                                    match row {
                                        MenuRow::Host(index) => {
                                            if let Some(host) = settings.hosts.get(index) {
                                                let addr = host.addr;
                                                let desired = settings.profile_for(addr);
                                                eprintln!(
                                                    "menu: connect {addr} as {}",
                                                    desired.label()
                                                );
                                                bus.trust(addr);
                                                save_host(Some(addr));
                                                *target.lock().unwrap() = Some(addr);
                                                modal = Some(ModalState::Connecting(addr));
                                                connect_deadline =
                                                    Some(Instant::now() + CONNECT_TIMEOUT);
                                                if desired != profile {
                                                    switch_to = Some(desired);
                                                    switch_connect = Some(addr);
                                                } else {
                                                    state = Screen::Menu { cursor: index };
                                                }
                                            }
                                        }
                                        MenuRow::Pair => state = Screen::Pairing { back: true },
                                    }
                                    drawn = None;
                                }
                            }
                        }
                        Screen::Profile { choice } => {
                            if let Some(dir) = direction {
                                let step = match dir {
                                    Dir::Up => -1,
                                    Dir::Down => 1,
                                    _ => 0,
                                };
                                if step != 0 {
                                    let current = Profile::ALL
                                        .iter()
                                        .position(|profile| *profile == choice)
                                        .unwrap_or(0)
                                        as isize;
                                    let next = (current + step)
                                        .rem_euclid(Profile::ALL.len() as isize)
                                        as usize;
                                    state = Screen::Profile {
                                        choice: Profile::ALL[next],
                                    };
                                    drawn = None;
                                }
                            }
                            if confirm {
                                if choice != settings.pairing {
                                    settings.pairing = choice;
                                    save_last_profile(choice);
                                    switch_to = Some(choice);
                                } else {
                                    state = Screen::Pairing { back: true };
                                }
                                drawn = None;
                            } else if back {
                                state = Screen::Pairing { back: true };
                                drawn = None;
                            }
                        }
                        Screen::Pairing { back: can_back } => {
                            if x_pressed {
                                state = Screen::Profile {
                                    choice: settings.pairing,
                                };
                                drawn = None;
                            } else if back && can_back {
                                open_devices(
                                    &mut link,
                                    &mut half,
                                    &bus,
                                    &mut settings,
                                    profile,
                                    &target,
                                    &connected,
                                    motor.as_ref(),
                                );
                                state = Screen::Menu { cursor: 0 };
                                drawn = None;
                            }
                        }
                        Screen::Controller => {
                            // Leaving the Connected view is reserved for the
                            // Menu+Start hold, handled above this match.
                        }
                    }
                }
            }
        }

        if !combos {
            wake_used = false;
        }

        if !screen_off
            && matches!(state, Screen::Controller)
            && link.is_some()
            && screen_off_at.is_some_and(|deadline| Instant::now() >= deadline)
        {
            backlight.off();
            screen_off = true;
            screen_off_at = None;
            drawn = None;
        }

        if let Some(next) = switch_to {
            break 'session SessionOutcome::SwitchProfile {
                profile: next,
                connect: switch_connect,
            };
        }

        let ui_active = !matches!(state, Screen::Controller) || modal.is_some();
        if last_ui_active && !ui_active {
            resume_after_release = true;
        }
        if resume_after_release && now == hid::State::default() {
            resume_after_release = false;
        }
        last_ui_active = ui_active;
        let report_state = if ui_active || combo_seen || resume_after_release {
            hid::State::default()
        } else {
            now
        };

        if let Some(l) = &link {
            let due =
                Some(report_state) != sent && (sent.is_none() || sent_at.elapsed() >= REPORT_GAP);
            if !lost && due {
                let result = match profile {
                    Profile::Pc => l.interrupt.send(&hid::input_packet(&report_state)),
                    Profile::Xbox => {
                        let home = hid::button::HOME;
                        let guide_changed = sent.is_none_or(|state: hid::State| {
                            state.pressed(home) != report_state.pressed(home)
                        });
                        l.interrupt
                            .send(&xbox::controls(&report_state))
                            .and_then(|()| {
                                if guide_changed {
                                    l.interrupt.send(&xbox::guide(&report_state))
                                } else {
                                    Ok(())
                                }
                            })
                    }
                };
                match result {
                    Ok(()) => {
                        sent = Some(report_state);
                        sent_at = Instant::now();
                    }
                    // A full queue is the radio falling behind; the next
                    // report carries the whole state anyway.
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => lost = true,
                }
            }
        }
        if lost {
            if let Some(l) = link.take() {
                eprintln!("link: {} disconnected while sending", l.name);
            }
            open_devices(
                &mut link,
                &mut half,
                &bus,
                &mut settings,
                profile,
                &target,
                &connected,
                motor.as_ref(),
            );
            if screen_off {
                backlight.on();
                screen_off = false;
                wake_used = false;
            }
            state = Screen::Menu { cursor: 0 };
            modal = None;
            connect_deadline = None;
            screen_off_at = None;
            drawn = None;
        }

        if screen_off {
            previous = now;
            continue;
        }

        let status = StatusInfo::read(true);
        let target_addr = *target.lock().unwrap();
        let view = view_for(&state, &settings, link.as_ref(), profile, target_addr);
        let modal_view = modal.as_ref().map(|pending| modal_view(pending, &settings));
        let hold = if combo_seen && matches!(state, Screen::Controller) && !screen_off {
            disconnect_since.map(|since| {
                ((since.elapsed().as_secs_f32() / DISCONNECT_HOLD.as_secs_f32()).clamp(0.0, 1.0)
                    * 20.0) as u8
            })
        } else {
            None
        };
        let key = (view.clone(), status.clone(), modal_view.clone(), hold);
        if !screen_off && drawn.as_ref() != Some(&key) {
            screen.show(Frame {
                view,
                status,
                modal: modal_view,
                hold,
            });
            drawn = Some(key);
        }

        previous = now;
    };

    drop(link);
    let _ = bus.set_discoverable(false);
    drop(bus);
    drop(daemon);
    Ok(outcome)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

fn primary_dir(hat: u8) -> Option<Dir> {
    match hat {
        0..=1 | 7 => Some(Dir::Up),
        2 => Some(Dir::Right),
        3..=5 => Some(Dir::Down),
        6 => Some(Dir::Left),
        _ => None,
    }
}

fn nav_dir(previous: u8, current: u8) -> Option<Dir> {
    let from = primary_dir(previous);
    let to = primary_dir(current);
    if from == to {
        None
    } else {
        to
    }
}

fn move_cursor(cursor: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    ((cursor as isize + delta).rem_euclid(len as isize)) as usize
}

fn view_for(
    screen: &Screen,
    settings: &Settings,
    link: Option<&Link>,
    profile: Profile,
    target: Option<Addr>,
) -> View {
    match screen {
        Screen::Controller => View::Controller {
            host: link.map(|l| l.name.clone()).unwrap_or_default(),
            profile: profile.label(),
        },
        Screen::Menu { cursor } => {
            let rows = settings.rows();
            let cursor = (*cursor).min(rows.len().saturating_sub(1));
            let start = cursor.saturating_sub(4);
            let rows = rows
                .iter()
                .enumerate()
                .skip(start)
                .take(5)
                .map(|(index, row)| match row {
                    MenuRow::Host(host_index) => {
                        let host = &settings.hosts[*host_index];
                        let status = if host.connected {
                            "Connected"
                        } else {
                            "Offline"
                        };
                        Row::new(
                            host.name.clone(),
                            format!("{} · {status}", settings.profile_for(host.addr).label()),
                            index == cursor,
                        )
                    }
                    MenuRow::Pair => Row::new(
                        "Pair a new device",
                        settings.pairing.label(),
                        index == cursor,
                    ),
                })
                .collect();
            View::Menu { rows }
        }
        Screen::Profile { choice } => {
            let rows = Profile::ALL
                .iter()
                .map(|profile| Row::new(profile.label(), "", *profile == *choice))
                .collect();
            View::Profile { rows }
        }
        Screen::Pairing { .. } => View::Pairing {
            identity: settings.pairing.bt_name().into(),
            controller_type: settings.pairing.label(),
            reconnecting: target.map(|addr| addr.to_string()),
        },
    }
}

fn modal_view(modal: &ModalState, settings: &Settings) -> Modal {
    match modal {
        ModalState::Forget(index) => {
            let name = settings
                .hosts
                .get(*index)
                .map(|host| host.name.clone())
                .unwrap_or_else(|| "this controller".into());
            Modal {
                title: format!("Forget {name}?"),
                message: "You will need to pair it again from the host.".into(),
                confirm: "Forget",
                destructive: true,
                loading: false,
            }
        }
        ModalState::Connecting(addr) => {
            let name = settings
                .hosts
                .iter()
                .find(|host| host.addr == *addr)
                .map(|host| host.name.clone())
                .unwrap_or_else(|| addr.to_string());
            Modal {
                title: format!("Connecting to {name}"),
                message: String::new(),
                confirm: "",
                destructive: false,
                loading: true,
            }
        }
    }
}

/// Show an error and wait for Retry or Quit.
fn fail(screen: &Renderer, pad: &mut pad::Pad, error: String) -> Result<SessionOutcome, String> {
    eprintln!("brick-controller: {error}");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut previous = pad.mapper.state;
    let mut shown = 60u64;
    screen.show(Frame {
        view: View::Error {
            title: "Bluetooth off".into(),
            message: error.clone(),
            steps: vec![
                "Open NextUI Settings and turn Bluetooth on.".into(),
                "Close Brick Controller and open it again.".into(),
            ],
            countdown: Some("Closing in 60 s.".into()),
        },
        status: StatusInfo::read(false),
        modal: None,
        hold: None,
    });
    while Instant::now() < deadline {
        let mut fd = libc::pollfd {
            fd: pad.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut fd, 1, 200) };
        if fd.revents & libc::POLLIN != 0 && pad.read().is_ok() {
            let now = pad.mapper.state;
            let a = now.pressed(hid::button::EAST) && !previous.pressed(hid::button::EAST);
            let b = now.pressed(hid::button::SOUTH) && !previous.pressed(hid::button::SOUTH);
            previous = now;
            if a {
                return Ok(SessionOutcome::Retry);
            }
            if b {
                return Ok(SessionOutcome::Quit);
            }
        }
        let left = deadline.saturating_duration_since(Instant::now()).as_secs();
        if left != shown {
            shown = left;
            screen.show(Frame {
                view: View::Error {
                    title: "Bluetooth off".into(),
                    message: error.clone(),
                    steps: vec![
                        "Open NextUI Settings and turn Bluetooth on.".into(),
                        "Close Brick Controller and open it again.".into(),
                    ],
                    countdown: Some(format!("Closing in {left} s.")),
                },
                status: StatusInfo::read(false),
                modal: None,
                hold: None,
            });
        }
    }
    Ok(SessionOutcome::Quit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn preview_status(bluetooth: bool) -> StatusInfo {
        StatusInfo {
            clock: "14:32".into(),
            battery: Some(76),
            charging: false,
            wifi: true,
            bluetooth,
        }
    }

    fn preview_menu_rows(linked: bool) -> Vec<Row> {
        let mut rows = vec![
            Row::new(
                "MacBook Pro",
                if linked {
                    "PC · Connected"
                } else {
                    "PC · Offline"
                },
                true,
            ),
            Row::new("iPhone", "Xbox · Offline", false),
        ];
        rows.push(Row::new("Pair a new device", "PC", false));
        rows
    }

    /// `BRICK_CONTROLLER_PREVIEW=dir cargo test preview` writes every screen
    /// as a PNG, so the layout can be checked without the device.
    #[test]
    fn preview_every_screen() {
        let Ok(dir) = std::env::var("BRICK_CONTROLLER_PREVIEW") else {
            return;
        };
        let _ = fs::create_dir_all(&dir);
        let theme = Theme::fallback();
        let mut fonts = Fonts::load(&[]);
        let screens: Vec<(&str, View, StatusInfo)> = vec![
            (
                "splash",
                View::Splash {
                    message: "Starting Bluetooth…".into(),
                },
                preview_status(false),
            ),
            (
                "controller",
                View::Controller {
                    host: "MacBook Pro".into(),
                    profile: "PC",
                },
                preview_status(true),
            ),
            (
                "menu",
                View::Menu {
                    rows: preview_menu_rows(true),
                },
                preview_status(true),
            ),
            (
                "menu-idle",
                View::Menu {
                    rows: preview_menu_rows(false),
                },
                preview_status(true),
            ),
            (
                "profile",
                View::Profile {
                    rows: vec![Row::new("PC", "", true), Row::new("Xbox", "", false)],
                },
                preview_status(true),
            ),
            (
                "pairing",
                View::Pairing {
                    identity: "Brick Controller".into(),
                    controller_type: "PC",
                    reconnecting: None,
                },
                preview_status(true),
            ),
            (
                "error",
                View::Error {
                    title: "Bluetooth off".into(),
                    message: "Brick Controller could not start Bluetooth.".into(),
                    steps: vec![
                        "Open NextUI Settings and turn Bluetooth on.".into(),
                        "Close Brick Controller and open it again.".into(),
                    ],
                    countdown: Some("Closing in 60 s.".into()),
                },
                preview_status(false),
            ),
        ];
        for (name, view, status) in screens {
            let mut canvas = Canvas::in_memory(1024, 768);
            ui::draw(
                &mut canvas,
                &mut fonts,
                &theme,
                &view,
                &status,
                None,
                None,
                0.35,
            );
            png::write_png(
                &format!("{dir}/{name}.png"),
                1024,
                768,
                &canvas.snapshot_rgb(),
            )
            .unwrap();
        }

        let modal = Modal {
            title: "Forget MacBook Pro?".into(),
            message: "You will need to pair it again from the host.".into(),
            confirm: "Forget",
            destructive: true,
            loading: false,
        };
        let mut canvas = Canvas::in_memory(1024, 768);
        ui::draw(
            &mut canvas,
            &mut fonts,
            &theme,
            &View::Menu {
                rows: preview_menu_rows(true),
            },
            &preview_status(true),
            Some(&modal),
            None,
            0.35,
        );
        png::write_png(
            &format!("{dir}/confirm.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();

        let connecting = Modal {
            title: "Connecting to MacBook Pro".into(),
            message: String::new(),
            confirm: "",
            destructive: false,
            loading: true,
        };
        let mut canvas = Canvas::in_memory(1024, 768);
        ui::draw(
            &mut canvas,
            &mut fonts,
            &theme,
            &View::Menu {
                rows: preview_menu_rows(true),
            },
            &preview_status(true),
            Some(&connecting),
            None,
            0.35,
        );
        png::write_png(
            &format!("{dir}/connecting.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();

        let controller = View::Controller {
            host: "MacBook Pro".into(),
            profile: "PC",
        };
        let mut canvas = Canvas::in_memory(1024, 768);
        ui::draw(
            &mut canvas,
            &mut fonts,
            &theme,
            &controller,
            &preview_status(true),
            None,
            Some(0.72),
            0.35,
        );
        png::write_png(
            &format!("{dir}/hold.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
    }

    #[test]
    fn the_dpad_maps_to_one_direction_at_a_time() {
        assert_eq!(nav_dir(hid::HAT_CENTRED, 2), Some(Dir::Right));
        assert_eq!(nav_dir(2, 3), Some(Dir::Down));
        assert_eq!(nav_dir(1, 2), Some(Dir::Right));
        assert_eq!(nav_dir(1, 0), None);
        assert_eq!(nav_dir(0, hid::HAT_CENTRED), None);
    }

    #[test]
    fn cursors_wrap_and_an_empty_list_is_safe() {
        assert_eq!(move_cursor(0, 3, 1), 1);
        assert_eq!(move_cursor(2, 3, 1), 0);
        assert_eq!(move_cursor(0, 3, -1), 2);
        assert_eq!(move_cursor(4, 0, 1), 0);
    }

    #[test]
    fn the_menu_keeps_pairing_while_a_host_is_connected() {
        let settings = Settings {
            hosts: vec![bluez::Host {
                addr: Addr([0x11; 6]),
                name: "Studio Mac".into(),
                connected: true,
            }],
            profiles: HashMap::new(),
            pairing: Profile::Pc,
            error: None,
        };
        let rows = settings.rows();
        assert!(rows.iter().any(|row| matches!(row, MenuRow::Host(0))));
        assert_eq!(rows.last(), Some(&MenuRow::Pair));
    }

    #[test]
    fn a_profile_key_round_trips() {
        for profile in [Profile::Pc, Profile::Xbox] {
            assert_eq!(Profile::from_key(profile.key()), Some(profile));
        }
        assert_eq!(Profile::from_key(" xbox\n"), Some(Profile::Xbox));
        assert_eq!(Profile::from_key("PC"), Some(Profile::Pc));
        assert_eq!(Profile::from_key("ds4"), None);
    }
}
