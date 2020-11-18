// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

#![cfg(target_arch = "x86_64")]

//! Reference VMM built with rust-vmm components and minimal glue.
#![deny(missing_docs)]

use std::convert::TryFrom;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, stdin, stdout, Write};
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use event_manager::{EventManager, EventSubscriber, MutEventSubscriber, SubscriberOps};
use kvm_bindings::{KVM_API_VERSION, KVM_MAX_CPUID_ENTRIES};
use kvm_ioctls::{
    Cap::{self, Ioeventfd, Irqchip, Irqfd, UserMemory},
    Kvm,
};
use linux_loader::bootparam::boot_params;
use linux_loader::cmdline::{self, Cmdline};
use linux_loader::configurator::{
    self, linux::LinuxBootConfigurator, BootConfigurator, BootParams,
};
use linux_loader::loader::{self, elf::Elf, load_cmdline, KernelLoader, KernelLoaderResult};
use vm_device::bus::{BusManager, PioAddress};
use vm_device::device_manager::IoManager;
use vm_device::resources::Resource;
use vm_memory::{GuestAddress, GuestMemoryMmap};
use vm_superio::Serial;
use vm_vcpu::vcpu::{cpuid::filter_cpuid, VcpuState};
use vm_vcpu::vm::{self, KvmVm, VmState};
use vmm_sys_util::{eventfd::EventFd, terminal::Terminal};

mod boot;
use boot::*;

mod config;
pub use config::*;

mod devices;
use devices::{SerialWrapper, WithInterruptNotification};

/// First address past 32 bits.
const FIRST_ADDR_PAST_32BITS: u64 = 1 << 32;
/// Size of the MMIO gap.
const MEM_32BIT_GAP_SIZE: u64 = 768 << 20;
/// The start of the memory area reserved for MMIO devices.
const MMIO_MEM_START: u64 = FIRST_ADDR_PAST_32BITS - MEM_32BIT_GAP_SIZE;
/// Address of the zeropage, where Linux kernel boot parameters are written.
const ZEROPG_START: u64 = 0x7000;
/// Address where the kernel command line is written.
const CMDLINE_START: u64 = 0x0002_0000;

/// VMM memory related errors.
#[derive(Debug)]
pub enum MemoryError {
    /// Not enough memory slots.
    NotEnoughMemorySlots,
    /// Failed to configure guest memory.
    VmMemory(vm_memory::Error),
}

/// VMM errors.
#[derive(Debug)]
pub enum Error {
    /// Failed to write boot parameters to guest memory.
    BootConfigure(configurator::Error),
    /// Error configuring boot parameters.
    BootParam(boot::Error),
    /// Error configuring the kernel command line.
    Cmdline(cmdline::Error),
    /// Error setting up devices.
    Device(devices::Error),
    /// Event management error.
    EventManager(event_manager::Error),
    /// I/O error.
    IO(io::Error),
    /// Failed to load kernel.
    KernelLoad(loader::Error),
    /// Invalid KVM API version.
    KvmApiVersion(i32),
    /// Unsupported KVM capability.
    KvmCap(Cap),
    /// Error issuing an ioctl to KVM.
    KvmIoctl(kvm_ioctls::Error),
    /// Memory error.
    Memory(MemoryError),
    /// VM errors.
    Vm(vm::Error),
}

impl std::convert::From<vm::Error> for Error {
    fn from(vm_error: vm::Error) -> Self {
        Self::Vm(vm_error)
    }
}

/// Dedicated [`Result`](https://doc.rust-lang.org/std/result/) type.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) trait DormantDevice: WithInterruptNotification + MutEventSubscriber {}

impl<W: Write> DormantDevice for SerialWrapper<W> {}
impl<T: DormantDevice + EventSubscriber + ?Sized> DormantDevice for Arc<T> {}
impl<T: DormantDevice + MutEventSubscriber + ?Sized> DormantDevice for Mutex<T> {}

/// A live VMM.
pub struct VMM {
    kvm: Kvm,
    vm: KvmVm,
    guest_memory: GuestMemoryMmap,
    // An Option, of all things, because it needs to be mutable while adding devices (so it can't
    // be in an Arc), then it needs to be packed in an Arc to share it across vCPUs. An Option
    // allows us to .take() it when starting the vCPUs.
    device_mgr: Option<IoManager>,
    // Arc<Mutex<>> because the same device (a dyn DevicePio/DeviceMmio from IoManager's
    // perspective, and a dyn MutEventSubscriber from EventManager's) is managed by the 2 entities,
    // and isn't Copy-able; so once one of them gets ownership, the other one can't anymore.
    event_mgr: EventManager<Arc<Mutex<dyn MutEventSubscriber>>>,
    dormant_devices: Vec<Arc<Mutex<dyn DormantDevice>>>,
}

