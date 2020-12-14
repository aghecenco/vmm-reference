// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause
#[cfg(target_arch = "x86_64")]
use std::convert::TryFrom;
#[cfg(target_arch = "x86_64")]
use std::env;

#[cfg(target_arch = "x86_64")]
use api_cpp::*;
#[cfg(target_arch = "x86_64")]
use vmm::VMM;

fn main() {
    #[cfg(target_arch = "x86_64")]
    {
        let cli = ffi::new_cli(&env::args().collect::<Vec<String>>());
        let mut vmm_config = ffi::VMMConfig::default();
        if !cli.launch(&mut vmm_config) {
            eprintln!("Failed to parse VMM configuration!");
        } else {
            let converted_config: vmm::VMMConfig = vmm_config.into();
            let mut vmm =
                VMM::try_from(converted_config).expect("Failed to create VMM from configurations");
            // For now we are just unwrapping here, in the future we might use a nicer way of
            // handling errors such as pretty printing them.
            vmm.run().unwrap();
        }
    }
    #[cfg(target_arch = "aarch64")]
    println!("Reference VMM under construction!")
}
