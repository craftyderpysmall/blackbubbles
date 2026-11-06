//! Raw HCI commands for LE advertising with a chosen random address.
//!
//! Needs `CAP_NET_RAW` (and an adapter that BlueZ has powered on).

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

const LE: u16 = 0x08;

pub const fn opcode(ocf: u16) -> u16 {
    (LE << 10) | ocf
}

pub const SET_RANDOM_ADDRESS: u16 = opcode(0x0005);
pub const SET_ADV_PARAMS: u16 = opcode(0x0006);
pub const SET_ADV_DATA: u16 = opcode(0x0008);
pub const SET_ADV_ENABLE: u16 = opcode(0x000a);
pub const SET_EXT_ADV_SET_RANDOM_ADDRESS: u16 = opcode(0x0035);
pub const SET_EXT_ADV_PARAMS: u16 = opcode(0x0036);
pub const SET_EXT_ADV_DATA: u16 = opcode(0x0037);
pub const SET_EXT_ADV_ENABLE: u16 = opcode(0x0039);
pub const REMOVE_ADV_SET: u16 = opcode(0x003c);

pub const STATUS_UNKNOWN_COMMAND: u8 = 0x01;
pub const STATUS_COMMAND_DISALLOWED: u8 = 0x0c;

/// Our extended advertising set (high, to stay clear of BlueZ's own instances).
pub const ADV_HANDLE: u8 = 0x0e;
/// 1 s in 0.625 ms units.
pub const INTERVAL: u16 = 0x0640;

/// Encode an HCI command packet.
pub fn command_packet(opcode: u16, params: &[u8]) -> Vec<u8> {
    let mut out = vec![0x01];
    out.extend_from_slice(&opcode.to_le_bytes());
    out.push(params.len() as u8);
    out.extend_from_slice(params);
    out
}

/// BLE addresses go on the wire least significant byte first.
pub fn address_le(address: &[u8; 6]) -> [u8; 6] {
    let mut out = *address;
    out.reverse();
    out
}

pub fn legacy_params() -> Vec<u8> {
    let mut p = Vec::with_capacity(15);
    p.extend_from_slice(&INTERVAL.to_le_bytes());
    p.extend_from_slice(&INTERVAL.to_le_bytes());
    p.push(0x03); // ADV_NONCONN_IND
    p.push(0x01); // own address: random
    p.push(0x00); // peer address type
    p.extend_from_slice(&[0; 6]);
    p.push(0x07); // all three channels
    p.push(0x00); // no filter
    p
}

pub fn extended_params() -> Vec<u8> {
    let mut p = Vec::with_capacity(25);
    p.push(ADV_HANDLE);
    p.extend_from_slice(&0x0010u16.to_le_bytes()); // legacy PDU, non-connectable, non-scannable
    let interval = u32::from(INTERVAL).to_le_bytes();
    p.extend_from_slice(&interval[..3]);
    p.extend_from_slice(&interval[..3]);
    p.push(0x07);
    p.push(0x01); // own address: random
    p.push(0x00);
    p.extend_from_slice(&[0; 6]);
    p.push(0x00); // no filter
    p.push(0x7f); // no tx power preference
    p.push(0x01); // primary PHY 1M
    p.push(0x00); // secondary max skip
    p.push(0x01); // secondary PHY 1M
    p.push(0x00); // SID
    p.push(0x00); // no scan request notifications
    p
}

#[cfg(target_os = "linux")]
pub struct Socket {
    fd: i32,
}

