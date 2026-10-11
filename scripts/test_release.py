#!/usr/bin/env python3
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from prepare_release import collect
from release_metadata import metadata


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text('[package]\nversion = "1.3.0"\n')
        (self.root / "flutter").mkdir()
        (self.root / "flutter/pubspec.yaml").write_text("version: 1.3.0+85\n")
        self.source = self.root / "artifacts"
        self.source.mkdir()
        self.output = self.root / "packages"

    def windows_packages(self):
        for suffix in (".msi", "-portable.exe", "-portable.zip"):
            (self.source / ("subnetdesk-1.3.0.85-x86_64" + suffix)).write_bytes(b"package")

    def test_default_version(self):
        info = metadata(self.root)
        self.assertEqual(info["version"], "1.3.0.85")
        self.assertEqual(info["tag"], "v1.3.0.85")

    def test_architecture_filter(self):
        info = metadata(self.root, "macos", "aarch64")
        jobs = json.loads(info["macos-matrix"])["job"]
        self.assertEqual(len(jobs), 1)
        self.assertEqual(jobs[0]["target"], "aarch64-apple-darwin")

    def test_invalid_tag_and_inputs(self):
        for tag in ("master", "v1.3.0.84", "vv1.3.0.85", "v1.3.0.85\nsecret=x"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                metadata(self.root, tag=tag)
        with self.assertRaises(ValueError):
            metadata(self.root, platform="unknown")
        with self.assertRaises(ValueError):
            metadata(self.root, architecture="x86")

    def test_version_mismatch(self):
        (self.root / "flutter/pubspec.yaml").write_text("version: 1.4.0+85\n")
        with self.assertRaises(ValueError):
            metadata(self.root)

    def test_msi_limits(self):
        (self.root / "flutter/pubspec.yaml").write_text("version: 1.3.0+65536\n")
        with self.assertRaises(ValueError):
            metadata(self.root)

    def test_missing_packages(self):
        self.windows_packages()
        (self.source / "subnetdesk-1.3.0.85-x86_64-portable.zip").unlink()
        with self.assertRaises(ValueError):
            collect(self.source, self.output, metadata(self.root, "windows", "x86_64"))

    def test_empty_duplicate_and_unexpected_artifacts(self):
        self.windows_packages()
        extra = self.source / "unexpected.dll"
        extra.write_bytes(b"lib")
        with self.assertRaises(ValueError):
            collect(self.source, self.output, metadata(self.root, "windows", "x86_64"))
        extra.unlink()
        duplicate = self.source / "other-artifact"
        duplicate.mkdir()
        (duplicate / "subnetdesk-1.3.0.85-x86_64.msi").write_bytes(b"duplicate")
        with self.assertRaises(ValueError):
            collect(self.source, self.output, metadata(self.root, "windows", "x86_64"))
        (duplicate / "subnetdesk-1.3.0.85-x86_64.msi").unlink()
        (self.source / "subnetdesk-1.3.0.85-x86_64.msi").write_bytes(b"")
        with self.assertRaises(ValueError):
            collect(self.source, self.output, metadata(self.root, "windows", "x86_64"))

    def test_checksums_and_manifest(self):
        self.windows_packages()
        assets = collect(self.source, self.output, metadata(self.root, "windows", "x86_64"))
        self.assertEqual(len(assets), 3)
        for line in (self.output / "SHA256SUMS").read_text().splitlines():
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        manifest = json.loads((self.output / "build-manifest.json").read_text())
        self.assertEqual(manifest["version"], "1.3.0.85")

    def test_linux_missing_rpm_fails(self):
        (self.source / "subnetdesk-1.3.0.85-aarch64.deb").write_bytes(b"deb")
        with self.assertRaises(ValueError):
            collect(self.source, self.output, metadata(self.root, "linux", "aarch64"))

    def test_build_py_version_override_is_validated(self):
        import sys
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        import build
        with patch.dict(os.environ, {"SUBNETDESK_PACKAGE_VERSION": "1.3.0.85"}):
            self.assertEqual(build.get_version(), "1.3.0.85")
        with patch.dict(os.environ, {"SUBNETDESK_PACKAGE_VERSION": "1.3.0;command"}):
            with self.assertRaises(ValueError):
                build.get_version()


if __name__ == "__main__":
    unittest.main()
