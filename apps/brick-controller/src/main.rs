// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Brick Controller: the TrimUI Brick as a standard Bluetooth HID gamepad.
//!
//! Two identities: a generic PC gamepad, and an Xbox Wireless Controller for
//! XInput. The choice is kept per host and gives the host an identity it
//! already knows. The screen says what state the link is in and lights each
//! control as it is pressed; it dims itself once a host is connected.

mod bluez;
use brick::canvas;
mod hid;
mod l2cap;
mod pad;
#[cfg(test)]
use brick::png;
mod rumble;
mod sdp;
mod xbox;
use brick::text;

use canvas::{Canvas, Rgb};
use l2cap::{Addr, Channel, Listener};
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use text::Fonts;

const NAME: &str = "Brick Controller";
/// Who made it, and where it lives: on screen, so the credit travels with
/// the app. The Padpod credit is required by its license.
const PADPOD_CREDIT: &str = "Padpod by ligt";
const HOME_PAGE: &str = "github.com/duyquang6/padpod";
/// The release, as `package.sh` builds it; "dev" for any other build.
const VERSION: &str = match option_env!("BRICK_CONTROLLER_VERSION") {
    Some(v) => v,
    None => "0.1.0",
};
const DATA_DIR: &str = "/mnt/SDCARD/.userdata/tg5040/brick-controller";
const THEME_FONT: &str = "/mnt/SDCARD/Themes/SPRUCE/nunwen.ttf";

/// Holding F1 and F2 together this long quits.
const QUIT_CHORD_HOLD: Duration = Duration::from_secs(3);
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
// The look follows spruce's SPRUCE theme (Gruvbox), as the handheld's other
// native apps do: black, cream text, an orange accent.
const BG: Rgb = Rgb(0, 0, 0);
/// The title, and the rule under it.
const TITLE: Rgb = Rgb(0xEB, 0xDB, 0xB2);
const FG: Rgb = Rgb(0xFB, 0xF1, 0xC7);
/// Secondary text, still bright enough to read at a glance.
const DIM: Rgb = Rgb(0xBD, 0xAE, 0x93);
/// What is pressed, chosen or on.
const ACCENT: Rgb = Rgb(0xD6, 0x5D, 0x0E);
/// The menu's cursor: the accent, dimmed.
const CURSOR: Rgb = Rgb(0x6B, 0x2F, 0x07);
/// The rule under the title: the title, dimmed.
const RULE: Rgb = Rgb(0x76, 0x6E, 0x59);
const OK: Rgb = Rgb(0xB8, 0xBB, 0x26);
const WARN: Rgb = Rgb(0xFA, 0xBD, 0x2F);
const ERROR: Rgb = Rgb(0xFB, 0x49, 0x34);
/// Every control's resting colour. The panel shows dark greys darker than a
/// monitor does: at (40, 44, 56) on the body the D-pad and sticks could not
/// be made out on the device.
const CHIP: Rgb = Rgb(0x66, 0x5C, 0x54);
/// A stick's cap, lighter than its well so where it points can be seen.
const KNOB: Rgb = Rgb(0xBD, 0xAE, 0x93);
const WELL: Rgb = Rgb(0x3C, 0x38, 0x36);

// The layout: the title and a rule along the top, the content below, the
// buttons' hints along the bottom.
const LEFT: i32 = 28;
const TITLE_BASELINE: i32 = 52;
const TITLE_SIZE: f32 = 40.0;
const RULE_Y: i32 = 70;
const CONTENT_TOP: i32 = 116;
const TEXT_SIZE: f32 = 28.0;
const BOTTOM_BASELINE: i32 = 744;
const HINT_SIZE: f32 = 26.0;

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

/// The Brick standard has no analog sticks; the Brick Pro and the Smart Pro do.
/// NextUI passes the model in `DEVICE`, so the status screen can match the
/// hardware without probing the input device.
fn has_sticks() -> bool {
    std::env::var("DEVICE")
        .map(|device| device != "brick")
        .unwrap_or(true)
}

/// The HID identity used with a host. A host remembers the controller type it
/// paired with, so the choice is stored per host and applied when connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    Pc,
    Xbox,
}