impl VMM {
    /// Create a new VMM.
    pub fn new(config: &VMMConfig) -> Result<Self> {
        let kvm = Kvm::new().map_err(Error::KvmIoctl)?;

        // Check that the KVM on the host is supported.
        let kvm_api_ver = kvm.get_api_version();
        if kvm_api_ver != KVM_API_VERSION as i32 {
            return Err(Error::KvmApiVersion(kvm_api_ver));
        }
        VMM::check_kvm_capabilities(&kvm)?;

        let guest_memory = VMM::create_guest_memory(&config.memory_config)?;

        // Create the KvmVm.
        let vm_state = VmState {
            num_vcpus: config.vcpu_config.num_vcpus,
        };
        let vm = KvmVm::new(&kvm, vm_state, &guest_memory)?;

        let mut vmm = VMM {
            vm,
            kvm,
            guest_memory,
            device_mgr: Some(IoManager::new()),
            event_mgr: EventManager::new().map_err(Error::EventManager)?,
            dormant_devices: vec![],
        };

        vmm.configure_pio_devices()?;

        Ok(vmm)
    }

    // Create guest memory regions.
    // On x86_64, they surround the MMIO gap (dedicated space for MMIO device slots) if the
    // configured memory size exceeds the latter's starting address.
    fn create_guest_memory(memory_config: &MemoryConfig) -> Result<GuestMemoryMmap> {
        let mem_size = ((memory_config.mem_size_mib as u64) << 20) as usize;
        let mem_regions = match mem_size.checked_sub(MMIO_MEM_START as usize) {
            // Guest memory fits before the MMIO gap.
            None | Some(0) => vec![(GuestAddress(0), mem_size)],
            // Guest memory extends beyond the MMIO gap.
            Some(remaining) => vec![
                (GuestAddress(0), MMIO_MEM_START as usize),
                (GuestAddress(FIRST_ADDR_PAST_32BITS), remaining),
            ],
        };

        // Create guest memory from regions.
        GuestMemoryMmap::from_ranges(&mem_regions)
            .map_err(|e| Error::Memory(MemoryError::VmMemory(e)))
    }

    /// Configure guest kernel.
    ///
    /// # Arguments
    ///
    /// * `kernel_cfg` - [`KernelConfig`](struct.KernelConfig.html) struct containing kernel
    ///                  configurations.
    pub fn configure_kernel(&mut self, kernel_cfg: KernelConfig) -> Result<KernelLoaderResult> {
        let mut kernel_image = File::open(kernel_cfg.path).map_err(Error::IO)?;
        let zero_page_addr = GuestAddress(ZEROPG_START);

        // Load the kernel into guest memory.
        let kernel_load = Elf::load(
            &self.guest_memory,
            None,
            &mut kernel_image,
            Some(GuestAddress(kernel_cfg.himem_start)),
        )
        .map_err(Error::KernelLoad)?;

        // Generate boot parameters.
        let mut bootparams = build_bootparams(
            &self.guest_memory,
            GuestAddress(kernel_cfg.himem_start),
            GuestAddress(MMIO_MEM_START),
            GuestAddress(FIRST_ADDR_PAST_32BITS),
        )
        .map_err(Error::BootParam)?;

        // Add the kernel command line to the boot parameters.
        bootparams.hdr.cmd_line_ptr = CMDLINE_START as u32;
        bootparams.hdr.cmdline_size = kernel_cfg.cmdline.len() as u32 + 1;

        // Load the kernel command line into guest memory.
        let mut cmdline = Cmdline::new(kernel_cfg.cmdline.len() + 1);
        cmdline
            .insert_str(kernel_cfg.cmdline)
            .map_err(Error::Cmdline)?;
        load_cmdline(
            &self.guest_memory,
            GuestAddress(CMDLINE_START),
            // Safe because we know the command line string doesn't contain any 0 bytes.
            unsafe { &CString::from_vec_unchecked(cmdline.into()) },
        )
        .map_err(Error::KernelLoad)?;

        // Write the boot parameters in the zeropage.
        LinuxBootConfigurator::write_bootparams::<GuestMemoryMmap>(
            &BootParams::new::<boot_params>(&bootparams, zero_page_addr),
            &self.guest_memory,
        )
        .map_err(Error::BootConfigure)?;

        Ok(kernel_load)
    }

