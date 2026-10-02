use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

use crate::Error;

pub const MAPS: [&str; 4] = ["SETTINGS", "TARGETS", "EXCLUSIONS", "ORIGINS"];

const CAPACITY: usize = 32;

pub struct Bundle {
    pub maps: Vec<OwnedFd>,
    pub links: Vec<OwnedFd>,
}

impl Bundle {
    pub fn send(&self, channel: &UnixStream) -> Result<(), Error> {
        let fds: Vec<_> = self
            .maps
            .iter()
            .chain(&self.links)
            .map(AsFd::as_fd)
            .collect();
        let header = [self.maps.len() as u8, self.links.len() as u8];

        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(CAPACITY))];
        let mut control = SendAncillaryBuffer::new(&mut space);

        if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
            return Err(Error::Handoff);
        }

        sendmsg(
            channel,
            &[IoSlice::new(&header)],
            &mut control,
            SendFlags::empty(),
        )?;
        Ok(())
    }

    pub fn receive(channel: &UnixStream) -> Result<Self, Error> {
        let mut header = [0u8; 2];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(CAPACITY))];
        let mut control = RecvAncillaryBuffer::new(&mut space);

        let message = recvmsg(
            channel,
            &mut [IoSliceMut::new(&mut header)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        )?;

        let mut maps: Vec<OwnedFd> = control
            .drain()
            .filter_map(|message| match message {
                RecvAncillaryMessage::ScmRights(fds) => Some(fds),
                _ => None,
            })
            .flatten()
            .collect();

        let [map_count, link_count] = header.map(usize::from);

        if message.bytes != header.len() || maps.len() != map_count + link_count {
            return Err(Error::Handoff);
        }

        let links = maps.split_off(map_count);
        Ok(Self { maps, links })
    }
}
