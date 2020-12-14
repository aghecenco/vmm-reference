// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use std::convert::Into;
use std::path::PathBuf;

use vmm;

#[cxx::bridge(namespace = "api")]
pub mod ffi {
    // Shadow structs for those in vmm.
    // Some day, maybe we'll be able to `pub use` them here instead.
    #[derive(Default)]
    pub struct KernelConfig {
        pub cmdline: String,
        pub path: String,
        pub himem_start: u64,
    }

    #[derive(Default)]
    pub struct MemoryConfig {
        pub size_mib: u32,
    }

    #[derive(Default)]
    pub struct VcpuConfig {
        pub num: u8,
    }

    #[derive(Default)]
    pub struct VMMConfig {
        pub memory_config: MemoryConfig,
        pub vcpu_config: VcpuConfig,
        pub kernel_config: KernelConfig,
    }

    unsafe extern "C++" {
        include!("api-cpp/include/cli.hpp");

        type CliCpp;

        fn launch(&self, vmm_config: &mut VMMConfig) -> bool;

        fn new_cli(cmdline_args: &Vec<String>) -> UniquePtr<CliCpp>;
    }
}

impl Into<vmm::KernelConfig> for ffi::KernelConfig {
    fn into(self) -> vmm::KernelConfig {
        vmm::KernelConfig {
            path: PathBuf::from(self.path),
            ..Default::default()
        }
    }
}

impl Into<vmm::MemoryConfig> for ffi::MemoryConfig {
    fn into(self) -> vmm::MemoryConfig {
        vmm::MemoryConfig {
            size_mib: self.size_mib,
        }
    }
}

impl Into<vmm::VcpuConfig> for ffi::VcpuConfig {
    fn into(self) -> vmm::VcpuConfig {
        vmm::VcpuConfig { num: self.num }
    }
}

impl Into<vmm::VMMConfig> for ffi::VMMConfig {
    fn into(self) -> vmm::VMMConfig {
        vmm::VMMConfig {
            kernel_config: self.kernel_config.into(),
            memory_config: self.memory_config.into(),
            vcpu_config: self.vcpu_config.into(),
        }
    }
}