impl Profile {
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

#[derive(Clone)]
enum Status {
    Starting,
    Failed(String),
    /// `reconnecting` names the host being reconnected to.
    Waiting {
        reconnecting: Option<String>,
    },
    Connected {
        name: String,
    },
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
struct Frame {
    profile: Profile,
    status: Status,
    state: hid::State,
    chord_held: bool,
    settings: Option<Settings>,
    monitor: Option<InputMonitor>,
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
/// gamepad is what has to be quick.
const FRAME_EVERY: Duration = Duration::from_millis(33);

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

impl Renderer {
    fn spawn(mut canvas: Canvas, mut fonts: Fonts) -> Self {
        let slot: Arc<(Mutex<Option<Option<Frame>>>, std::sync::Condvar)> = Arc::default();
        let shared = Arc::clone(&slot);
        let thread = std::thread::spawn(move || {
            // Below the I/O thread, so a frame never delays a report.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, current_tid() as _, 10) };
            let (lock, ready) = &*shared;
            let mut drawn = 0u32;
            loop {
                let next = {
                    let mut pending = lock.lock().unwrap();
                    while pending.is_none() {
                        pending = ready.wait(pending).unwrap();
                    }
                    pending.take().unwrap()
                };
                let started = Instant::now();
                match next {
                    None => {
                        canvas.clear(Rgb(0, 0, 0));
                        let _ = canvas.flush();
                        return;
                    }
                    Some(frame) => {
                        draw(
                            &mut canvas,
                            &mut fonts,
                            frame.profile,
                            &frame.status,
                            &frame.state,
                            frame.chord_held,
                        );
                        let connected = matches!(&frame.status, Status::Connected { .. });
                        if let Some(settings) = frame.settings {
                            draw_settings(
                                &mut canvas,
                                &mut fonts,
                                &settings,
                                connected,
                                frame.chord_held,
                            );
                        } else if let Some(monitor) = frame.monitor {
                            draw_input_monitor(&mut canvas, &mut fonts, &monitor, frame.chord_held);
                        }
                        let _ = canvas.flush();
                        // A few, to know what a frame costs on the device.
                        drawn += 1;
                        if drawn <= 5 {
                            eprintln!(
                                "screen: frame {drawn} took {} ms",
                                started.elapsed().as_millis()
                            );
                        }
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
        return;
    }
    if let Err(e) = run() {
        eprintln!("brick-controller: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let canvas = Canvas::open("/dev/fb0").map_err(|e| format!("framebuffer: {e}"))?;
    let fonts = Fonts::load(&[THEME_FONT.to_string()]);
    let mut pad = pad::Pad::open(&pad::pad_path()).map_err(|e| format!("pad: {e}"))?;
    let screen = Renderer::spawn(canvas, fonts);
    let mut profile = saved_host().map(profile_for).unwrap_or_else(last_profile);
    let mut open_settings = false;
    loop {
        save_last_profile(profile);
        match session(profile, open_settings, &screen, &mut pad)? {
            SessionOutcome::Quit => break,
            SessionOutcome::SwitchProfile(next) => {
                profile = next;
                // A profile change comes from the settings screen; keep it
                // there across the Bluetooth restart.
                open_settings = true;
                eprintln!("profile: {}", profile.label());
            }
        }
    }
    drop(screen);
    Ok(())
}

fn frame(profile: Profile, status: Status, state: hid::State, chord_held: bool) -> Frame {
    Frame {
        profile,
        status,
        state,
        chord_held,
        settings: None,
        monitor: None,
    }
}

/// The settings screen F1 opens: the paired hosts and their link state.
/// Start and Select stay available to the host; this screen is navigated with
/// the D-pad and A/B.
#[derive(Clone, PartialEq, Eq)]
struct Settings {
    hosts: Vec<bluez::Host>,
    profiles: HashMap<Addr, Profile>,
    /// The identity a new host would pair with, when no host is known yet.
    pairing: Profile,
    cursor: usize,
    error: Option<String>,
}

type DrawKey = (
    Profile,
    String,
    hid::State,
    Option<Settings>,
    Option<(usize, Option<String>)>,
    bool,
);

#[derive(Clone)]
struct InputMonitor {
    started: Instant,
    lines: Vec<String>,
}

impl InputMonitor {
    const MAX_LINES: usize = 14;

    fn new() -> Self {
        Self {
            started: Instant::now(),
            lines: Vec::new(),
        }
    }

    fn push(&mut self, line: impl Into<String>) {
        let line = format!(
            "{:>6.1}s  {}",
            self.started.elapsed().as_secs_f32(),
            line.into()
        );
        self.lines.push(line);
        if self.lines.len() > Self::MAX_LINES {
            self.lines.remove(0);
        }
    }
}

fn hat_label(hat: u8) -> &'static str {
    match hat {
        0 => "up",
        1 => "up-right",
        2 => "right",
        3 => "down-right",
        4 => "down",
        5 => "down-left",
        6 => "left",
        7 => "up-left",
        _ => "centred",
    }
}

fn record_input(
    monitor: &mut InputMonitor,
    was: &hid::State,
    now: &hid::State,
    was_f1: bool,
    f1: bool,
    was_f2: bool,
    f2: bool,
) {
    use hid::button;
    let button = |monitor: &mut InputMonitor, number: u8, label: &str| {
        if now.pressed(number) && !was.pressed(number) {
            monitor.push(format!("{label} down"));
        } else if was.pressed(number) && !now.pressed(number) {
            monitor.push(format!("{label} up"));
        }
    };
    button(monitor, button::SOUTH, "B");
    button(monitor, button::EAST, "A");
    button(monitor, button::NORTH, "X");
    button(monitor, button::WEST, "Y");
    button(monitor, button::L1, "L1");
    button(monitor, button::R1, "R1");
    button(monitor, button::SELECT, "SELECT");
    button(monitor, button::START, "START");
    button(monitor, button::HOME, "MENU");
    if was.triggers[0] != now.triggers[0] {
        monitor.push(format!("L2 {}", now.triggers[0]));
    }
    if was.triggers[1] != now.triggers[1] {
        monitor.push(format!("R2 {}", now.triggers[1]));
    }
    if was.hat != now.hat {
        monitor.push(format!("D-pad {}", hat_label(now.hat)));
    }
    if was.sticks != now.sticks {
        monitor.push(format!(
            "sticks {} {} {} {}",
            now.sticks[0], now.sticks[1], now.sticks[2], now.sticks[3]
        ));
    }
    if f1 != was_f1 {
        monitor.push(if f1 { "F1 down" } else { "F1 up" });
    }
    if f2 != was_f2 {
        monitor.push(if f2 { "F2 down" } else { "F2 up" });
    }
}

impl Settings {
    fn load(bus: &bluez::Bus, pairing: Profile) -> Self {
        match bus.paired_hosts() {
            Ok(hosts) => {
                eprintln!("settings: {} paired host(s)", hosts.len());
                Self {
                    hosts,
                    profiles: load_profiles(),
                    pairing,
                    cursor: 0,
                    error: None,
                }
            }
            Err(error) => {
                eprintln!("settings: {error}");
                Self {
                    hosts: Vec::new(),
                    profiles: load_profiles(),
                    pairing,
                    cursor: 0,
                    error: Some(error),
                }
            }
        }
    }

    fn selected(&self) -> Option<&bluez::Host> {
        self.hosts.get(self.cursor)
    }

    /// With no paired host, the only row chooses the identity for the next
    /// pairing, so a host can be paired as Xbox from the start. A BlueZ error
    /// leaves no usable row.
    fn has_pairing_row(&self) -> bool {
        self.hosts.is_empty() && self.error.is_none()
    }

    fn at_pairing(&self) -> bool {
        self.has_pairing_row() && self.cursor == self.hosts.len()
    }

    fn rows(&self) -> usize {
        self.hosts.len() + usize::from(self.has_pairing_row())
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.rows();
        if len == 0 {
            return;
        }
        self.cursor = ((self.cursor as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn profile_for(&self, address: Addr) -> Profile {
        self.profiles.get(&address).copied().unwrap_or(Profile::Pc)
    }

    fn toggle_profile(&mut self, address: Addr) -> Profile {
        let next = match self.profile_for(address) {
            Profile::Pc => Profile::Xbox,
            Profile::Xbox => Profile::Pc,
        };
        self.profiles.insert(address, next);
        save_profiles(&self.profiles);
        next
    }

    fn toggle_pairing(&mut self) -> Profile {
        self.pairing = match self.pairing {
            Profile::Pc => Profile::Xbox,
            Profile::Xbox => Profile::Pc,
        };
        save_last_profile(self.pairing);
        self.pairing
    }
}

enum SessionOutcome {
    Quit,
    SwitchProfile(Profile),
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

/// Bluetooth up as the chosen controller, until F1+F2 is held or another host
/// profile is selected.
fn session(
    profile: Profile,
    start_in_settings: bool,
    screen: &Renderer,
    pad: &mut pad::Pad,
) -> Result<SessionOutcome, String> {
    if !start_in_settings {
        screen.show(frame(profile, Status::Starting, pad.mapper.state, false));
    }

    let daemon = match bluez::Daemon::take_over(profile.bt_name(), profile.device_id()) {
        Ok(d) => d,
        Err(e) => return fail(profile, screen, pad, e),
    };
    let bus = match bluez::Bus::open(&sdp::record(profile.bt_name(), profile.descriptor())) {
        Ok(b) => b,
        Err(e) => return fail(profile, screen, pad, e),
    };
    let _ = bus.set_alias(profile.bt_name());
    let listeners =
        Listener::bind(sdp::PSM_CONTROL).and_then(|c| Ok((c, Listener::bind(sdp::PSM_INTERRUPT)?)));
    let (control_listener, interrupt_listener) = match listeners {
        Ok(pair) => pair,
        Err(e) => {
            return fail(
                profile,
                screen,
                pad,
                format!("could not open the HID channels: {e}"),
            )
        }
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

    let target = Arc::new(Mutex::new(saved_host()));
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
    // A restart can happen while a key is held (F2 switches a profile). Start
    // from the real state so that key is not seen as a fresh press.
    let mut previous_f1 = pad.mapper.f1;
    let mut previous_f2 = pad.mapper.f2;
    let mut chord_since: Option<Instant> = None;
    let mut chord_used = false;
    let mut settings: Option<Settings> = start_in_settings.then(|| Settings::load(&bus, profile));
    let mut monitor: Option<InputMonitor> = None;
    // The profile to switch to. A switch always comes from settings, so the
    // new session reopens it.
    let mut switch_to: Option<Profile> = None;
    let mut drawn: Option<DrawKey> = None;
    let mut names: std::collections::HashMap<Addr, String> = Default::default();

    let outcome = loop {
        // Discoverable only while nothing is connected: a gamepad in use has
        // no reason to answer every scan in the room.
        let want_discoverable = link.is_none();
        if discoverable != Some(want_discoverable) {
            if let Err(e) = bus.set_discoverable(want_discoverable) {
                eprintln!("bluez: {e}");
            }
            discoverable = Some(want_discoverable);
        }

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
        // everything else waits for something to happen.
        let wait = if link.is_some() && Some(pad.mapper.state) != sent {
            REPORT_GAP.saturating_sub(sent_at.elapsed()).as_millis() as i32
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

        // New connections: the host opens control first, then interrupt.
        if let Ok(Some((control, host))) = control_listener.accept() {
            eprintln!("link: control channel from {host}");
            half = Some((control, host));
        }
        if let Ok(Some((interrupt, host))) = interrupt_listener.accept() {
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
        if let Ok((control, interrupt, host)) = reconnected.try_recv() {
            if link.is_none() {
                link = Some(Link {
                    control,
                    interrupt,
                    name: String::new(),
                    host,
                });
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
            settings = None;
            monitor = None;
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
                    Ok(Some(message)) => match hid::control(&message, &pad.mapper.state) {
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
            let report_state = if settings.is_some() || monitor.is_some() {
                hid::State::default()
            } else {
                pad.mapper.state
            };
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
            connected.store(false, Ordering::Relaxed);
            // A host that goes away must not leave the motor running.
            if let Some(motor) = &motor {
                motor.set(0);
            }
            // The host list now shows the new state; the controller view is
            // only for a connected host.
            settings = None;
            monitor = None;
            drawn = None;
        }

        // The controller view is only meaningful with a host. While nothing
        // is connected the settings screen is the app's screen.
        if link.is_none() {
            monitor = None;
            if settings.is_none() {
                settings = Some(Settings::load(&bus, profile));
                drawn = None;
            }
        }

        // F1 opens settings, F2 opens the input monitor. F1 fires on release
        // so a short F1+F2 chord never opens either screen.
        let now = pad.mapper.state;
        let f1_now = pad.mapper.f1;
        let f2_now = pad.mapper.f2;
        let f1_released = previous_f1 && !f1_now;
        let f2_pressed = f2_now && !previous_f2;
        if f1_now && f2_now {
            chord_used = true;
            let since = chord_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= QUIT_CHORD_HOLD {
                if let Some(l) = &link {
                    send_neutral(l, profile);
                }
                break SessionOutcome::Quit;
            }
        } else {
            // A released key breaks the chord: the next hold starts its own
            // timer. `chord_used` stays set until both are up, so a release
            // in this frame cannot trigger F1 or F2.
            chord_since = None;
        }
        if f1_released && !chord_used && link.is_some() {
            settings = match settings {
                Some(_) => None,
                None => {
                    monitor = None;
                    Some(Settings::load(&bus, profile))
                }
            };
            drawn = None;
        }
        if f2_pressed && !f1_now && !chord_used && settings.is_none() && link.is_some() {
            monitor = if monitor.is_some() {
                None
            } else {
                Some(InputMonitor::new())
            };
            drawn = None;
        }
        if let Some(entry) = monitor.as_mut() {
            record_input(
                entry,
                &previous,
                &now,
                previous_f1,
                f1_now,
                previous_f2,
                f2_now,
            );
        }
        if f1_now != previous_f1 {
            eprintln!("ui: f1 {f1_now}");
        }
        if f2_now != previous_f2 {
            eprintln!("ui: f2 {f2_now}");
        }
        previous_f1 = f1_now;
        previous_f2 = f2_now;

        // Settings navigation: D-pad moves, A connects/disconnects, F2 switches
        // the selected host between the PC and Xbox identities. The quit chord
        // keeps navigation quiet while it is held.
        if let Some(open) = settings.as_mut().filter(|_| !chord_used) {
            let hat_to = |dir: u8| now.hat == dir && previous.hat != dir;
            if hat_to(0) {
                open.move_cursor(-1);
            } else if hat_to(4) {
                open.move_cursor(1);
            } else if f2_pressed && !f1_now && !chord_used {
                if open.at_pairing() {
                    let next = open.toggle_pairing();
                    eprintln!("settings: next pairing {}", next.label());
                    switch_to = Some(next);
                } else if let Some(host) = open.selected().cloned() {
                    let next = open.toggle_profile(host.addr);
                    eprintln!("settings: {} profile {}", host.addr, next.label());
                    let is_current = link.as_ref().is_some_and(|l| l.host == host.addr)
                        || *target.lock().unwrap() == Some(host.addr);
                    if is_current {
                        // A host caches the identity it first paired with. The
                        // old link key would let it come back with the previous
                        // identity, so forget the pairing here too and let the
                        // user pair afresh under the new name.
                        if let Some(l) = link.take() {
                            let _ = bus.disconnect(l.host);
                        }
                        if !bus.remove(host.addr) {
                            eprintln!("settings: could not forget {}", host.addr);
                        }
                        save_host(None);
                        *target.lock().unwrap() = None;
                        connected.store(false, Ordering::Relaxed);
                        switch_to = Some(next);
                    }
                }
            } else if now.pressed(hid::button::EAST) && !previous.pressed(hid::button::EAST) {
                if let Some(host) = open.selected().cloned() {
                    if host.connected {
                        eprintln!("settings: disconnect {}", host.addr);
                        if let Some(l) = link.take() {
                            let _ = bus.disconnect(l.host);
                        } else {
                            let _ = bus.disconnect(host.addr);
                        }
                        save_host(None);
                        *target.lock().unwrap() = None;
                        connected.store(false, Ordering::Relaxed);
                        if let Some(motor) = &motor {
                            motor.set(0);
                        }
                        settings = None;
                    } else {
                        let desired = open.profile_for(host.addr);
                        eprintln!("settings: connect {} as {}", host.addr, desired.label());
                        if let Some(l) = link.take() {
                            let _ = bus.disconnect(l.host);
                            connected.store(false, Ordering::Relaxed);
                        }
                        bus.trust(host.addr);
                        save_host(Some(host.addr));
                        *target.lock().unwrap() = Some(host.addr);
                        if desired != profile {
                            switch_to = Some(desired);
                        }
                    }
                }
            }
        }
        if let Some(next) = switch_to {
            break SessionOutcome::SwitchProfile(next);
        }
        // A key may have cleared the settings view while the host is gone;
        // without a connection, settings is the only screen.
        if link.is_none() && settings.is_none() {
            settings = Some(Settings::load(&bus, profile));
            drawn = None;
        }
        if !f1_now && !f2_now {
            chord_used = false;
        }
        previous = now;

        // While settings or the input monitor is open the host gets a neutral
        // report, so navigation cannot press buttons in the game.
        let report_state = if settings.is_some() || monitor.is_some() {
            hid::State::default()
        } else {
            pad.mapper.state
        };
        let status = match &link {
            Some(l) => Status::Connected {
                name: l.name.clone(),
            },
            None => {
                // By name, as the host announced itself; asked of BlueZ once.
                let reconnecting = (*target.lock().unwrap()).map(|a| {
                    names
                        .entry(a)
                        .or_insert_with(|| bus.name_of(a).unwrap_or_else(|| a.to_string()))
                        .clone()
                });
                Status::Waiting { reconnecting }
            }
        };
        let chord_held = f1_now && f2_now;
        let monitor_key = monitor
            .as_ref()
            .map(|monitor| (monitor.lines.len(), monitor.lines.last().cloned()));
        let key = (
            profile,
            status_key(&status),
            report_state,
            settings.clone(),
            monitor_key,
            chord_held,
        );
        if drawn.as_ref() != Some(&key) {
            screen.show(Frame {
                settings: settings.clone(),
                monitor: monitor.clone(),
                ..frame(profile, status, pad.mapper.state, chord_held)
            });
            drawn = Some(key);
        }
    };

    drop(link);
    let _ = bus.set_discoverable(false);
    drop(bus);
    drop(daemon);
    Ok(outcome)
}

fn status_key(status: &Status) -> String {
    match status {
        Status::Starting => "starting".into(),
        Status::Failed(e) => format!("failed {e}"),
        Status::Waiting { reconnecting } => format!("waiting {reconnecting:?}"),
        Status::Connected { name } => format!("connected {name}"),
    }
}

/// Show an error and wait for MENU or B, so it can be read before the app
/// closes.
fn fail(
    profile: Profile,
    screen: &Renderer,
    pad: &mut pad::Pad,
    error: String,
) -> Result<SessionOutcome, String> {
    eprintln!("brick-controller: {error}");
    screen.show(Frame {
        profile,
        status: Status::Failed(error.clone()),
        state: pad.mapper.state,
        chord_held: false,
        settings: None,
        monitor: None,
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let mut fd = libc::pollfd {
            fd: pad.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut fd, 1, 200) };
        if fd.revents & libc::POLLIN != 0 && pad.read().is_ok() {
            let s = &pad.mapper.state;
            if s.pressed(hid::button::HOME) || s.pressed(hid::button::SOUTH) {
                break;
            }
        }
    }
    drop(error);
    Ok(SessionOutcome::Quit)
}

#[derive(Clone, Copy)]
enum Control {
    /// A hat direction, true for it and for the two diagonals beside it.
    Hat(u8),
}

fn lit(control: Control, state: &hid::State) -> bool {
    match control {
        Control::Hat(dir) => {
            state.hat != hid::HAT_CENTRED
                && matches!((state.hat as i32 - dir as i32).rem_euclid(8), 0 | 1 | 7)
        }
    }
}

/// Coverage for a rounded rectangle, anti-aliased on its edge. A circle is
/// the case where the radius is half the side.
fn round_rect_mask(w: i32, h: i32, r: i32) -> Vec<u8> {
    let (w, h) = (w.max(1), h.max(1));
    let r = (r as f32).min(w as f32 / 2.0).min(h as f32 / 2.0);
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut mask = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let qx = ((x as f32 + 0.5 - hw).abs() - (hw - r)).max(0.0);
            let qy = ((y as f32 + 0.5 - hh).abs() - (hh - r)).max(0.0);
            let d = (qx * qx + qy * qy).sqrt() - r;
            mask.push(((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    mask
}

fn round_rect(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, r: i32, colour: Rgb) {
    // The same few shapes every frame; their masks are worked out once.
    thread_local! {
        static MASKS: std::cell::RefCell<std::collections::HashMap<(i32, i32, i32), Vec<u8>>> = Default::default();
    }
    MASKS.with(|masks| {
        let mut masks = masks.borrow_mut();
        let mask = masks
            .entry((w, h, r))
            .or_insert_with(|| round_rect_mask(w, h, r));
        canvas.blend_mask(x, y, w.max(1) as usize, mask, colour);
    });
}

fn circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, colour: Rgb) {
    round_rect(canvas, cx - r, cy - r, 2 * r, 2 * r, r, colour);
}

const BODY: Rgb = Rgb(0x28, 0x28, 0x28);
const BODY_EDGE: Rgb = Rgb(0x50, 0x49, 0x45);
const SHOULDER_BACK: Rgb = Rgb(0x50, 0x49, 0x45);

/// A shoulder button, `visible` pixels of it above the body: the front one
/// (L1/R1) solid, the back one (L2/R2) filling up with how far its trigger is
/// pulled. The tab runs on under the body, which is drawn over it.
// The extra coordinates are the drawing API; grouping them would only move the
// same arguments into a struct at every call site.
#[allow(clippy::too_many_arguments)]
fn shoulder(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    x: i32,
    y: i32,
    w: i32,
    visible: i32,
    label: &str,
    on: bool,
    travel: Option<u8>,
) {
    let base = if travel.is_some() {
        SHOULDER_BACK
    } else {
        CHIP
    };
    round_rect(
        canvas,
        x,
        y,
        w,
        visible + 40,
        18,
        if on { ACCENT } else { base },
    );
    if let (Some(t), false) = (travel, on) {
        let filled = visible * t as i32 / 255;
        if filled > 0 {
            canvas.fill_rect(x + 10, y + visible - filled, w - 20, filled, CURSOR);
        }
    }
    fonts.draw_centred(
        canvas,
        label,
        x + w / 2,
        y + visible / 2 + 9,
        26.0,
        if on { BG } else { FG },
    );
}

fn face_button(canvas: &mut Canvas, fonts: &mut Fonts, cx: i32, cy: i32, label: &str, on: bool) {
    circle(canvas, cx, cy, 44, if on { ACCENT } else { CHIP });
    fonts.draw_centred(canvas, label, cx, cy + 12, 34.0, if on { BG } else { FG });
}

fn pill(canvas: &mut Canvas, fonts: &mut Fonts, cx: i32, cy: i32, w: i32, label: &str, on: bool) {
    round_rect(
        canvas,
        cx - w / 2,
        cy - 20,
        w,
        40,
        20,
        if on { ACCENT } else { CHIP },
    );
    fonts.draw_centred(canvas, label, cx, cy + 8, 20.0, if on { BG } else { FG });
}

/// The D-pad as a cross; each arm lights for its direction and the
/// diagonals beside it.
/// An analog stick: its well, and the cap shown where the stick is pushed.
fn stick(canvas: &mut Canvas, cx: i32, cy: i32, x: u8, y: u8, clicked: bool) {
    circle(canvas, cx, cy, 54, CHIP);
    circle(canvas, cx, cy, 48, WELL);
    let offset = |v: u8| (v as i32 - 128) * 26 / 128;
    circle(
        canvas,
        cx + offset(x),
        cy + offset(y),
        30,
        if clicked { ACCENT } else { KNOB },
    );
}

/// The title and the rule under it, with a short reading at the right of
/// the title's line.
fn header(canvas: &mut Canvas, fonts: &mut Fonts, title: &str, corner: &str, corner_colour: Rgb) {
    let w = canvas.width();
    fonts.draw(canvas, title, LEFT, TITLE_BASELINE, TITLE_SIZE, TITLE);
    let size = TITLE_SIZE * 0.75;
    let room = w - LEFT * 2 - fonts.measure(title, TITLE_SIZE) - 24;
    let corner = fonts.ellipsize(corner, size, room);
    let cw = fonts.measure(&corner, size);
    fonts.draw(
        canvas,
        &corner,
        w - LEFT - cw,
        TITLE_BASELINE,
        size,
        corner_colour,
    );
    canvas.fill_rect(LEFT, RULE_Y, w - LEFT * 2, 2, RULE);
}

/// Button hints along the bottom - each key in the accent, what it does
/// beside it - and the credit at the right.
fn footer(canvas: &mut Canvas, fonts: &mut Fonts, hints: &[(&str, &str)], credit: &str) {
    let w = canvas.width();
    let mut x = LEFT;
    for (i, (key, does)) in hints.iter().enumerate() {
        if i > 0 {
            x += 26;
        }
        x = fonts.draw(canvas, key, x, BOTTOM_BASELINE, HINT_SIZE, ACCENT) + 8;
        x = fonts.draw(canvas, does, x, BOTTOM_BASELINE, HINT_SIZE, DIM);
    }
    let size = HINT_SIZE * 0.85;
    let credit = fonts.ellipsize(credit, size, w - LEFT - x - 32);
    let cw = fonts.measure(&credit, size);
    fonts.draw(canvas, &credit, w - LEFT - cw, BOTTOM_BASELINE, size, DIM);
}

/// Lines of text under the rule: the first in the text colour, the rest
/// dimmer. `who`, if the first line names it, is picked out in green: the
/// host connected, the one thing worth finding at a glance.
fn lines(canvas: &mut Canvas, fonts: &mut Fonts, text: &[String], who: Option<&str>) {
    let w = canvas.width();
    let mut y = CONTENT_TOP;
    for (i, line) in text.iter().enumerate() {
        if let Some((who, (before, after))) = who
            .filter(|_| i == 0)
            .and_then(|who| Some((who, line.split_once(who)?)))
        {
            let x = fonts.draw(canvas, before, LEFT, y, TEXT_SIZE, FG);
            // A long name gives way, so the line stays on one row.
            let room = w - LEFT - x - fonts.measure(after, TEXT_SIZE);
            let who = fonts.ellipsize(who, TEXT_SIZE, room);
            let x = fonts.draw(canvas, &who, x, y, TEXT_SIZE, OK);
            fonts.draw(canvas, after, x, y, TEXT_SIZE, FG);
            y += 36;
            continue;
        }
        for wrapped in fonts.wrap(line, TEXT_SIZE, w - LEFT * 2, 2) {
            fonts.draw(
                canvas,
                &wrapped,
                LEFT,
                y,
                TEXT_SIZE,
                if i == 0 { FG } else { DIM },
            );
            y += 36;
        }
    }
}

/// A line `width` wide from one point to another, as a run of dots: enough
/// for the few short strokes of the face buttons' symbols.
fn stroke(
    canvas: &mut Canvas,
    (x0, y0): (i32, i32),
    (x1, y1): (i32, i32),
    width: i32,
    colour: Rgb,
) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        circle(
            canvas,
            x0 + (x1 - x0) * i / steps,
            y0 + (y1 - y0) * i / steps,
            width / 2,
            colour,
        );
    }
}

/// A round face button `r` across; returns the colour to mark it in.
fn round_button(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, on: bool) -> Rgb {
    circle(canvas, cx, cy, r, if on { ACCENT } else { CHIP });
    if on {
        BG
    } else {
        FG
    }
}

/// The D-pad as a cross, `arm` wide and reaching `reach` from its centre;
/// each arm lights for its direction and the diagonals beside it.
fn dpad_sized(canvas: &mut Canvas, cx: i32, cy: i32, arm: i32, reach: i32, state: &hid::State) {
    let r = arm / 5;
    round_rect(canvas, cx - arm / 2, cy - reach, arm, 2 * reach, r, CHIP);
    round_rect(canvas, cx - reach, cy - arm / 2, 2 * reach, arm, r, CHIP);
    let len = reach - arm / 2 - 4;
    let arms = [
        (0, cx - arm / 2, cy - reach, arm, len),
        (4, cx - arm / 2, cy + arm / 2 + 4, arm, len),
        (6, cx - reach, cy - arm / 2, len, arm),
        (2, cx + arm / 2 + 4, cy - arm / 2, len, arm),
    ];
    for (dir, x, y, w, h) in arms {
        if lit(Control::Hat(dir), state) {
            round_rect(canvas, x, y, w, h, r, ACCENT);
        }
    }
    circle(canvas, cx, cy, arm / 5, WELL);
}

/// The face of an Xbox-style controller: an offset layout, the left stick
/// above the D-pad and the face buttons above the right stick. Drawn plain,
/// with no maker's logo: the Home button is a plain disc.
#[allow(dead_code)]
fn xbox_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    // Bumpers in front, triggers behind them; the body overlaps both.
    shoulder(
        canvas,
        fonts,
        210,
        196,
        150,
        60,
        "LT",
        state.pressed(button::L2),
        Some(state.triggers[0]),
    );
    shoulder(
        canvas,
        fonts,
        664,
        196,
        150,
        60,
        "RT",
        state.pressed(button::R2),
        Some(state.triggers[1]),
    );
    shoulder(
        canvas,
        fonts,
        150,
        236,
        220,
        56,
        "LB",
        state.pressed(button::L1),
        None,
    );
    shoulder(
        canvas,
        fonts,
        654,
        236,
        220,
        56,
        "RB",
        state.pressed(button::R1),
        None,
    );
    // The body: a wide top and two grips.
    for (grow, colour) in [(3, BODY_EDGE), (0, BODY)] {
        round_rect(
            canvas,
            110 - grow,
            280 - grow,
            804 + 2 * grow,
            270 + 2 * grow,
            130 + grow,
            colour,
        );
        circle(canvas, 250, 548, 140 + grow, colour);
        circle(canvas, 774, 548, 140 + grow, colour);
    }
    // Plain disc for Home, View and Menu beside it.
    circle(
        canvas,
        512,
        330,
        30,
        if state.pressed(button::HOME) {
            ACCENT
        } else {
            CHIP
        },
    );
    pill(
        canvas,
        fonts,
        440,
        410,
        70,
        "VIEW",
        state.pressed(button::SELECT),
    );
    pill(
        canvas,
        fonts,
        584,
        410,
        70,
        "MENU",
        state.pressed(button::START),
    );
    let [lx, ly, rx, ry] = state.sticks;
    stick(canvas, 290, 395, lx, ly, state.pressed(button::L3));
    dpad_sized(canvas, 390, 545, 54, 82, state);
    stick(canvas, 634, 545, rx, ry, state.pressed(button::R3));
    // A, B, X, Y by position, as Xbox games read them: the bottom button is
    // A whatever the handheld prints on it.
    let (fx, fy, gap) = (744, 395, 66);
    for (label, x, y, n) in [
        ("Y", fx, fy - gap, button::NORTH),
        ("X", fx - gap, fy, button::WEST),
        ("B", fx + gap, fy, button::EAST),
        ("A", fx, fy + gap, button::SOUTH),
    ] {
        let on = state.pressed(n);
        let ink = round_button(canvas, x, y, 34, on);
        fonts.draw_centred(
            canvas,
            label,
            x,
            y + 11,
            32.0,
            if on { ink } else { letter_colour(label) },
        );
    }
}

/// The colours Xbox players know the letters by, in the theme's own
/// shades.
fn letter_colour(label: &str) -> Rgb {
    match label {
        "A" => OK,
        "B" => ERROR,
        "X" => Rgb(0x83, 0xA5, 0x98),
        _ => WARN,
    }
}

/// The face of a DualShock-style controller: both sticks low and side by
/// side, a touchpad between the D-pad and the face buttons, which carry
/// shapes rather than letters. The Home button is a plain disc.
#[allow(dead_code)]
fn ds4_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    shoulder(
        canvas,
        fonts,
        180,
        196,
        160,
        60,
        "L2",
        state.pressed(button::L2),
        Some(state.triggers[0]),
    );
    shoulder(
        canvas,
        fonts,
        684,
        196,
        160,
        60,
        "R2",
        state.pressed(button::R2),
        Some(state.triggers[1]),
    );
    shoulder(
        canvas,
        fonts,
        150,
        236,
        200,
        52,
        "L1",
        state.pressed(button::L1),
        None,
    );
    shoulder(
        canvas,
        fonts,
        674,
        236,
        200,
        52,
        "R1",
        state.pressed(button::R1),
        None,
    );
    // A wide top, and two long grips reaching down.
    for (grow, colour) in [(3, BODY_EDGE), (0, BODY)] {
        round_rect(
            canvas,
            110 - grow,
            276 - grow,
            804 + 2 * grow,
            250 + 2 * grow,
            110 + grow,
            colour,
        );
        round_rect(
            canvas,
            130 - grow,
            380 - grow,
            250 + 2 * grow,
            320 + 2 * grow,
            125 + grow,
            colour,
        );
        round_rect(
            canvas,
            644 - grow,
            380 - grow,
            250 + 2 * grow,
            320 + 2 * grow,
            125 + grow,
            colour,
        );
    }
    // The touchpad, Share and Options at its corners.
    round_rect(canvas, 392, 290, 240, 130, 20, WELL);
    pill(
        canvas,
        fonts,
        340,
        316,
        84,
        "SHARE",
        state.pressed(button::SELECT),
    );
    pill(
        canvas,
        fonts,
        690,
        316,
        104,
        "OPTIONS",
        state.pressed(button::START),
    );
    dpad_sized(canvas, 255, 380, 54, 84, state);
    let [lx, ly, rx, ry] = state.sticks;
    stick(canvas, 400, 530, lx, ly, state.pressed(button::L3));
    stick(canvas, 624, 530, rx, ry, state.pressed(button::R3));
    circle(
        canvas,
        512,
        500,
        24,
        if state.pressed(button::HOME) {
            ACCENT
        } else {
            CHIP
        },
    );
    // Shapes by position: triangle top, circle right, cross bottom, square
    // left - the same buttons the PS4 mode sends.
    let (fx, fy, gap) = (789, 380, 66);
    let shapes: [(i32, i32, u8, Rgb); 4] = [
        (fx, fy - gap, button::NORTH, OK),
        (fx + gap, fy, button::EAST, ERROR),
        (fx, fy + gap, button::SOUTH, Rgb(0x83, 0xA5, 0x98)),
        (fx - gap, fy, button::WEST, Rgb(0xD3, 0x86, 0x9B)),
    ];
    for (i, (x, y, n, tint)) in shapes.into_iter().enumerate() {
        let on = state.pressed(n);
        let ink = if on {
            round_button(canvas, x, y, 34, true)
        } else {
            round_button(canvas, x, y, 34, false);
            tint
        };
        let fill = if on { ACCENT } else { CHIP };
        match i {
            // Triangle.
            0 => {
                let (a, b, c) = ((x, y - 15), (x - 15, y + 11), (x + 15, y + 11));
                stroke(canvas, a, b, 5, ink);
                stroke(canvas, b, c, 5, ink);
                stroke(canvas, c, a, 5, ink);
            }
            // Circle: a ring.
            1 => {
                circle(canvas, x, y, 15, ink);
                circle(canvas, x, y, 10, fill);
            }
            // Cross.
            2 => {
                stroke(canvas, (x - 12, y - 12), (x + 12, y + 12), 5, ink);
                stroke(canvas, (x - 12, y + 12), (x + 12, y - 12), 5, ink);
            }
            // Square.
            _ => {
                canvas.fill_rect(x - 13, y - 13, 26, 26, ink);
                canvas.fill_rect(x - 8, y - 8, 16, 16, fill);
            }
        }
    }
}

/// The handheld's own face, as the generic gamepad has no other: laid out
/// as the buttons sit on the Brick.
fn brick_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    let w = canvas.width();
    let (bx, by, bw, bh) = (64, 256, w - 128, 436);
    // Shoulders first, so the body overlaps their lower edge. On the
    // handheld L1/R1 sit at the outer corners and L2/R2 just inside them.
    shoulder(
        canvas,
        fonts,
        bx + 8,
        by - 64,
        190,
        64,
        "L1",
        state.pressed(button::L1),
        None,
    );
    shoulder(
        canvas,
        fonts,
        bx + bw - 198,
        by - 64,
        190,
        64,
        "R1",
        state.pressed(button::R1),
        None,
    );
    shoulder(
        canvas,
        fonts,
        bx + 210,
        by - 64,
        170,
        64,
        "L2",
        state.pressed(button::L2),
        Some(state.triggers[0]),
    );
    shoulder(
        canvas,
        fonts,
        bx + bw - 380,
        by - 64,
        170,
        64,
        "R2",
        state.pressed(button::R2),
        Some(state.triggers[1]),
    );
    round_rect(canvas, bx - 3, by - 3, bw + 6, bh + 6, 51, BODY_EDGE);
    round_rect(canvas, bx, by, bw, bh, 48, BODY);

    let mid_y = by + 158;
    dpad_sized(canvas, bx + 210, mid_y, 74, 112, state);

    // Diamond: X top, Y left, A right, B bottom, as printed on the handheld.
    let (fx, gap) = (bx + bw - 210, 92);
    face_button(
        canvas,
        fonts,
        fx,
        mid_y - gap,
        "X",
        state.pressed(button::NORTH),
    );
    face_button(
        canvas,
        fonts,
        fx - gap,
        mid_y,
        "Y",
        state.pressed(button::WEST),
    );
    face_button(
        canvas,
        fonts,
        fx + gap,
        mid_y,
        "A",
        state.pressed(button::EAST),
    );
    face_button(
        canvas,
        fonts,
        fx,
        mid_y + gap,
        "B",
        state.pressed(button::SOUTH),
    );

    // Sticks below the D-pad and the face buttons, on the models that have
    // them. The Brick standard's status screen keeps the same row for the
    // menu buttons so the layout stays balanced.
    let stick_y = by + bh - 74;
    if has_sticks() {
        let [lx, ly, rx, ry] = state.sticks;
        stick(canvas, bx + 210, stick_y, lx, ly, state.pressed(button::L3));
        stick(canvas, fx, stick_y, rx, ry, state.pressed(button::R3));
    }

    let cx = bx + bw / 2;
    // MENU, SELECT, START from left to right, as on the handheld.
    pill(
        canvas,
        fonts,
        cx - 112,
        stick_y,
        100,
        "MENU",
        state.pressed(button::HOME),
    );
    pill(
        canvas,
        fonts,
        cx,
        stick_y,
        100,
        "SELECT",
        state.pressed(button::SELECT),
    );
    pill(
        canvas,
        fonts,
        cx + 112,
        stick_y,
        100,
        "START",
        state.pressed(button::START),
    );
}

fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    profile: Profile,
    status: &Status,
    state: &hid::State,
    chord_held: bool,
) {
    canvas.clear(BG);

    let (corner, colour, text): (String, Rgb, Vec<String>) = match status {
        Status::Starting => ("Starting".into(), DIM, vec!["Turning Bluetooth on…".into()]),
        Status::Failed(e) => (
            "Could not start".into(),
            ERROR,
            vec![e.clone(), "Press B or MENU to quit.".into()],
        ),
        Status::Waiting { reconnecting } => {
            let pair = format!(
                "On a PC or Mac: Bluetooth settings > choose \"{}\".",
                profile.bt_name()
            );
            let mut text = vec![pair];
            if let Some(host) = reconnecting {
                text.push(format!("Reconnecting to {host}…"));
            } else {
                text.push("Pair from the host's Bluetooth settings.".into());
                text.push("Profile changed? Forget the old pairing first.".into());
            }
            (format!("{} · waiting", profile.label()), WARN, text)
        }
        Status::Connected { name } => (
            format!("{} · connected", profile.label()),
            OK,
            vec![format!("Connected to {name}.")],
        ),
    };
    header(canvas, fonts, NAME, &corner, colour);
    let who = match status {
        Status::Connected { name } => Some(name.as_str()),
        _ => None,
    };
    lines(canvas, fonts, &text, who);

    // The Brick's own controls, lighting what is pressed.
    brick_face(canvas, fonts, state);

    let credit = NAME;
    if chord_held {
        footer(canvas, fonts, &[("F1+F2", "keep holding to quit…")], credit);
    } else {
        footer(
            canvas,
            fonts,
            &[("F1", "settings"), ("F2", "inputs"), ("F1+F2", "quit")],
            credit,
        );
    }
}

/// The settings screen: the paired hosts and their link state. It is also
/// what the app shows while no host is connected.
fn draw_settings(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    settings: &Settings,
    connected: bool,
    chord_held: bool,
) {
    let w = canvas.width();
    canvas.clear(BG);
    header(canvas, fonts, "Settings", &format!("{NAME} {VERSION}"), DIM);

    let (size, row_h) = (30.0, 70);
    if let Some(error) = &settings.error {
        for (i, line) in fonts.wrap(error, 24.0, w - LEFT * 2, 2).iter().enumerate() {
            fonts.draw(canvas, line, LEFT, CONTENT_TOP + i as i32 * 32, 24.0, ERROR);
        }
    } else if settings.hosts.is_empty() {
        fonts.draw(canvas, "No paired host.", LEFT, CONTENT_TOP, size, FG);
        fonts.draw(
            canvas,
            &format!("Pair with \"{}\".", settings.pairing.bt_name()),
            LEFT,
            CONTENT_TOP + 36,
            24.0,
            DIM,
        );
        let baseline = CONTENT_TOP + 100;
        if settings.at_pairing() {
            canvas.fill_rect(LEFT - 12, baseline - 42, w - LEFT * 2 + 24, row_h, CURSOR);
        }
        fonts.draw(canvas, "Next pairing", LEFT, baseline, size, FG);
        let label = settings.pairing.label();
        let lw = fonts.measure(label, 24.0);
        fonts.draw(canvas, label, w - LEFT - lw, baseline, 24.0, OK);
        fonts.draw(
            canvas,
            "F2 to change · used for the next host",
            LEFT,
            baseline + 24,
            18.0,
            DIM,
        );
        fonts.draw(
            canvas,
            "Profile changed? Forget the old pairing first.",
            LEFT,
            baseline + 56,
            18.0,
            DIM,
        );
    } else {
        let visible = 8;
        let start = settings.cursor.saturating_sub(visible - 1);
        for (index, host) in settings.hosts.iter().enumerate().skip(start).take(visible) {
            let baseline = CONTENT_TOP + 8 + (index - start) as i32 * row_h;
            if index == settings.cursor {
                canvas.fill_rect(LEFT - 12, baseline - 42, w - LEFT * 2 + 24, row_h, CURSOR);
            }
            fonts.draw(canvas, &host.name, LEFT, baseline, size, FG);
            let status = if host.connected {
                "connected"
            } else {
                "disconnected"
            };
            let sw = fonts.measure(status, 24.0);
            fonts.draw(
                canvas,
                status,
                w - LEFT - sw,
                baseline,
                24.0,
                if host.connected { OK } else { DIM },
            );
            let profile = settings.profile_for(host.addr);
            fonts.draw(
                canvas,
                &format!("{} · {}", host.addr, profile.label()),
                LEFT,
                baseline + 24,
                18.0,
                DIM,
            );
        }
    }

    // Discreet upstream credit and version. The full license stays in the pak.
    let notice = format!("{NAME} {VERSION} · based on {PADPOD_CREDIT} · {HOME_PAGE}");
    let lines = fonts.wrap(&notice, 18.0, w - LEFT * 2, 1);
    for (i, line) in lines.iter().enumerate() {
        fonts.draw(
            canvas,
            line,
            LEFT,
            BOTTOM_BASELINE - 38 + i as i32 * 24,
            18.0,
            DIM,
        );
    }
    if chord_held {
        footer(canvas, fonts, &[("F1+F2", "keep holding to quit…")], "");
    } else if settings.has_pairing_row() {
        footer(canvas, fonts, &[("F2", "PC/Xbox"), ("F1+F2", "quit")], "");
    } else if settings.hosts.is_empty() {
        footer(canvas, fonts, &[("F1+F2", "quit")], "");
    } else {
        let mut hints = vec![
            ("A", "connect/disconnect"),
            ("F2", "PC/Xbox"),
            ("F1+F2", "quit"),
        ];
        if connected {
            hints.insert(0, ("F1", "controller"));
        }
        footer(canvas, fonts, &hints, "");
    }
}

fn draw_input_monitor(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    monitor: &InputMonitor,
    chord_held: bool,
) {
    canvas.clear(BG);
    header(
        canvas,
        fonts,
        "Input monitor",
        &format!("{NAME} {VERSION}"),
        DIM,
    );

    if monitor.lines.is_empty() {
        fonts.draw(canvas, "Press a button…", LEFT, CONTENT_TOP, 30.0, DIM);
    } else {
        let visible = 14;
        let start = monitor.lines.len().saturating_sub(visible);
        let lines = &monitor.lines[start..];
        let bottom = BOTTOM_BASELINE - 70;
        for (index, line) in lines.iter().enumerate() {
            let y = bottom - (lines.len() - 1 - index) as i32 * 32;
            fonts.draw(canvas, line, LEFT, y, 24.0, FG);
        }
    }

    if chord_held {
        footer(canvas, fonts, &[("F1+F2", "keep holding to quit…")], "");
    } else {
        footer(
            canvas,
            fonts,
            &[("F2", "controller"), ("F1+F2", "quit")],
            "",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BRICK_CONTROLLER_PREVIEW=dir cargo test preview` writes each screen as a PNG,
    /// for checking the layout without the device.
    #[test]
    fn preview_every_screen() {
        let Ok(dir) = std::env::var("BRICK_CONTROLLER_PREVIEW") else {
            return;
        };
        let mut fonts = Fonts::load(&[]);
        let mut pressed = hid::State::default();
        pressed.set_button(hid::button::SOUTH, true);
        pressed.set_button(hid::button::R1, true);
        pressed.hat = 1;
        pressed.triggers[0] = 120;
        pressed.sticks = [128, 128, 255, 60];
        pressed.set_button(hid::button::L3, true);
        let screens = [
            ("starting", Status::Starting, hid::State::default()),
            (
                "failed",
                Status::Failed(
                    "Bluetooth is off. Turn it on in NextUI settings and open Brick Controller again."
                        .into(),
                ),
                hid::State::default(),
            ),
            (
                "waiting",
                Status::Waiting {
                    reconnecting: Some("Pixel 9".into()),
                },
                hid::State::default(),
            ),
            ("connected", Status::Connected { name: "my-pc".into() }, pressed),
        ];
        let mut canvas = Canvas::in_memory(1024, 768);
        draw(
            &mut canvas,
            &mut fonts,
            Profile::Pc,
            &Status::Waiting { reconnecting: None },
            &hid::State::default(),
            false,
        );
        draw_settings(
            &mut canvas,
            &mut fonts,
            &Settings {
                hosts: vec![
                    bluez::Host {
                        addr: Addr([0x11; 6]),
                        name: "Studio Mac".into(),
                        connected: true,
                    },
                    bluez::Host {
                        addr: Addr([0x22; 6]),
                        name: "Desktop PC".into(),
                        connected: false,
                    },
                ],
                profiles: HashMap::from([(Addr([0x22; 6]), Profile::Xbox)]),
                pairing: Profile::Pc,
                cursor: 1,
                error: None,
            },
            true,
            false,
        );
        png::write_png(
            &format!("{dir}/settings.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
        let mut canvas = Canvas::in_memory(1024, 768);
        draw_settings(
            &mut canvas,
            &mut fonts,
            &Settings {
                hosts: Vec::new(),
                profiles: HashMap::new(),
                pairing: Profile::Xbox,
                cursor: 0,
                error: None,
            },
            false,
            false,
        );
        png::write_png(
            &format!("{dir}/settings-empty.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
        let mut monitor = InputMonitor::new();
        for line in [
            "MENU down",
            "MENU up",
            "D-pad right",
            "A down",
            "A up",
            "L2 255",
            "R2 120",
            "sticks 255 128 128 60",
            "SELECT down",
            "SELECT up",
            "F1 down",
            "F1 up",
            "F2 down",
            "F2 up",
        ] {
            monitor.push(line);
        }
        let mut canvas = Canvas::in_memory(1024, 768);
        draw_input_monitor(&mut canvas, &mut fonts, &monitor, false);
        png::write_png(
            &format!("{dir}/monitor.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
        for (name, status, state) in screens {
            let mut canvas = Canvas::in_memory(1024, 768);
            draw(&mut canvas, &mut fonts, Profile::Pc, &status, &state, false);
            png::write_png(
                &format!("{dir}/{name}.png"),
                1024,
                768,
                &canvas.snapshot_rgb(),
            )
            .unwrap();
        }
        let mut canvas = Canvas::in_memory(1024, 768);
        draw(
            &mut canvas,
            &mut fonts,
            Profile::Pc,
            &Status::Connected {
                name: "my-pc".into(),
            },
            &hid::State::default(),
            true,
        );
        png::write_png(
            &format!("{dir}/connected-quit.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
    }

    #[test]
    fn the_settings_cursor_moves_and_wraps() {
        let host = |n: u8| bluez::Host {
            addr: Addr([n; 6]),
            name: format!("host {n}"),
            connected: false,
        };
        let mut settings = Settings {
            hosts: vec![host(1), host(2)],
            profiles: HashMap::new(),
            pairing: Profile::Pc,
            cursor: 0,
            error: None,
        };
        settings.move_cursor(1);
        assert_eq!(settings.cursor, 1);
        settings.move_cursor(1);
        assert_eq!(settings.cursor, 0, "wraps");
        settings.move_cursor(-1);
        assert_eq!(settings.cursor, 1);

        let mut empty = Settings {
            hosts: Vec::new(),
            profiles: HashMap::new(),
            pairing: Profile::Pc,
            cursor: 0,
            error: None,
        };
        assert!(empty.at_pairing());
        empty.move_cursor(1);
        assert_eq!(empty.cursor, 0, "the pairing row is the only one");
        assert_eq!(empty.rows(), 1);
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

    #[test]
    fn a_hat_direction_lights_its_arrow_and_the_diagonals_beside_it() {
        let mut s = hid::State {
            hat: 1,
            ..Default::default()
        }; // up-right
        assert!(lit(Control::Hat(0), &s));
        assert!(lit(Control::Hat(2), &s));
        assert!(!lit(Control::Hat(4), &s));
        assert!(!lit(Control::Hat(6), &s));
        s.hat = hid::HAT_CENTRED;
        assert!(!lit(Control::Hat(0), &s));
    }
}
