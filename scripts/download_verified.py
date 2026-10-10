"""Download only reviewed HTTPS build inputs; publish the file after verification."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import urllib.request

MANIFEST = Path(__file__).with_name("build-downloads.json")


class HttpsRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlparse(newurl).scheme != "https":
            raise ValueError("Build downloads must remain HTTPS across redirects")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def load_entry(key):
    entry = json.loads(MANIFEST.read_text(encoding="utf-8"))[key]
    if urllib.parse.urlparse(entry["url"]).scheme != "https":
        raise ValueError("Build downloads require HTTPS")
    if not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
        raise ValueError("Invalid pinned SHA-256")
    return entry


def verify(path, expected):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != expected:
        raise ValueError(f"SHA-256 mismatch for {Path(path).name}")


def download(entry, output):
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    staged = None
    try:
        opener = urllib.request.build_opener(HttpsRedirectHandler())
        request = urllib.request.Request(entry["url"], headers={"User-Agent": "SubnetDesk-build"})
        with opener.open(request, timeout=60) as response:
            with tempfile.NamedTemporaryFile(dir=output.parent, delete=False) as stream:
                staged = Path(stream.name)
                for chunk in iter(lambda: response.read(1024 * 1024), b""):
                    stream.write(chunk)
        verify(staged, entry["sha256"])
        os.replace(staged, output)
        staged = None
    finally:
        if staged is not None:
            staged.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("key", help="Reviewed input name from build-downloads.json")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    entry = load_entry(args.key)
    if args.verify_only:
        verify(args.output, entry["sha256"])
    else:
        download(entry, args.output)
    print(f"Verified {args.key}: {entry['sha256']}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(str(error)) from error