    /// Configure PIO devices.
    ///
    /// This sets up the following PIO devices:
    /// * `x86_64`: serial console
    /// * `aarch64`: N/A
    fn configure_pio_devices(&mut self) -> Result<()> {
        // Create the serial console.
        let interrupt_evt = EventFd::new(libc::EFD_NONBLOCK).map_err(Error::IO)?;
        let serial = Arc::new(Mutex::new(SerialWrapper(Serial::new(
            interrupt_evt.try_clone().map_err(Error::IO)?,
            stdout(),
        ))));

        // Put it on the bus.
        // Safe to use expect() because the device manager is instantiated in new(), there's no
        // default implementation, and the field is private inside the VMM struct.
        self.device_mgr
            .as_mut()
            .expect("Missing device manager")
            .register_pio_resources(
                serial.clone(),
                &[Resource::PioAddressRange {
                    base: 0x3f8,
                    size: 0x8,
                }],
            )
            .unwrap();

        Ok(())
    }

    /// Creates guest vCPUs.
    ///
    /// # Arguments
    ///
    /// * `vcpu_cfg` - [`VcpuConfig`](struct.VcpuConfig.html) struct containing vCPU configurations.
    /// * `kernel_load` - address where the kernel is loaded in guest memory.
    pub fn create_vcpus(&mut self, vcpu_cfg: VcpuConfig, kernel_load: GuestAddress) -> Result<()> {
        // Safe to use expect() because the device manager is instantiated in new(), there's no
        // default implementation, and the field is private inside the VMM struct.
        let shared_device_mgr = Arc::new(self.device_mgr.take().expect("Missing device manager"));
        let base_cpuid = self
            .kvm
            .get_supported_cpuid(KVM_MAX_CPUID_ENTRIES)
            .map_err(Error::KvmIoctl)?;

        for index in 0..vcpu_cfg.num_vcpus {
            // Set CPUID.
            let mut cpuid = base_cpuid.clone();
            filter_cpuid(
                &self.kvm,
                index as usize,
                vcpu_cfg.num_vcpus as usize,
                &mut cpuid,
            );

            let vcpu_state = VcpuState {
                kernel_load_addr: kernel_load,
                cpuid,
                id: index,
                zero_page_start: ZEROPG_START,
            };
            self.vm
                .create_vcpu(shared_device_mgr.clone(), vcpu_state, &self.guest_memory)?;
        }

        Ok(())
    }

    /// Run the VMM.
    pub fn run(&mut self) {
        if stdin().lock().set_raw_mode().is_err() {
            eprintln!("Failed to set raw mode on terminal. Stdin will echo.");
        }

        // Bring the devices to life (or, rather, to the hypervisor).
        for device in self.dormant_devices.drain(..) {
            // IRQ fd.
            <dyn DormantDevice as WithInterruptNotification>::setup_interrupt_notif(
                &device,
                &mut self.vm,
            )
            .unwrap();

            // std::sync::Arc<std::sync::Mutex<(dyn event_manager::MutEventSubscriber + 'static)>>
            self.event_mgr.add_subscriber(device);
        }

        // TODO: should we handle this in another way rather than a panic?
        self.vm.run().expect("Cannot start vcpus.");

        loop {
            match self.event_mgr.run() {
                Ok(_) => (),
                Err(e) => eprintln!("Failed to handle events: {:?}", e),
            }
        }
    }

    fn check_kvm_capabilities(kvm: &Kvm) -> Result<()> {
        let capabilities = vec![Irqchip, Ioeventfd, Irqfd, UserMemory];

        // Check that all desired capabilities are supported.
        if let Some(c) = capabilities
            .iter()
            .find(|&capability| !kvm.check_extension(*capability))
        {
            Err(Error::KvmCap(*c))
        } else {
            Ok(())
        }
    }
}

impl TryFrom<VMMConfig> for VMM {
    type Error = Error;

    fn try_from(config: VMMConfig) -> Result<Self> {
        let mut vmm = VMM::new(&config)?;
        let kernel_load = vmm.configure_kernel(config.kernel_config)?;
        vmm.create_vcpus(config.vcpu_config, kernel_load.kernel_load)?;
        Ok(vmm)
    }
}
