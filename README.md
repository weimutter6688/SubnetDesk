<p align="center">
  <img src="res/subnetdesk-icon.svg" alt="SubnetDesk logo" width="120">
</p>

# SubnetDesk

<p align="center">
  <strong>A LAN-first remote desktop for fast, private, direct connections.</strong>
</p>

<p align="center">
  An independently maintained <a href="https://github.com/rustdesk/rustdesk">RustDesk</a> fork built for LANs, routed private networks, and VPNs.
</p>

<p align="center">
  <a href="https://github.com/zibo-chen/SubnetDesk/releases/latest"><img src="https://img.shields.io/github/v/release/zibo-chen/SubnetDesk?style=flat-square&logo=github&color=2563eb" alt="Latest release"></a>
  <a href="https://github.com/zibo-chen/SubnetDesk/releases"><img src="https://img.shields.io/github/downloads/zibo-chen/SubnetDesk/total?style=flat-square&logo=github&color=0ea5e9" alt="Total downloads"></a>
  <a href="https://github.com/zibo-chen/SubnetDesk/stargazers"><img src="https://img.shields.io/github/stars/zibo-chen/SubnetDesk?style=flat-square&logo=github&color=f59e0b" alt="GitHub stars"></a>
  <a href="https://github.com/zibo-chen/SubnetDesk/network/members"><img src="https://img.shields.io/github/forks/zibo-chen/SubnetDesk?style=flat-square&logo=github" alt="GitHub forks"></a>
  <a href="https://github.com/zibo-chen/SubnetDesk/actions/workflows/subnetdesk-preflight.yml"><img src="https://img.shields.io/github/actions/workflow/status/zibo-chen/SubnetDesk/subnetdesk-preflight.yml?branch=master&style=flat-square&label=build" alt="Build status"></a>
  <a href="LICENCE"><img src="https://img.shields.io/github/license/zibo-chen/SubnetDesk?style=flat-square&color=16a34a" alt="License"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-x64%20%7C%20ARM64-0078D6?style=flat-square&logo=windows11&logoColor=white" alt="Windows">
  <img src="https://img.shields.io/badge/macOS-Intel%20%7C%20Apple%20Silicon-111111?style=flat-square&logo=apple&logoColor=white" alt="macOS">
  <img src="https://img.shields.io/badge/Linux-x86__64%20%7C%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=111111" alt="Linux">
  <img src="https://img.shields.io/badge/Android-APK-3DDC84?style=flat-square&logo=android&logoColor=white" alt="Android">
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="#why-subnetdesk">Why SubnetDesk</a> ·
  <a href="#features">Features</a> ·
  <a href="#download">Download</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#build-from-source">Build</a>
</p>

<p align="center">
  <img src="assets/screenshots/device-discovery.png" alt="SubnetDesk device discovery" width="880">
</p>

<a id="why-subnetdesk"></a>

## 💡 Why SubnetDesk

SubnetDesk focuses on one clear use case: remote control between devices that can already reach each other. It removes the public device-ID, rendezvous, relay, cloud account, proxy, and automatic public-update paths from RustDesk, replacing them with direct endpoint connections and local device discovery.

That makes it a good fit for home labs, offices, classrooms, on-site support, and private networks connected by WireGuard, Tailscale, OpenVPN, or another VPN.

| | SubnetDesk | Typical RustDesk deployment |
| --- | --- | --- |
| Connection | Direct IP address or hostname | Device ID through rendezvous, with optional relay |
| Discovery | Automatic mDNS discovery on the LAN | Device ID or address book |
| Server dependency | No coordination server required | Public or self-hosted rendezvous/relay server |
| Best suited for | LANs, routed private networks, and VPNs | Internet and cross-NAT access |

> [!IMPORTANT]
> SubnetDesk does not provide Internet rendezvous or relay services. The two devices must be reachable through the same LAN, a routed private network, or a VPN.

<a id="features"></a>

## ✨ Features

| | Capability | Details |
| --- | --- | --- |
| 🔎 | Local discovery | Find nearby devices automatically over mDNS. |
| ⚡ | Quick connections | Enter an IP address or hostname in the desktop address bar and press Enter; the TCP port is configurable. |
| 🔐 | Access protection | Authenticate with a username and password; passwords are stored as Argon2id hashes. |
| 🔑 | Reusable accounts | Save account profiles, choose a default, or bind a profile to a device. |
| 🛡️ | Device verification | Verify and remember endpoint fingerprints before trusting a device. |
| 🌐 | Network allowlist | Restrict incoming connections to selected CIDR networks. |
| ⭐ | Recent devices and favorites | Keep successful connections across restarts, even without remembering passwords; display device names and addresses with search and sorting. |
| 🎚️ | Connection modes | Choose low latency, text clarity, or image quality, with adaptive bitrate/FPS and resolution scaling where supported. |
| 🔄 | Automatic reconnection | Retry established sessions after temporary network failures, with device verification preserved. |
| 🩺 | Diagnostics | Export recent logs and version information from settings to help investigate problems. |
| 🖥️ | RustDesk experience | Retain remote control, clipboard, audio, and file-transfer capabilities without a public coordination server. |

