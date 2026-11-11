// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! One Bluetooth session: daemon takeover, host link, HID reports and the
//! screen state that goes with them.
//!
//! The session is a poll-driven state machine. It owns the controller codec,
//! the L2CAP channels and the panel, but delegates report layout to
//! `profiles` and view construction to `app::view_model`.

use super::settings::{remember_profile, save_host, save_mode, save_profiles, Settings};
use super::view_model::{
    modal_view, move_cursor, nav_dir, view_for, Dir, MenuRow, ModalState, Screen,
};
use super::StartScreen;
use crate::bluetooth::{
    bluez,
    l2cap::{Channel, Listener},
    sdp, Address,
};
use crate::hid;
use crate::i18n::{fill, tr, Key, Lang};
use crate::input::{Button, PadState};
use crate::platform::input_trimui::Pad;
use crate::platform::{display, rumble, StatusInfo};
use crate::profiles::{Controller, LinkContext, Profile, Schedule};
use crate::ui::{Frame, Modal, Renderer, View};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// The parts of a frame that decide whether a redraw is needed.
type DrawKey = (Lang, View, StatusInfo, Option<Modal>, Option<u8>);

/// An established HID link: the two L2CAP channels and the host they belong to.
///
/// `name` starts empty and is filled once BlueZ can answer, so the first report
/// is not delayed by a D-Bus lookup.
pub(crate) struct Link {
    pub(crate) control: Channel,
    pub(crate) interrupt: Channel,
    pub(crate) host: Address,
    pub(crate) name: String,
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
    target: Arc<Mutex<Option<Address>>>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    out: mpsc::Sender<(Channel, Channel, Address)>,
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

pub(crate) enum SessionOutcome {
    Quit,
    SwitchProfile {
        profile: Profile,
        connect: Option<Address>,
        /// Reopen Pairing after the Bluetooth restart, instead of Devices.
        pairing: bool,
    },
    Retry,
}

/// Release every button the host may still see held, in the profile's format.
///
/// This runs before leaving the controller view. Without it, a button held
/// while quitting would stay pressed in the host's view of the device until
/// the link drops, which games read as a stuck input.
fn send_neutral(link: &Link, profile: Profile) {
    let mut controller = Controller::new(profile);
    let reports = controller.packets(&PadState::default(), Duration::ZERO);
    for report in reports.iter() {
        let _ = link.interrupt.send(report);
    }
}

/// Let the profile parse an output report and drive the single motor with the
/// result. Returns true when the host asked the controller to disconnect.
fn drive_rumble(
    controller: &mut Controller,
    link: &LinkContext,
    motor: Option<&rumble::Rumble>,
    report: &[u8],
) -> bool {
    let output = controller.output(report, link);
    if let (Some(level), Some(motor)) = (output.rumble, motor) {
        motor.set(level);
    }
    output.disconnect
}

/// End the active link, a half-open pairing channel and any connect attempt.
/// Devices is a chooser, so nothing may keep running while it is open.
fn stop_devices_session(
    link: &mut Option<Link>,
    half: &mut Option<(Channel, Address)>,
    bus: &bluez::Bus,
    target: &Arc<Mutex<Option<Address>>>,
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
    half: &mut Option<(Channel, Address)>,
    bus: &bluez::Bus,
    settings: &mut Settings,
    profile: Profile,
    target: &Arc<Mutex<Option<Address>>>,
    connected: &Arc<AtomicBool>,
    motor: Option<&rumble::Rumble>,
) {
    stop_devices_session(link, half, bus, target, connected, motor);
    *settings = Settings::load(bus, profile);
}

/// Bluetooth up as the chosen controller, until Menu+Start is held or another
/// host profile is selected.
///
/// Invariants kept by the loop below:
///
/// - only the pairing screen is discoverable and accepts incoming channels;
/// - no host is paged until the user chooses one from Devices;
/// - the pad is neutral towards the host whenever an interface screen or a
///   modal is open, and until every interface button has been released;
/// - the panel may go dark while reports keep flowing, but Menu+Start always
///   wakes it and re-arms the ten-second cycle;
/// - leaving the controller view releases any held button before the link is
///   torn down.
pub(crate) fn session(
    profile: Profile,
    start: StartScreen,
    connect: Option<Address>,
    screen: &Renderer,
    pad: &mut Pad,
) -> Result<SessionOutcome, String> {
    let lang = Lang::En;
    if start == StartScreen::Splash {
        screen.show(Frame {
            lang,
            view: View::Splash {
                message: tr(lang, Key::StartingBluetooth).into(),
            },
            status: StatusInfo::read(false),
            modal: None,
            hold: None,
        });
    } else if start == StartScreen::Pairing {
        // The previous session may have left the Profile list on screen while
        // the daemon restarts. Show the new identity's Pairing screen at once,
        // then let the loop replace it with the live status frame.
        screen.show(Frame {
            lang,
            view: View::Pairing {
                identity: profile.bt_name().into(),
                controller_type: profile.label(),
                reconnecting: None,
            },
            status: StatusInfo::read(false),
            modal: None,
            hold: None,
        });
    }

    let daemon = match bluez::Daemon::take_over(profile.bt_name(), profile.device_id()) {
        Ok(d) => d,
        Err(e) => return fail(screen, pad, e, lang),
    };
    let bus = match bluez::Bus::open(&sdp::record(profile.bt_name(), profile.descriptor())) {
        Ok(b) => b,
        Err(e) => return fail(screen, pad, e, lang),
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
    let local = bus.address().unwrap_or(Address([0; 6]));

    // A profile decides whether it has a rumble path; a missing GPIO is not an
    // error, the gamepad still works.
    let motor = if profile.has_rumble() {
        let motor = rumble::Rumble::open();
        if motor.is_none() {
            eprintln!("rumble: motor not available");
        }
        motor
    } else {
        None
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
    let mut half: Option<(Channel, Address)> = None;
    let mut discoverable = None;
    let mut sent = None;
    let mut sent_at = Instant::now();
    let mut previous = pad.mapper.state;
    let mut disconnect_since: Option<Instant> = None;
    let mut combo_seen = false;
    let mut settings = Settings::load(&bus, profile);
    let mut controller = Controller::new(profile);
    let mut state = match start {
        StartScreen::Pairing => Screen::Pairing { back: true },
        _ => Screen::Menu { cursor: 0 },
    };
    let mut modal = connect.map(ModalState::Connecting);
    let mut connect_deadline = connect.map(|_| Instant::now() + CONNECT_TIMEOUT);
    // The profile to switch to. A type chosen from Pairing reopens Pairing; a
    // Connect that needs another identity reopens Devices and then reconnects
    // to the host.
    let mut switch_to: Option<Profile> = None;
    let mut switch_connect: Option<Address> = None;
    let mut switch_pairing = false;
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
            PadState::default()
        };

        // One poll for every event source. The pad is first so the read below
        // can rely on it; listeners come next, then the two link channels when
        // a host is connected.
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
        // A change held back by the profile's minimum gap is due when that
        // gap ends; a periodic profile is due on its own cadence. Everything
        // else waits for something to happen, and a dark panel can sleep
        // between input events because it has no animation to feed.
        let wait = if link.is_some() {
            match controller.profile().schedule() {
                Schedule::OnChange { min_gap } if Some(pad.mapper.state) != sent => {
                    min_gap.saturating_sub(sent_at.elapsed()).as_millis() as i32
                }
                Schedule::Periodic { every } => {
                    every.saturating_sub(sent_at.elapsed()).as_millis() as i32
                }
                _ => 100,
            }
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
        //
        // Control and interrupt channels carry different traffic: control is
        // request/reply (GET_REPORT, SET_IDLE, unplug), interrupt is the
        // one-way report path plus output reports. They are drained
        // separately so a slow control reply cannot delay a gamepad report.
        let mut lost = false;
        let mut unplugged = false;
        if let Some(l) = &link {
            let link_ctx = LinkContext {
                local,
                peer: l.host,
            };
            loop {
                match l.control.recv() {
                    // The profile handles its feature reports before falling
                    // back to the generic HID transactions. The session never
                    // needs to know which profile is active.
                    Ok(Some(message)) => {
                        if let Some(report) = hid::set_report_output(&message) {
                            if drive_rumble(&mut controller, &link_ctx, motor.as_ref(), report) {
                                lost = true;
                            }
                        }
                        match controller.control(&message, &link_ctx, &control_state) {
                            hid::Control::Reply(bytes) => {
                                if l.control.send(&bytes).is_err() {
                                    lost = true;
                                }
                            }
                            hid::Control::Unplug => unplugged = true,
                            hid::Control::Ignore => {}
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            // Output reports (rumble, Switch subcommands) arrive on the
            // interrupt channel as 0xA2 DATA transactions.
            loop {
                match l.interrupt.recv() {
                    Ok(Some(message)) => {
                        if let Some(report) = hid::data_output(&message) {
                            if drive_rumble(&mut controller, &link_ctx, motor.as_ref(), report) {
                                lost = true;
                            }
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
        let a_pressed = now.pressed(Button::East) && !previous.pressed(Button::East);
        let b_pressed = now.pressed(Button::South) && !previous.pressed(Button::South);
        let x_pressed = now.pressed(Button::North) && !previous.pressed(Button::North);
        let direction = nav_dir(previous.hat, now.hat);
        let combos = now.pressed(Button::Home) && now.pressed(Button::Start);
        let combo_was = previous.pressed(Button::Home) && previous.pressed(Button::Start);

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
                                    save_mode(choice);
                                    switch_to = Some(choice);
                                    switch_pairing = true;
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

        // The panel-off timer is a display decision, not a link decision: the
        // HID session stays up and keeps serving reports while the screen is
        // dark, which is the whole point of the battery-saving cycle.
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
                pairing: switch_pairing,
            };
        }

        // Interface buttons must never leak to the host. While a screen or a
        // modal is open, the report is neutral; after returning to the
        // controller view, it stays neutral until every button used in the
        // interface has been released. Otherwise the host would see the A that
        // confirmed a menu as a fresh gamepad press.
        let ui_active = !matches!(state, Screen::Controller) || modal.is_some();
        if last_ui_active && !ui_active {
            resume_after_release = true;
        }
        if resume_after_release && now == PadState::default() {
            resume_after_release = false;
        }
        last_ui_active = ui_active;
        let report_state = if ui_active || combo_seen || resume_after_release {
            PadState::default()
        } else {
            now
        };

        if let Some(l) = &link {
            // A change-driven profile waits out its minimum gap; a periodic
            // profile sends on its own cadence. Either way the report carries
            // the whole state, so a send the radio refuses can be dropped: the
            // next one supersedes it.
            let due = match controller.profile().schedule() {
                Schedule::OnChange { min_gap } => {
                    sent.is_none() || (Some(report_state) != sent && sent_at.elapsed() >= min_gap)
                }
                Schedule::Periodic { every } => sent.is_none() || sent_at.elapsed() >= every,
            };
            if !lost && due {
                let reports = controller.packets(&report_state, sent_at.elapsed());
                let mut result = Ok(());
                for report in reports.iter() {
                    if let Err(error) = l.interrupt.send(report) {
                        result = Err(error);
                        break;
                    }
                }
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

        // Build a pure render model from the current state. The renderer sees
        // only labels and rows, never `Settings`, `Profile` or `Link`; this is
        // the seam that keeps `ui` independent of the session.
        let status = StatusInfo::read(true);
        let target_addr = *target.lock().unwrap();
        let view = view_for(&state, &settings, link.as_ref(), profile, target_addr, lang);
        let modal_view = modal
            .as_ref()
            .map(|pending| modal_view(pending, &settings, lang));
        let hold = if combo_seen && matches!(state, Screen::Controller) && !screen_off {
            disconnect_since.map(|since| {
                ((since.elapsed().as_secs_f32() / DISCONNECT_HOLD.as_secs_f32()).clamp(0.0, 1.0)
                    * 20.0) as u8
            })
        } else {
            None
        };
        let key = (lang, view.clone(), status.clone(), modal_view.clone(), hold);
        if !screen_off && drawn.as_ref() != Some(&key) {
            screen.show(Frame {
                lang,
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

/// Show an error and wait for Retry or Quit.
fn fail(
    screen: &Renderer,
    pad: &mut Pad,
    error: String,
    lang: Lang,
) -> Result<SessionOutcome, String> {
    eprintln!("brick-controller: {error}");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut previous = pad.mapper.state;
    let mut shown = 60u64;
    screen.show(Frame {
        lang,
        view: error_view(lang, &error, fill(tr(lang, Key::ClosingIn), &[("n", "60")])),
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
            let a = now.pressed(Button::East) && !previous.pressed(Button::East);
            let b = now.pressed(Button::South) && !previous.pressed(Button::South);
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
            let countdown = left.to_string();
            screen.show(Frame {
                lang,
                view: error_view(
                    lang,
                    &error,
                    fill(tr(lang, Key::ClosingIn), &[("n", &countdown)]),
                ),
                status: StatusInfo::read(false),
                modal: None,
                hold: None,
            });
        }
    }
    Ok(SessionOutcome::Quit)
}

/// The error screen for a failed Bluetooth takeover.
///
/// The daemon's own message is shown only for the known Bluetooth-off case;
/// anything else is a technical detail kept verbatim so a bug report can be
/// matched to the log.
fn error_view(lang: Lang, error: &str, countdown: String) -> View {
    let message = if error == bluez::BLUETOOTH_OFF {
        tr(lang, Key::BluetoothOffMessage).into()
    } else {
        error.to_string()
    };
    View::Error {
        title: tr(lang, Key::BluetoothOffTitle).into(),
        message,
        steps: vec![
            tr(lang, Key::TurnBluetoothOn).into(),
            tr(lang, Key::ReopenApp).into(),
        ],
        countdown: Some(countdown),
    }
}
