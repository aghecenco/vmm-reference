// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

#include "api-cpp/include/cli.hpp"
#include "api-cpp/src/lib.rs.h"

#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>

#include <sys/stat.h>
#include <unistd.h>

namespace api {
    CliCpp::CliCpp(const rust::Vec<rust::String>& cmdline_args) {
        // No copy constructors yet. And no initializer-list constructors either.
        for(auto arg: cmdline_args) {
            this->m_cmdline_args.push_back(std::string(arg));
        }
        this->m_rgx_mem = std::regex("size_mib=([0-9]+)");
        this->m_rgx_kern = std::regex("path=(.+)");
        this->m_rgx_vcpu = std::regex("num=([0-9]+)");
        std::cout << "Rust made me a better C++ programmer" << std::endl;
    }

    bool CliCpp::launch(VMMConfig& vmm_config) const {
        try {
            vmm_config.kernel_config = this->parse_kernel();
            vmm_config.memory_config = this->parse_memory();
            vmm_config.vcpu_config = this->parse_vcpu();

            return true;
        } catch(const std::invalid_argument& err) {
            std::cerr << "Failed to parse memory: " << err.what() << std::endl;
            return false;
        }
    }

    KernelConfig CliCpp::parse_kernel() const{
        KernelConfig kern_cfg {
            "i8042.nokbd reboot=t panic=1 pci=off", // cmdline
            "",                                     // path
            0x00100000                              // highmem
        };

        for(auto it = this->m_cmdline_args.begin(); it != this->m_cmdline_args.end(); ++it) {
            if(*it == "--kernel") {
                // The next token after `it` contains the value for `--kernel`.
                const std::string kern_val(*++it);
                std::smatch match;
                if(!std::regex_search(kern_val.begin(), kern_val.end(), match, this->m_rgx_kern)) {
                    throw std::invalid_argument(kern_val);
                }
                if(match.size() != 2) {
                    throw std::invalid_argument(kern_val);
                }
                std::string path(match[1]);
                struct stat buffer;
                if(stat(path.c_str(), &buffer) < 0) {
                    throw std::invalid_argument(path);
                }
                kern_cfg.path = path;
                break;
            }
        }

        if(kern_cfg.path.size() == 0) {
            std::ostringstream oss;
            std::copy(
                this->m_cmdline_args.begin(),
                this->m_cmdline_args.end(),
                std::ostream_iterator<std::string>(oss, " ")
            );
            throw std::invalid_argument(oss.str());
        }

        return kern_cfg;
    }

    MemoryConfig CliCpp::parse_memory() const{
        MemoryConfig mem_cfg { 128 };

        for(auto it = this->m_cmdline_args.begin(); it != this->m_cmdline_args.end(); ++it) {
            if(*it == "--memory") {
                // The next token after `it` contains the value for `--memory`.
                const std::string mem_val(*++it);
                std::smatch match;
                if(!std::regex_search(mem_val.begin(), mem_val.end(), match, this->m_rgx_mem)) {
                    throw std::invalid_argument(mem_val);
                }
                if(match.size() != 2) {
                    throw std::invalid_argument(mem_val);
                }
                int memsz = std::stoi(match[1]);
                if(memsz <= 0) {
                    throw std::invalid_argument(match[1]);
                }
                mem_cfg.size_mib = memsz;
                break;
            }
        }

        return mem_cfg;
    }

    VcpuConfig CliCpp::parse_vcpu() const{
        VcpuConfig vcpu_cfg { 1 };

        for(auto it = this->m_cmdline_args.begin(); it != this->m_cmdline_args.end(); ++it) {
            if(*it == "--vcpu") {
                // The next token after `it` contains the value for `--vcpu`.
                const std::string vcpu_val(*++it);
                std::smatch match;
                if(!std::regex_search(vcpu_val.begin(), vcpu_val.end(), match, this->m_rgx_vcpu)) {
                    throw std::invalid_argument(vcpu_val);
                }
                if(match.size() != 2) {
                    throw std::invalid_argument(vcpu_val);
                }
                int num_vcpus = std::stoi(match[1]);
                if(num_vcpus <= 0 || num_vcpus > 256) {
                    throw std::invalid_argument(match[1]);
                }
                vcpu_cfg.num = num_vcpus;
                break;
            }
        }

        return vcpu_cfg;
    }

    std::unique_ptr<CliCpp> new_cli(const rust::Vec<rust::String>& cmdline_args) {
        return std::make_unique<CliCpp>(cmdline_args);
    }
}
