import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from download_verified import HttpsRedirectHandler, download, load_entry, verify


class VerifiedDownloadsTest(unittest.TestCase):
    def test_tampering_does_not_publish_or_replace_an_existing_file(self):
        entry = {"url": "https://example.invalid/input", "sha256": hashlib.sha256(b"reviewed").hexdigest()}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "tool.zip"
            output.write_bytes(b"existing")
            with patch("download_verified.urllib.request.build_opener") as opener:
                opener.return_value.open.return_value = io.BytesIO(b"tampered")
                with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                    download(entry, output)
            self.assertEqual(output.read_bytes(), b"existing")
            self.assertEqual(list(Path(directory).iterdir()), [output])

    def test_verified_input_is_published_and_rechecked(self):
        entry = {"url": "https://example.invalid/input", "sha256": hashlib.sha256(b"reviewed").hexdigest()}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "tool.zip"
            with patch("download_verified.urllib.request.build_opener") as opener:
                opener.return_value.open.return_value = io.BytesIO(b"reviewed")
                download(entry, output)
            verify(output, entry["sha256"])

    def test_https_redirect_cannot_downgrade_to_http(self):
        with self.assertRaisesRegex(ValueError, "remain HTTPS"):
            HttpsRedirectHandler().redirect_request(None, None, 302, "", {}, "http://example.invalid/tool")

    def test_all_manifest_entries_have_pinned_hashes_and_https_urls(self):
        import json
        from download_verified import MANIFEST
        for key in json.loads(MANIFEST.read_text(encoding="utf-8")):
            load_entry(key)


if __name__ == "__main__":
    unittest.main()
