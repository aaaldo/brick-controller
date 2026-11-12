// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The Linux side of the wired HID transport: take over `g1`, mount
//! FunctionFS, bind the UDC and restore everything on exit.
//!
//! The vendor configfs implementation accepts a single gadget. A second
//! `mkdir` fails on the internal `android0` name, so the session borrows the
//! ADB gadget instead of creating its own and snapshots it first. ADB is
//! therefore unavailable for the duration of the wired session; the `restore`
//! subcommand and the wrapper bring it back even after a crash.

use super::control::{self, Reply, Setup};
use super::ffs;
use super::snapshot::Snapshot;
use crate::paths::data_dir;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{symlink, OpenOptionsExt};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const G1: &str = "/sys/kernel/config/usb_gadget/g1";
const FFS_NAME: &str = "brickcontroller";
const FFS_DIR: &str = "/dev/usb-ffs/brickcontroller";
const MARKER: &str = "/tmp/brick-controller-usb";
const MANUFACTURER: &str = "Brick Controller";
const BCD_DEVICE: &str = "0x0100";
const BCD_USB: &str = "0x0200";
const MAX_POWER: &str = "500";

/// Device attributes saved before the identity is replaced.
const ATTRS: &[&str] = &[
    "idVendor",
    "idProduct",
    "bcdDevice",
    "bcdUSB",
    "bMaxPacketSize0",
    "bDeviceClass",
    "bDeviceSubClass",
    "bDeviceProtocol",
    "configs/c.1/MaxPower",
    "configs/c.1/bmAttributes",
];

/// String descriptors saved before the identity is replaced.
const STRINGS: &[&str] = &[
    "strings/0x409/manufacturer",
    "strings/0x409/product",
    "strings/0x409/serialnumber",
    "configs/c.1/strings/0x409/configuration",
];

/// Events FunctionFS reports on `ep0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Bind,
    Unbind,
    Enable,
    Disable,
    Setup(Setup),
    Suspend,
    Resume,
}

/// The wired HID function, bound to the UDC for the session.
///
/// Dropping it closes the endpoints and restores `g1`, so a normal exit, an
/// early return and a failed initialisation all leave ADB in its original
/// shape.
pub struct UsbHid {
    ep0: File,
    ep1: File,
    report_descriptor: &'static [u8],
    /// Restores `g1` when the struct drops; the field is never read.
    _restore: Restore,
}

impl UsbHid {
    /// Take over `g1`, publish the HID descriptors and bind the UDC.
    ///
    /// `product` is the name the host sees; `report_descriptor` is the PC
    /// profile's descriptor, shared with the Bluetooth codec.
    pub fn take_over(product: &str, report_descriptor: &'static [u8]) -> io::Result<Self> {
        // A marker left by a crash means g1 is still in its wired shape. Put
        // it back before snapshotting, or the snapshot would capture the
        // temporary identity instead of the ADB one.
        restore_after_crash();

        let udc = read_udc()?;
        // The guard is armed before the first write, and restores on every
        // early return from here on.
        let restore = Restore::capture(udc)?;
        restore.setup(product)?;

        let ep0 = open_ep0()?;
        write_once(
            ep0.as_raw_fd(),
            &ffs::descriptors(report_descriptor.len() as u16),
        )?;
        write_once(ep0.as_raw_fd(), &ffs::strings())?;
        let ep1 = wait_open_ep1()?;
        restore.bind()?;

        eprintln!("usb: wired HID ready");
        Ok(Self {
            ep0,
            ep1,
            report_descriptor,
            _restore: restore,
        })
    }

    pub fn ep0_fd(&self) -> RawFd {
        self.ep0.as_raw_fd()
    }

