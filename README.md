# WLTablet

WLTablet turns your tablet into a graphics tablet for your computer, **focused on compatibility** for competitive gaming.

This project is inspired by [Jibhong/ALTablet](https://github.com/Jibhong/ALTablet)

# Features
| Status |  Feature |
| --- | --- |
| ⏳ | **Cross-platform Input Stream**|
| ✅ | Control your mouse with your mobile device's stylus |
| ⏳ | Customize tablet input area (Position, Scale and Aspect Ratio) |
| ⏬ | Custom Background Image |
| ⏬ | Stylus pressure support |

✅ Finished
⏳ Someday...
⏬ Not Planned
🚫 Won't Do

## Installation

### Host Setup

#### Prerequisites for Linux
- Linux (Wayland or X11)
- [Rust](https://rust-lang.org/tools/install/)
- libxrandr-dev (for X11)
- xdg-desktop-portal (for Wayland)

#### Prerequisites for Windows
- [Rust](https://rust-lang.org/tools/install/)

#### Build
```sh
cargo build --release
```

#### Run
```sh
./target/release/wltablet
```

The server will start on `0.0.0.0:8080` and display a 6-digit pairing PIN in the terminal.

If you want it to be more secure,
you can connect your device with specific interface (ie. USB, Bluetooth)
```bash
./target/release/wltablet \
    --pin 123456          \
    --listen x.x.x.x:8080
```

### Mobile Setup

#### 1. Get Host Address

**Linux:**
```sh
ip address
```

**Windows:**
```cmd
ipconfig
```

**macOS:**
```sh
ifconfig
```

#### 2. Connect from Mobile Device

Open a web browser on your mobile device and navigate to:
```
http://<host-ip>:8080
```

#### 3. Pairing

1. Enter the 6-digit PIN displayed on the host
2. Accept the portal permission prompt on the host
3. Start drawing on your tablet

note: subjected to change

## Usage

- **Move cursor:** Move your stylus on the canvas
- **Click:** Tap on the canvas
- **Disconnect:** Close the web page or refresh

## Supported Platforms

### Host
- ✅ Linux (Wayland) - RemoteDesktop Portal
- ✅ Linux (X11) - XTest extension
- ✅ Windows - Windows API
- 🚫 macOS

### Mobile
- ✅ iOS Safari
- ✅ Android Chrome/Firefox
- ✅ iPadOS Safari

## Architecture

- **Backend:** Rust with Axum web framework
- **Protocol:** WebRTC DataChannel for low-latency input
- **Input Method:** D-Bus portal (Wayland) or XTest (X11)
- **Frontend:** Vanilla JavaScript with WebRTC API

## Security

WLTablet uses PIN-based pairing to ensure only authorized devices can connect. Each session requires explicit acceptance on the host.

## Contributing

Contributions are welcome! Areas of interest:
- Windows host support
- macOS support
- Input customization UI
- Performance optimization

note: most code are written by AI

## License

MIT