<a id="download"></a>

## 📦 Download

Download stable packages from **[GitHub Releases](https://github.com/zibo-chen/SubnetDesk/releases/latest)**. For the newest changes, use the **[nightly release](https://github.com/zibo-chen/SubnetDesk/releases/tag/nightly)** or artifacts from **[GitHub Actions](https://github.com/zibo-chen/SubnetDesk/actions)**.

| Platform | Architectures | Packages |
| --- | --- | --- |
| Windows | x86_64, ARM64 | `.msi` installer (recommended), portable/no-install `.exe` |
| macOS | Intel, Apple Silicon | `.dmg` |
| Linux | x86_64, ARM64 | `.deb`, `.rpm`, `.AppImage`, `.flatpak`; Arch package on x86_64 |
| Android | ARM64, ARMv7, x86_64, universal | `.apk` |

> [!NOTE]
> Nightly packages are development builds. Prefer the latest stable release for daily use.

Windows distributions may also include a portable `.zip`. Extract the entire folder before running `SubnetDesk.exe`; keep its bundled libraries and data files together.

<a id="quick-start"></a>

## 🚀 Quick start

1. Install and open SubnetDesk on both devices.
2. A controlled device with no previously configured access account initializes username `subnetrust` and password `subnet@local`, listening on port `21118` by default. Change credentials and LAN discovery options in **LAN settings**.
3. On the controller, select a discovered device, or enter a hostname/IP address (optionally with `:port`) in the desktop address bar and press **Enter**.
4. Verify the device fingerprint on first connection. When prompted, select an account profile or enter the controlled device's username and password, then press **Enter** to connect. Enter also submits from the username/password fields after switching between them with **Tab**.

The default credentials are shared by all copies; replace the password in LAN settings. Existing credentials are preserved, and restarting does not restore defaults after credentials are changed or explicitly cleared. These credentials are independent of the operating system login.

<p align="center">
  <img src="assets/screenshots/lan-settings.png" alt="SubnetDesk LAN settings" width="880">
</p>

### Accounts and recent devices

Use **Account** on the desktop to manage reusable account profiles. You can select a profile for a connection and remember its association with that device. Credential selection uses an explicitly selected profile first, then a device-bound profile, then that device's remembered password, and finally the default profile.

Successful connections appear in recent devices independently of **Remember password** and remain after restarting. Records follow the verified device fingerprint when its address changes; deleted records remain deleted until you successfully connect again. Favorites, search, sorting, and grid/list views help manage frequently used devices.

### Network checklist

- Both devices can route to each other.
- TCP port `21118` (or your configured port) is allowed by the host firewall.
- mDNS is available when automatic discovery is needed; direct address entry still works without it.
- The controlled device's CIDR allowlist includes the controller's network.
- On Linux, see [host readiness notes](docs/linux-host-readiness.md) for SELinux, Wayland, and login-screen guidance.

## 🔒 Security model

- Traffic stays on the network path between your devices; SubnetDesk does not require a public coordination service.
- Passwords are persisted as Argon2id hashes rather than plaintext.
- Device fingerprints provide trust-on-first-use verification and help detect an unexpected endpoint identity change.
- CIDR allowlists let the controlled device limit which source networks may connect.

The current transport uses LAN protocol **v2**, which separates encryption nonces
by direction and keeps port-forwarding/RDP tunnel traffic encrypted after login.
Update **both endpoints** together; protocol v1 peers are rejected without a
downgrade. Reload the browser client after updating the host. Existing accounts,
device fingerprints, and network rules retain their configuration formats.
Peer avatar URLs no longer trigger automatic HTTP requests. See the
[network and security review](docs/security-network-review.md) for findings,
runtime destinations, validation, and review limits.

As with any remote-control software, expose the listening port only to trusted networks, use a strong unique password, and verify fingerprints through a separate trusted channel when possible.


### Network rules and connection quality

LAN settings show the default allowed source networks: `10.0.0.0/8`,
`172.16.0.0/12`, `192.168.0.0/16`, Tailscale's `100.64.0.0/10`, IPv6
`fc00::/7` (including Tailscale IPv6), plus loopback and link-local ranges.
Edit, add, or delete one CIDR per line; commas are also accepted. Existing custom
rules are preserved. Saving an empty list denies new native connections and stays
empty after restart. Web access inherits this list unless its own allowlist is set.
These are source-address rules, not automatic detection of a VPN tunnel.

Known-device connections try an alternate address after **80 ms** if the first
attempt is still pending, with at most two concurrent candidate endpoints and
fingerprint checks. Each endpoint shares one timeout budget across DNS and TCP,
interleaves IPv4/IPv6 addresses, and races at most two resolved addresses with an
80 ms fallback delay. Together these limits allow at most four TCP attempts.

### Connection modes and recovery

The remote toolbar's **Connection mode** is saved per device:

- **Low latency (default)** reduces bitrate first, then transmission resolution during sustained congestion, and gradually restores quality as the link recovers.
- **Text clarity** keeps native resolution with a 30 FPS ceiling.
- **Image quality** keeps native resolution with a 45 FPS ceiling and a higher bitrate floor.

Video starts at 30 FPS with a conservative bitrate. Selected quality and custom FPS
remain upper limits; stable VPN RTT is distinguished from queueing delay. Adaptive
scaling preserves desktop geometry and mouse coordinates. Update both endpoints to
enable stream-size changes and decoder recovery fully; scaling also requires a
supported desktop renderer and is disabled during recording.

Viewers have independent encoders and bitrate/FPS feedback. Slow viewers receive
the latest captured image when sending resumes, including on a static desktop.
Pixel encoding runs in separate bounded workers, and keyboard/mouse input continues
to be received while video or large clipboard messages are sent. Shared CPU/GPU
saturation and severe link congestion still limit performance. See the
[connection tuning and verification guide](docs/weak-network-tuning.md) and
[video performance changes](docs/performance-fixes.md) for details.

Established sessions interrupted by a temporary network failure retry at 1, 2, 4,
then 8-second intervals for about two minutes. Closing the window or cancelling
stops retries; authentication, permission, or fingerprint errors and intentional
disconnects do not trigger automatic retries.

On Windows, the installed system-privileged server under a running background
service is restarted after 30 seconds without video-worker progress. Normal idle
time does not trigger recovery, and resume from sleep gets a fresh grace period.
Recovery interrupts existing sessions; portable and UI processes do not perform
this automatic restart. See [Windows idle/reconnect recovery](docs/idle-reconnect-recovery.md).

There is no GitHub update checker, downloader, installer, or online help link in
the application. Distribute updates manually over LAN/VPN or offline media.
Build dependencies and repository documentation may still reference GitHub.
See [manual update guidance](docs/automatic-updates.md) for preserving existing configuration.

<a id="build-from-source"></a>

## 🛠️ Build from source

Clone the repository with its submodules, then run the Flutter desktop build:

```bash
git clone --recurse-submodules https://github.com/zibo-chen/SubnetDesk.git
cd SubnetDesk
./build.py --flutter --hwcodec
```

The build requires Rust, Flutter, and platform-specific native dependencies. The [native package workflow](.github/workflows/flutter-build.yml) is the source of truth for pinned tool versions, target architectures, and packaging steps.

## 🤝 Contributing

Bug reports, feature ideas, and pull requests are welcome. Before opening a change, check the existing [issues](https://github.com/zibo-chen/SubnetDesk/issues) and keep changes focused on the LAN/VPN-first product scope.

## 💬 Community

<p align="center">
  <a href="https://discord.gg/MFZG478wPQ">
    <img src="https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Join the SubnetDesk Discord community">
  </a>
</p>

<p align="center">
  Join the <a href="https://discord.gg/MFZG478wPQ">SubnetDesk Discord community</a>, or scan the QR code below to add me on WeChat. Mention <strong>SubnetDesk</strong>, and I will invite you to the WeChat group.
</p>

<p align="center">
  <img src="assets/community/wechat-contact.jpg" alt="Add czb on WeChat to join the SubnetDesk community group" width="360">
</p>

## 🙏 Credits and license

SubnetDesk is based on [RustDesk](https://github.com/rustdesk/rustdesk) and retains its open-source foundations. SubnetDesk is an independent project and is not an official RustDesk release.

Licensed under the [GNU Affero General Public License v3.0](LICENCE). Use remote-control software only on systems you own or are authorized to administer.