#[cfg(target_os = "linux")]
impl Socket {
    pub fn open(dev: u16) -> Result<Self> {
        const AF_BLUETOOTH: i32 = 31;
        const BTPROTO_HCI: i32 = 1;
        const SOL_HCI: i32 = 0;
        const HCI_FILTER: i32 = 2;

        #[repr(C)]
        struct SockaddrHci {
            family: u16,
            dev: u16,
            channel: u16,
        }
        #[repr(C)]
        struct Filter {
            type_mask: u32,
            event_mask: [u32; 2],
            opcode: u16,
        }

        let fd = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC, BTPROTO_HCI) };
        if fd < 0 {
            bail!("cannot open an HCI socket: {}", std::io::Error::last_os_error());
        }
        let socket = Self { fd };
        let addr = SockaddrHci { family: AF_BLUETOOTH as u16, dev, channel: 0 };
        let rc = unsafe {
            libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<SockaddrHci>() as u32)
        };
        if rc < 0 {
            bail!("cannot bind hci{dev}: {} (is the adapter present?)", std::io::Error::last_os_error());
        }
        // Receive Command Complete (0x0e) and Command Status (0x0f) events only.
        let filter = Filter { type_mask: 1 << 0x04, event_mask: [(1 << 0x0e) | (1 << 0x0f), 0], opcode: 0 };
        let rc = unsafe {
            libc::setsockopt(fd, SOL_HCI, HCI_FILTER, &filter as *const _ as *const libc::c_void, std::mem::size_of::<Filter>() as u32)
        };
        if rc < 0 {
            bail!("cannot set HCI filter: {}", std::io::Error::last_os_error());
        }
        Ok(socket)
    }

    /// Send a command and return the controller's status byte.
    pub fn command(&self, opcode: u16, params: &[u8]) -> Result<u8> {
        let packet = command_packet(opcode, params);
        let written = unsafe { libc::write(self.fd, packet.as_ptr().cast(), packet.len()) };
        if written != packet.len() as isize {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EPERM) {
                bail!("permission denied sending HCI commands; run as root or grant CAP_NET_RAW");
            }
            bail!("HCI write failed: {err}");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buf = [0u8; 260];
        while Instant::now() < deadline {
            let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            let wait = deadline.saturating_duration_since(Instant::now()).as_millis() as i32;
            if unsafe { libc::poll(&mut pfd, 1, wait) } <= 0 {
                break;
            }
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n < 4 || buf[0] != 0x04 {
                continue;
            }
            let event = &buf[..n as usize];
            match event[1] {
                0x0e if event.len() >= 7 && u16::from_le_bytes([event[4], event[5]]) == opcode => return Ok(event[6]),
                0x0f if event.len() >= 7 && u16::from_le_bytes([event[5], event[6]]) == opcode => return Ok(event[3]),
                _ => {}
            }
        }
        bail!("no reply from the controller for opcode {opcode:#06x}")
    }

    fn check(&self, what: &str, opcode: u16, params: &[u8]) -> Result<()> {
        match self.command(opcode, params).with_context(|| what.to_string())? {
            0 => Ok(()),
            status => bail!("{what} failed with HCI status {status:#04x}"),
        }
    }

    fn legacy(&self, address: &[u8; 6], data: &[u8; 31]) -> Result<u8> {
        let _ = self.command(SET_ADV_ENABLE, &[0x00]);
        for (opcode, params) in [
            (SET_RANDOM_ADDRESS, address_le(address).to_vec()),
            (SET_ADV_PARAMS, legacy_params()),
            (SET_ADV_DATA, [&[31u8][..], data].concat()),
            (SET_ADV_ENABLE, vec![0x01]),
        ] {
            let status = self.command(opcode, &params)?;
            if status != 0 {
                return Ok(status);
            }
        }
        Ok(0)
    }

    fn extended(&self, address: &[u8; 6], data: &[u8; 31]) -> Result<()> {
        let _ = self.command(SET_EXT_ADV_ENABLE, &[0x00, 0x01, ADV_HANDLE, 0, 0, 0]);
        let _ = self.command(REMOVE_ADV_SET, &[ADV_HANDLE]);
        self.check("extended advertising parameters", SET_EXT_ADV_PARAMS, &extended_params())?;
        self.check("advertising set address", SET_EXT_ADV_SET_RANDOM_ADDRESS, &[&[ADV_HANDLE][..], &address_le(address)].concat())?;
        self.check("extended advertising data", SET_EXT_ADV_DATA, &[&[ADV_HANDLE, 0x03, 0x01, 31][..], data].concat())?;
        self.check("enable extended advertising", SET_EXT_ADV_ENABLE, &[0x01, 0x01, ADV_HANDLE, 0, 0, 0])
    }

    /// Start (or switch) advertising. Uses legacy commands, falling back to extended
    /// advertising on controllers where the kernel has switched to it.
    pub fn advertise(&self, address: &[u8; 6], data: &[u8; 31]) -> Result<&'static str> {
        match self.legacy(address, data)? {
            0 => Ok("legacy"),
            STATUS_COMMAND_DISALLOWED | STATUS_UNKNOWN_COMMAND => self.extended(address, data).map(|()| "extended"),
            status => bail!("advertising failed with HCI status {status:#04x}"),
        }
    }

    pub fn stop(&self) {
        let _ = self.command(SET_ADV_ENABLE, &[0x00]);
        let _ = self.command(SET_EXT_ADV_ENABLE, &[0x00, 0x01, ADV_HANDLE, 0, 0, 0]);
        let _ = self.command(REMOVE_ADV_SET, &[ADV_HANDLE]);
    }
}

#[cfg(target_os = "linux")]
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_commands() {
        assert_eq!(SET_ADV_DATA, 0x2008);
        assert_eq!(SET_EXT_ADV_PARAMS, 0x2036);
        assert_eq!(command_packet(SET_ADV_ENABLE, &[1]), [0x01, 0x0a, 0x20, 0x01, 0x01]);
        assert_eq!(address_le(&[1, 2, 3, 4, 5, 6]), [6, 5, 4, 3, 2, 1]);
        assert_eq!(legacy_params().len(), 15);
        assert_eq!(extended_params().len(), 25);
    }
}
