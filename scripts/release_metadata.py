#!/usr/bin/env python3
"""Validate release inputs and produce the desktop build matrices (stdlib only)."""

import argparse
import json
import re
from pathlib import Path


def metadata(root, platform="all", architecture="all", tag=""):
    if platform not in ("all", "windows", "macos", "linux"):
        raise ValueError("Unsupported desktop platform: " + platform)
    if architecture not in ("all", "x86_64", "aarch64"):
        raise ValueError("Unsupported architecture: " + architecture)
    cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
    flutter = (root / "flutter/pubspec.yaml").read_text(encoding="utf-8")
    base = re.search(r'^version\s*=\s*"(\d+\.\d+\.\d+)"', cargo, re.M)
    app = re.search(r'^version:\s*(\d+\.\d+\.\d+)\+(\d+)\s*$', flutter, re.M)
    if not base or not app or base[1] != app[1]:
        raise ValueError("Cargo.toml and Flutter must have the same numeric base version")
    parts = [int(part) for part in base[1].split(".")]
    build = int(app[2])
    if parts[0] > 255 or parts[1] > 255 or parts[2] > 65535 or not 1 <= build <= 65535:
        raise ValueError("Version exceeds Windows installer/resource limits")
    version = "{}.{}".format(base[1], build)
    tag = tag or "v" + version
    if tag != "nightly":
        # Accept upstream-style tags as well as the full package version.
        plain_tag = tag[1:] if tag.startswith("v") else tag
        if plain_tag not in (version, base[1], base[1] + "-" + str(build)):
            raise ValueError("Release tag must match source version " + version)
    arches = ["x86_64", "aarch64"] if architecture == "all" else [architecture]
    result = {"base-version": base[1], "build-number": str(build),
              "version": version, "tag": tag, "platform": platform,
              "architecture": architecture}
    windows, macos, linux, privacy, flatpak, appimage = [], [], [], [], [], []
    for arch in arches:
        arm = arch == "aarch64"
        win_os = "windows-11-arm" if arm else "windows-2022"
        windows.append({"target": arch + "-pc-windows-msvc", "os": win_os,
                        "arch": arch, "flutter-arch": "arm64" if arm else "x64",
                        "vcpkg-triplet": "arm64-windows-static" if arm else "x64-windows-static",
                        "build-args": "" if arm else "--vram"})
        privacy.append({"target": win_os, "platform": "ARM64" if arm else "x64"})
        macos.append({"target": arch + "-apple-darwin", "arch": arch,
                      "os": "macos-14" if arm else "macos-15-intel",
                      "vcpkg-triplet": "arm64-osx" if arm else "x64-osx",
                      "extra-build-args": "--screencapturekit" if arm else ""})
        linux.append({"target": arch + "-unknown-linux-gnu", "arch": arch,
                      "distro": "ubuntu22.04", "on": "ubuntu-22.04-arm" if arm else "ubuntu-22.04",
                      "deb_arch": "arm64" if arm else "amd64",
                      "vcpkg-triplet": "arm64-linux" if arm else "x64-linux"})
        flatpak.append({"target": arch + "-unknown-linux-gnu", "arch": arch,
                        "distro": "ubuntu22.04", "on": "ubuntu-22.04-arm" if arm else "ubuntu-22.04",
                        "suffix": ""})
        appimage.append({"target": arch + "-unknown-linux-gnu", "arch": arch})
    for name, jobs in (("windows", windows), ("macos", macos), ("linux", linux),
                       ("privacy", privacy), ("flatpak", flatpak), ("appimage", appimage)):
        result[name + "-matrix"] = json.dumps({"job": jobs}, separators=(",", ":"))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--platform", default="all")
    parser.add_argument("--architecture", default="all")
    parser.add_argument("--tag", default="")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = metadata(args.root, args.platform, args.architecture, args.tag)
    except ValueError as error:
        parser.error(str(error))
    if args.output:
        with args.output.open("a", encoding="utf-8") as output:
            for name, value in result.items():
                output.write("{}={}\n".format(name, value))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
