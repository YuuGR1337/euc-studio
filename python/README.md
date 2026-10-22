# EUC Studio Python

This package runs own or custom firmware inside the Rust simulator.
It uses PyO3 and maturin. It requires Python 3.11 or later.

Read [the Python guide](../docs/python.md) for setup, API details, and limits.
Read [SAFETY.md](../SAFETY.md) before firmware work.

Build from this repository with the pinned Rust toolchain and MuJoCo 3.9.0.
The package does not download firmware, profiles, or native libraries.