    /// Read one FunctionFS event, or `None` when the queue is empty.
    pub fn read_event(&mut self) -> io::Result<Option<Event>> {
        let mut buf = [0u8; 12];
        loop {
            let n =
                unsafe { libc::read(self.ep0.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len()) };
            if n < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(None);
                }
                return Err(error);
            }
            if n == 0 {
                return Ok(None);
            }
            if n != buf.len() as isize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("FunctionFS event {n} bytes"),
                ));
            }
            break;
        }

        let kind = buf[8];
        Ok(match kind {
            0 => Some(Event::Bind),
            1 => Some(Event::Unbind),
            2 => Some(Event::Enable),
            3 => Some(Event::Disable),
            4 => {
                let mut setup = [0u8; 8];
                setup.copy_from_slice(&buf[..8]);
                Some(Event::Setup(Setup::from_bytes(&setup)))
            }
            5 => Some(Event::Suspend),
            6 => Some(Event::Resume),
            other => {
                eprintln!("usb: unknown FunctionFS event {other}");
                None
            }
        })
    }

    /// Answer a control request using the report currently shown to the host.
    pub fn handle_setup(&mut self, setup: &Setup, report: &[u8]) -> io::Result<()> {
        match control::respond(setup, self.report_descriptor, report) {
            Reply::Data(data) => self.write_ep0(&data, setup.length),
            Reply::ReadOut => {
                if setup.length > 0 {
                    self.read_ep0(setup.length)?;
                }
                Ok(())
            }
            Reply::Ack => Ok(()),
            Reply::Stall => {
                self.stall(setup.dir_in());
                Ok(())
            }
        }
    }

    /// Queue one input report. A host that went away is not an error: the
    /// FunctionFS disable event will move the session back to waiting.
    pub fn send_report(&self, report: &[u8]) -> io::Result<()> {
        let n = unsafe {
            libc::write(
                self.ep1.as_raw_fd(),
                report.as_ptr() as *const _,
                report.len(),
            )
        };
        if n < 0 {
            let error = io::Error::last_os_error();
            let code = error.raw_os_error();
            if error.kind() == io::ErrorKind::WouldBlock
                || code == Some(libc::ESHUTDOWN)
                || code == Some(libc::EPIPE)
            {
                return Ok(());
            }
            return Err(error);
        }
        if (n as usize) < report.len() {
            eprintln!("usb: short report write {n}/{}", report.len());
        }
        Ok(())
    }

    fn write_ep0(&self, data: &[u8], wlength: u16) -> io::Result<()> {
        let n = data.len().min(wlength as usize);
        let rc = unsafe { libc::write(self.ep0.as_raw_fd(), data.as_ptr() as *const _, n) };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(error);
        }
        if (rc as usize) < n {
            eprintln!("usb: short ep0 write {rc}/{n}");
        }
        Ok(())
    }

    fn read_ep0(&self, wlength: u16) -> io::Result<()> {
        let mut buf = [0u8; 256];
        let n = (wlength as usize).min(buf.len());
        if n == 0 {
            return Ok(());
        }
        let rc = unsafe { libc::read(self.ep0.as_raw_fd(), buf.as_mut_ptr() as *mut _, n) };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(error);
        }
        Ok(())
    }

    fn stall(&self, dir_in: bool) {
        if dir_in {
            let mut byte = [0u8; 1];
            unsafe {
                libc::read(self.ep0.as_raw_fd(), byte.as_mut_ptr() as *mut _, 1);
            }
        } else {
            let byte = [0u8; 1];
            unsafe {
                libc::write(self.ep0.as_raw_fd(), byte.as_ptr() as *const _, 0);
            }
        }
    }
}

/// Restore `g1` after a crash, when no Rust value ran its `Drop`.
///
/// A missing marker means the wired session never started or already restored
/// itself; the system daemon owns `g1` and must be left alone.
pub fn restore_after_crash() {
    let Ok(udc) = read_udc() else {
        return;
    };
    let Ok(text) = fs::read_to_string(MARKER) else {
        return;
    };
    let snapshot = Snapshot::parse(&text);
    if snapshot.is_empty() {
        eprintln!("usb: ignoring an empty g1 marker");
        return;
    }
    let mut restore = Restore {
        udc,
        snapshot: Some(snapshot),
        done: false,
    };
    restore.restore();
}

/// `g1` as it was before the wired session, and the code that puts it back.
struct Restore {
    udc: String,
    snapshot: Option<Snapshot>,
    done: bool,
}

