# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

COMsniff is a TUI tool (Rust, `ratatui` + `crossterm`) intended to open two serial (COM) ports, forward data between them, and log the traffic passing through — a serial port sniffer/MITM. The project is at an early scaffold stage: `src/main.rs` currently only runs a placeholder ratatui event loop ("hello world") and does not yet touch `serialport`.

## Commands

- Build: `cargo build`
- Run: `cargo run`
- Check (fast compile check without producing binaries): `cargo check`
- Format: `cargo fmt`
- Lint: `cargo clippy`
- Test: `cargo test` (no tests exist yet)

## Architecture

- Single binary crate, entry point `src/main.rs`.
- Uses `color-eyre` for error handling/reporting (`color_eyre::Result`, installed via `color_eyre::install()`).
- Uses `ratatui`'s `run()` helper to manage terminal setup/teardown around an `app(&mut DefaultTerminal)` loop.
- The `app` loop redraws each frame with `render` and exits on any key press (`crossterm::event::read()?.is_key_press()`).
- `serialport` is a declared dependency for the eventual COM port I/O but is not yet wired into `main.rs`.
