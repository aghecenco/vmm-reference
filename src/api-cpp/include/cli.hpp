// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

#pragma once

#include "rust/cxx.h"

#include <memory>
#include <regex>

namespace api {
    // Some day, we might be able to `pub use` these from somewhere else.
    struct KernelConfig;
    struct MemoryConfig;
    struct VcpuConfig;
    struct VMMConfig;

    class CliCpp {
    public:
        CliCpp(const rust::Vec<rust::String>& cmdline_args);
        bool launch(VMMConfig& vmm_config) const;

    private:
        std::vector<std::string> m_cmdline_args;
        std::regex m_rgx_mem, m_rgx_kern, m_rgx_vcpu;

        KernelConfig parse_kernel() const;
        MemoryConfig parse_memory() const;
        VcpuConfig parse_vcpu() const;
    };

    std::unique_ptr<CliCpp> new_cli(const rust::Vec<rust::String>& cmdline_args);
}
