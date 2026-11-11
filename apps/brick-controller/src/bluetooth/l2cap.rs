// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Raw Bluetooth L2CAP sockets: the two HID channels.
//!
//! libc has no Bluetooth types, so the address structure is declared here.

use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_L2CAP: libc::c_int = 0;

/// The kernel's `sockaddr_l2`, declared here because libc has no Bluetooth
/// types.
///
/// `psm` and the address are little-endian on the wire. `Address` stores the
/// printed order, so every crossing of this boundary goes through
/// `to_wire`/`from_wire`; mixing the two orders is the classic Bluetooth
/// socket bug.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    /// Little-endian: the last octet of the printed address comes first.
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

use super::address::Address;

#[cfg(target_os = "linux")]
const SOCK_CLOEXEC: libc::c_int = libc::SOCK_CLOEXEC;
#[cfg(not(target_os = "linux"))]
const SOCK_CLOEXEC: libc::c_int = 0;

fn socket() -> io::Result<OwnedFd> {
    let fd = unsafe {
        libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_SEQPACKET | SOCK_CLOEXEC,
            BTPROTO_L2CAP,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(not(target_os = "linux"))]
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn accept_cloexec(fd: RawFd, addr: *mut libc::sockaddr, len: *mut libc::socklen_t) -> libc::c_int {
    unsafe { libc::accept4(fd, addr, len, libc::SOCK_CLOEXEC) }
}

#[cfg(not(target_os = "linux"))]
fn accept_cloexec(fd: RawFd, addr: *mut libc::sockaddr, len: *mut libc::socklen_t) -> libc::c_int {
    unsafe {
        let new = libc::accept(fd, addr, len);
        if new >= 0 {
            libc::fcntl(new, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        new
    }
}

fn addr(psm: u16, bdaddr: [u8; 6]) -> SockaddrL2 {
    SockaddrL2 {
        family: AF_BLUETOOTH as _,
        psm: psm.to_le(),
        bdaddr,
        ..Default::default()
    }
}

const ADDR_LEN: libc::socklen_t = std::mem::size_of::<SockaddrL2>() as _;

/// A channel waiting for the host to connect.
pub struct Listener(OwnedFd);

impl Listener {
    /// Bind a local HID PSM on every local Bluetooth address.
    ///
    /// The backlog is one: a gamepad has a single host, and extra queued
    /// connections would only let a stale peer race the current one.
    pub fn bind(psm: u16) -> io::Result<Self> {
        let fd = socket()?;
        let a = addr(psm, [0; 6]);
        if unsafe {
            libc::bind(
                fd.as_raw_fd(),
                &a as *const _ as *const libc::sockaddr,
                ADDR_LEN,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::listen(fd.as_raw_fd(), 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }

    /// Take a waiting connection, or None if there is none yet.
    ///
    /// The caller pairs the control and interrupt channels by address: a host
    /// opens control first, then interrupt. Accepting them independently here
    /// keeps a half-open pairing from blocking the poll loop.
    pub fn accept(&self) -> io::Result<Option<(Channel, Address)>> {
        let mut a = SockaddrL2::default();
        let mut len = ADDR_LEN;
        let fd = accept_cloexec(
            self.0.as_raw_fd(),
            &mut a as *mut _ as *mut libc::sockaddr,
            &mut len,
        );
        if fd < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(e)
            };
        }
        let channel = Channel(unsafe { OwnedFd::from_raw_fd(fd) });
        Ok(Some((channel, Address::from_wire(a.bdaddr))))
    }

    pub fn set_nonblocking(&self) -> io::Result<()> {
        set_nonblocking(self.0.as_raw_fd())
    }
}

impl AsRawFd for Listener {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// One open HID channel.
pub struct Channel(OwnedFd);

impl Channel {
    /// Connect out to a host, blocking until the link is up or refused. Used to
    /// reconnect to the host already paired with, which is the device's job
    /// in the HID profile.
    pub fn connect(to: Address, psm: u16) -> io::Result<Self> {
        let fd = socket()?;
        let a = addr(psm, to.to_wire());
        if unsafe {
            libc::connect(
                fd.as_raw_fd(),
                &a as *const _ as *const libc::sockaddr,
                ADDR_LEN,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }

    /// Never blocks: with the radio behind, a full send queue is reported as
    /// WouldBlock and the caller moves on, rather than the loop that reads
    /// the pad stalling until the queue drains - felt as input lag.
    pub fn send(&self, bytes: &[u8]) -> io::Result<()> {
        let flags = libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT;
        let n = unsafe {
            libc::send(
                self.0.as_raw_fd(),
                bytes.as_ptr() as *const _,
                bytes.len(),
                flags,
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Keep the send queue as short as the kernel allows, so a report the
    /// radio cannot take yet is refused rather than queued behind the others:
    /// a queue grows when the host takes reports slower than they are made,
    /// and every press then waits behind it.
    pub fn shorten_queue(&self) -> io::Result<()> {
        // 0 is raised to the kernel's minimum.
        let size: libc::c_int = 0;
        let len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if unsafe {
            libc::setsockopt(
                self.0.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &size as *const _ as *const _,
                len,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// One message, or None if nothing is waiting. A closed channel is an
    /// error, so the caller has one place to notice the host leaving.
    pub fn recv(&self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 256];
        let n = unsafe {
            libc::recv(
                self.0.as_raw_fd(),
                buf.as_mut_ptr() as *mut _,
                buf.len(),
                libc::MSG_DONTWAIT,
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(e)
            };
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "the host closed the channel",
            ));
        }
        Ok(Some(buf[..n as usize].to_vec()))
    }
}

impl AsRawFd for Channel {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_structure_matches_the_kernels() {
        // struct sockaddr_l2 is 14 bytes on Linux.
        assert_eq!(std::mem::size_of::<SockaddrL2>(), 14);
    }

    #[test]
    fn addresses_print_as_written_and_go_out_reversed() {
        let a = Address::parse("12:34:56:78:9A:BC").unwrap();
        assert_eq!(a.to_string(), "12:34:56:78:9A:BC");
        assert_eq!(a.to_wire(), [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]);
        assert_eq!(Address::from_wire(a.to_wire()), a);
        assert_eq!(Address::parse("nonsense"), None);
        assert_eq!(Address::parse("12:34:56:78:9A"), None);
    }
}
