#!/usr/bin/env python3
"""Collect build artifacts, enforce the package contract, and write checksums."""

import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path
from urllib.parse import quote

from release_metadata import metadata


def collect(source, output, info, extras=False):
    platforms = ["windows", "macos", "linux"] if info["platform"] == "all" else [info["platform"]]
    arches = ["x86_64", "aarch64"] if info["architecture"] == "all" else [info["architecture"]]
    version = info["version"]
    packages = {}
    for path in sorted(source.rglob("*")):
        if not path.is_file():
            continue
        if path.is_symlink() or not path.name.startswith("subnetdesk-" + version + "-"):
            raise ValueError("Unexpected release file: " + str(path))
        if path.name in packages:
            raise ValueError("Duplicate release filename: " + path.name)
        if path.stat().st_size == 0:
            raise ValueError("Empty release file: " + path.name)
        if not path.name.lower().endswith((".msi", ".exe", ".zip", ".dmg", ".deb", ".rpm",
                                            ".appimage", ".flatpak", ".pkg.tar.zst", ".apk")):
            raise ValueError("Unsupported package format: " + path.name)
        packages[path.name] = path
    for platform in platforms:
        for arch in arches:
            prefix = "subnetdesk-{}-{}".format(version, arch)
            required = []
            if platform == "windows":
                required = [prefix + ".msi", prefix + "-portable.exe", prefix + "-portable.zip"]
            elif platform == "macos":
                required = [prefix + ".dmg"]
            else:
                required = [prefix + ".deb"]
                for suse in (False, True):
                    rpms = [name for name in packages if name.endswith(".rpm") and
                            arch in name and name.endswith("-suse.rpm") == suse]
                    if len(rpms) != 1:
                        raise ValueError("Expected one {}RPM for {}".format("SUSE " if suse else "", arch))
                if extras:
                    required.extend([prefix + ".AppImage", prefix + ".flatpak"])
                    if arch == "x86_64" and not any(name.endswith(".pkg.tar.zst") for name in packages):
                        raise ValueError("Missing Arch Linux package")
            for name in required:
                if name not in packages:
                    raise ValueError("Missing required package: " + name)
    if output.exists() and any(output.iterdir()):
        raise ValueError("Release output directory must be empty")
    output.mkdir(parents=True, exist_ok=True)
    assets = []
    for name, path in sorted(packages.items()):
        destination = output / name
        shutil.copyfile(str(path), str(destination))
        digest = hashlib.sha256()
        with destination.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        assets.append({"name": name, "size": destination.stat().st_size, "sha256": digest.hexdigest()})
    manifest = {"version": version, "tag": info["tag"], "platform": info["platform"],
                "architecture": info["architecture"], "linux_extras": extras,
                "source_commit": os.environ.get("GITHUB_SHA", ""),
                "workflow_run": os.environ.get("GITHUB_RUN_ID", ""), "assets": assets}
    manifest_path = output / "build-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    sums = ["{}  {}".format(asset["sha256"], asset["name"]) for asset in assets]
    sums.append("{}  build-manifest.json".format(hashlib.sha256(manifest_path.read_bytes()).hexdigest()))
    (output / "SHA256SUMS").write_text("\n".join(sums) + "\n", encoding="utf-8")
    return assets


def notes(info, assets, repository):
    lines = ["SubnetDesk " + info["version"], "",
             "Windows packages are unsigned. macOS packages use ad-hoc signing unless explicitly configured otherwise.",
             "Verify downloads against SHA256SUMS. Build details are in build-manifest.json.", "",
             "| Package | Download |", "| --- | --- |"]
    for asset in assets:
        name = asset["name"]
        url = "https://github.com/{}/releases/download/{}/{}".format(repository, quote(info["tag"], safe=""), quote(name))
        lines.append("| {} | [Download]({}) |".format(name, url))
    lines.extend(["", "Source: `" + os.environ.get("GITHUB_SHA", "") + "`", ""])
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--notes", required=True, type=Path)
    parser.add_argument("--platform", default="all")
    parser.add_argument("--architecture", default="all")
    parser.add_argument("--tag", default="")
    parser.add_argument("--linux-extras", action="store_true")
    args = parser.parse_args()
    try:
        info = metadata(Path(__file__).resolve().parents[1], args.platform, args.architecture, args.tag)
        assets = collect(args.source, args.output, info, args.linux_extras)
    except ValueError as error:
        parser.error(str(error))
    args.notes.parent.mkdir(parents=True, exist_ok=True)
    args.notes.write_text(notes(info, assets, os.environ.get("GITHUB_REPOSITORY", "weimutter6688/SubnetDesk")), encoding="utf-8")
    print("Validated {} packages for {}".format(len(assets), info["tag"]))


if __name__ == "__main__":
    main()
