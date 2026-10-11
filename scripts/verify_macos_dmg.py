#!/usr/bin/env python3
"""Check the packaged macOS app without launching it."""
import argparse
from pathlib import Path
import plistlib
import subprocess


def verify(image, architecture):
    subprocess.run(["hdiutil", "verify", str(image)], check=True)
    attached = subprocess.check_output(["hdiutil", "attach", "-readonly", "-nobrowse", "-plist", str(image)])
    entities = plistlib.loads(attached).get("system-entities", [])
    mount = next((Path(item["mount-point"]) for item in entities if "mount-point" in item), None)
    if mount is None:
        raise ValueError("Disk image did not mount a filesystem")
    try:
        app = mount / "SubnetDesk.app"
        with (app / "Contents/Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
        executable = app / "Contents/MacOS" / info["CFBundleExecutable"]
        helper = app / "Contents/MacOS/subnetdesk-service"
        expected = {"x86_64": "x86_64", "aarch64": "arm64"}[architecture]
        for binary in (executable, helper):
            if not binary.is_file():
                raise ValueError("Missing packaged executable: " + str(binary))
            arches = subprocess.check_output(["lipo", "-archs", str(binary)], text=True).split()
            if expected not in arches:
                raise ValueError("Wrong packaged architecture: " + str(binary))
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        print("Verified disk image app, service, architecture and signatures:", image)
    finally:
        subprocess.run(["hdiutil", "detach", str(mount)], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("architecture", choices=("x86_64", "aarch64"))
    args = parser.parse_args()
    verify(args.image, args.architecture)