impl Restore {
    /// Snapshot `g1` and arm the crash marker before anything is changed.
    fn capture(udc: String) -> io::Result<Self> {
        let snapshot = read_snapshot();
        if snapshot.attrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the ADB gadget g1 is not present",
            ));
        }
        fs::write(MARKER, snapshot.render())?;
        Ok(Self {
            udc,
            snapshot: Some(snapshot),
            done: false,
        })
    }

    fn setup(&self, product: &str) -> io::Result<()> {
        // Unbind before changing the identity or the function links: configfs
        // refuses those writes while a UDC owns the gadget.
        let _ = fs::write(format!("{G1}/UDC"), "\n");
        std::thread::sleep(Duration::from_millis(200));
        remove_stale_ffs();
        if let Some(snapshot) = &self.snapshot {
            for name in &snapshot.links {
                let _ = fs::remove_file(format!("{G1}/configs/c.1/{name}"));
            }
        }

        fs::create_dir_all(format!("{G1}/strings/0x409"))?;
        fs::create_dir_all(format!("{G1}/configs/c.1/strings/0x409"))?;

        write_attr("idVendor", &format!("0x{:04X}", super::USB_VENDOR))?;
        write_attr("idProduct", &format!("0x{:04X}", super::USB_PRODUCT))?;
        write_attr("bcdDevice", BCD_DEVICE)?;
        write_attr("bcdUSB", BCD_USB)?;
        write_attr("bMaxPacketSize0", "64")?;
        write_attr("bDeviceClass", "0x00")?;
        write_attr("bDeviceSubClass", "0x00")?;
        write_attr("bDeviceProtocol", "0x00")?;
        write_attr("configs/c.1/MaxPower", MAX_POWER)?;
        write_attr("configs/c.1/bmAttributes", "0x80")?;
        write_string("strings/0x409/manufacturer", MANUFACTURER)?;
        write_string("strings/0x409/product", product)?;
        write_string("strings/0x409/serialnumber", &stable_serial())?;
        write_string("configs/c.1/strings/0x409/configuration", product)?;

        fs::create_dir(format!("{G1}/functions/ffs.{FFS_NAME}"))?;
        // Configfs resolves a symlink target from the process working
        // directory, so the function path must be absolute.
        symlink(
            format!("{G1}/functions/ffs.{FFS_NAME}"),
            format!("{G1}/configs/c.1/ffs.{FFS_NAME}"),
        )?;
        mount_ffs()?;
        eprintln!("usb: g1 repurposed");
        Ok(())
    }

    fn bind(&self) -> io::Result<()> {
        fs::write(format!("{G1}/UDC"), format!("{}\n", self.udc))?;
        eprintln!("usb: g1 bound to {}", self.udc);
        Ok(())
    }

    fn restore(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let Some(snapshot) = self.snapshot.take() else {
            return;
        };

        let _ = fs::write(format!("{G1}/UDC"), "\n");
        std::thread::sleep(Duration::from_millis(100));
        remove_stale_ffs();

        for (path, value) in &snapshot.attrs {
            let _ = write_attr(path, value);
        }
        for (path, value) in &snapshot.strings {
            let _ = write_string(path, value);
        }

        // Remove whatever links are present, then recreate the saved ones with
        // absolute function targets. A relative target recreated from the
        // app's working directory fails with ENOENT.
        if let Ok(entries) = fs::read_dir(format!("{G1}/configs/c.1")) {
            for entry in entries.flatten() {
                if entry
                    .file_type()
                    .map(|kind| kind.is_symlink())
                    .unwrap_or(false)
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        for name in &snapshot.links {
            let target = format!("{G1}/functions/{name}");
            let link = format!("{G1}/configs/c.1/{name}");
            if let Err(error) = symlink(&target, &link) {
                eprintln!("usb: could not restore g1 link {name}: {error}");
            }
        }

        if self.rebind() {
            let _ = fs::remove_file(MARKER);
            eprintln!("usb: g1 and ADB restored");
        } else {
            // Keep the snapshot: the next launch retries after adbd has had
            // more time to come back.
            eprintln!("usb: g1 restore incomplete; keeping {MARKER}");
        }
    }

    fn rebind(&self) -> bool {
        for _ in 0..20 {
            if fs::write(format!("{G1}/UDC"), format!("{}\n", self.udc)).is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // adbd usually survives the unbind; if it did not, restart it and wait
        // for its FunctionFS endpoints before trying the UDC again.
        eprintln!("usb: g1 rebind failed; restarting adbd");
        let _ = std::process::Command::new("/etc/init.d/adbd")
            .arg("restart")
            .status();
        std::thread::sleep(Duration::from_secs(1));
        if let Err(error) = fs::write(format!("{G1}/UDC"), format!("{}\n", self.udc)) {
            eprintln!("usb: g1 rebind still failed: {error}");
            return false;
        }
        true
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        self.restore();
    }
}

fn read_udc() -> io::Result<String> {
    let mut entries = fs::read_dir("/sys/class/udc")?;
    let entry = entries
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no UDC"))??;
    Ok(entry.file_name().to_string_lossy().into_owned())
}

fn read_snapshot() -> Snapshot {
    let mut snapshot = Snapshot::default();
    for path in ATTRS {
        if let Ok(value) = fs::read_to_string(format!("{G1}/{path}")) {
            snapshot
                .attrs
                .push(((*path).to_string(), value.trim().to_string()));
        }
    }
    for path in STRINGS {
        if let Some(value) = read_string(path) {
            snapshot.strings.push(((*path).to_string(), value));
        }
    }
    if let Ok(entries) = fs::read_dir(format!("{G1}/configs/c.1")) {
        for entry in entries.flatten() {
            if entry
                .file_type()
                .map(|kind| kind.is_symlink())
                .unwrap_or(false)
            {
                snapshot
                    .links
                    .push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    snapshot.links.sort();
    snapshot
}

fn read_string(path: &str) -> Option<String> {
    fs::read_to_string(format!("{G1}/{path}"))
        .ok()
        .map(|value| value.trim_end_matches('\n').to_string())
}

fn write_attr(path: &str, value: &str) -> io::Result<()> {
    fs::write(format!("{G1}/{path}"), format!("{value}\n"))
}

fn write_string(path: &str, value: &str) -> io::Result<()> {
    fs::write(format!("{G1}/{path}"), format!("{value}\n"))
}

fn remove_stale_ffs() {
    umount_ffs();
    let _ = fs::remove_file(format!("{G1}/configs/c.1/ffs.{FFS_NAME}"));
    let _ = fs::remove_dir(format!("{G1}/functions/ffs.{FFS_NAME}"));
    let _ = fs::remove_dir(FFS_DIR);
}

fn mount_ffs() -> io::Result<()> {
    fs::create_dir_all(FFS_DIR)?;
    let source = CString::new(FFS_NAME)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid FFS name"))?;
    let target = CString::new(FFS_DIR)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid FFS path"))?;
    let fstype = CString::new("functionfs")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid FFS type"))?;
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn umount_ffs() {
    let _ = CString::new(FFS_DIR).map(|target| unsafe {
        libc::umount2(target.as_ptr(), 0);
    });
}

fn open_ep0() -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(format!("{FFS_DIR}/ep0"))
}

fn open_ep1() -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(format!("{FFS_DIR}/ep1"))
}

fn wait_open_ep1() -> io::Result<File> {
    let path = format!("{FFS_DIR}/ep1");
    for _ in 0..200 {
        if Path::new(&path).exists() {
            return open_ep1();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "ep1 did not appear after the FunctionFS descriptors",
    ))
}

fn write_once(fd: RawFd, data: &[u8]) -> io::Result<()> {
    let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n as usize != data.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("partial FunctionFS write: {n}/{}", data.len()),
        ));
    }
    Ok(())
}

/// A stable serial for the host's device cache, never the SoC serial.
fn stable_serial() -> String {
    let path = data_dir().join("usb-serial");
    if let Ok(text) = fs::read_to_string(&path) {
        let text = text.trim();
        if text.len() == 16 && text.chars().all(|c| c.is_ascii_hexdigit()) {
            return text.to_string();
        }
    }

    let serial = random_hex();
    if let Err(error) = fs::create_dir_all(data_dir()).and_then(|_| fs::write(&path, &serial)) {
        eprintln!("usb: could not persist the serial number: {error}");
    }
    serial
}

fn random_hex() -> String {
    let mut bytes = [0u8; 8];
    if let Err(error) = File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes))
    {
        eprintln!("usb: /dev/urandom unavailable: {error}");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        bytes = (now.as_nanos() as u64 ^ ((std::process::id() as u64) << 32)).to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
