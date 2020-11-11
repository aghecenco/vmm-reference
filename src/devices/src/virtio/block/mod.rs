// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

mod device;
mod inorder_handler;
mod queue_handler;

use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use event_manager::Error as EvmgrError;
use vm_device::bus;
use vm_virtio::block::stdio_executor;
use vmm_sys_util::errno;

use crate::virtio::CommonArgs;

pub use device::Block;

// TODO: Move relevant defines to vm-virtio crate.

// Block device ID as defined by the standard.
pub const BLOCK_DEVICE_ID: u32 = 2;

// Block device FLUSH feature.
pub const VIRTIO_BLK_F_FLUSH: u64 = 9;

// The sector size is 512 bytes (1 << 9).
const SECTOR_SHIFT: u8 = 9;

#[derive(Debug)]
pub enum Error {
    AlreadyActivated,
    Backend(stdio_executor::Error),
    BadFeatures(u64),
    Bus(bus::Error),
    Endpoint(EvmgrError),
    EventFd(io::Error),
    OpenFile(io::Error),
    QueuesNotValid,
    RegisterIoevent(errno::Error),
    RegisterIrqfd(errno::Error),
    Seek(io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// TODO: Add a helper abstraction to rust-vmm for building the device configuration space.
// The one we build below for the block device contains the minimally required `capacity` member,
// but other fields can be present as well depending on the negotiated features.
fn build_config_space<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    // TODO: right now, the file size is computed by the StdioBackend as well. Maybe we should
    // create the backend as early as possible, and get the size information from there.
    let file_size = File::open(path)
        .map_err(Error::OpenFile)?
        .seek(SeekFrom::End(0))
        .map_err(Error::Seek)?;
    // If the file size is actually not a multiple of sector size, then data at the very end
    // will be ignored.
    let num_sectors = file_size >> SECTOR_SHIFT;
    // This has to be in little endian btw.
    Ok(num_sectors.to_le_bytes().to_vec())
}

// Arguments required when building a block device.
// TODO: Add read-only operation support as a quick next step.
pub struct BlockArgs<'a, M, B> {
    pub common: CommonArgs<'a, M, B>,
    pub file_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Write;

    use vmm_sys_util::tempfile::TempFile;

    use super::*;
    use std::convert::TryInto;

    #[test]
    fn test_build_config_space() {
        let tmp = TempFile::new().unwrap();

        // TODO: There no way to get (via `Deref` or something else) the file associated with
        // tmp without explicitly opening the path?
        let mut file = OpenOptions::new().write(true).open(tmp.as_path()).unwrap();

        let sector = [1u8; 512];
        let num_sectors = 1024;

        for _ in 0..num_sectors {
            file.write_all(&sector).unwrap();
        }

        let config_space = build_config_space(tmp.as_path()).unwrap();

        // The config space can only be populated with the `capacity` field for now.
        assert_eq!(config_space.len(), 8);

        // The `try_into` attempts to convert the byte slice into a `[u8; 8]` array, as required
        // by `u64::from_le_bytes`.
        let capacity = u64::from_le_bytes(config_space[..8].try_into().unwrap());
        assert_eq!(capacity, num_sectors);
    }
}
