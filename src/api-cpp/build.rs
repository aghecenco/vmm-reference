// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

fn main() {
    cxx_build::bridge("src/lib.rs")
        .file("src/cli.cpp")
        .flag_if_supported("-std=c++14")
        .compile("api-cpp");
}
