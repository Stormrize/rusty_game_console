# Rusty Game Console

A DIY handheld gaming console powered by **ESP32** and written entirely in **Rust**.

snake
<img width="800" height="602" alt="snake" src="https://github.com/user-attachments/assets/4e8eb406-1036-4b60-ba0d-ee84c8cd8637" />

chess
<img width="800" height="602" alt="chess" src="https://github.com/user-attachments/assets/c95ba22a-eb0a-4d20-840b-b1bb9e9190a2" />

## Hardware

- **MCU:** ESP32
- **Display:** ILI9341 TFT
- **Controls:** Joystick and physical buttons
- **Connectivity:** Wi-Fi

## Software & Features

- Written in Rust using `no_std`-oriented embedded technologies.
- Built on top of `esp-hal` and the Embassy async ecosystem.
- Custom game logic and rendering optimized for limited hardware resources.
- Wi-Fi networking for multiplayer functionality.
- Efficient memory usage with iterator-based processing.
- Optimized rendering with no unnecessary full-screen redraws, resulting in smooth visuals without noticeable flickering.

## Currently Implemented

- **Snake** — a classic Snake game.
- **Online Chess** — multiplayer chess over a local Wi-Fi network, using a separate custom-built server.

The chess networking system uses a dedicated server developed as a separate project. Rendering is carefully optimized to avoid unnecessary redraws, keeping RAM usage low and gameplay visually smooth.

## Work in Progress

Development is still ongoing. Current priorities include:

- Further optimizing memory usage and rendering performance.
- Developing additional games.
- Implementing an SD card-based storage system with a filesystem.
- Creating a system for loading and running games dynamically instead of relying on games being statically compiled into the firmware.
- Soldering all together

## Project Goal

The ultimate goal is to turn this into a fully-fledged DIY gaming console — with expandable storage, multiple games, physical controls, and wireless multiplayer capabilities.

_This project is actively being developed. More features, games, and optimizations are coming._
