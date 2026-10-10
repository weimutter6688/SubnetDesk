"""Isolate the window identity in the pinned Windows privacy helper source."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source_root", type=Path)
args = parser.parse_args()
path = args.source_root / "WindowInjection/dllmain.cpp"
source = path.read_text(encoding="utf-8-sig")
for old, new in (
    ('_T("RustDeskPrivacyWindow")', '_T("SubnetDeskWeimutter6688PrivacyWindow")'),
    ('_T("RustDeskPrivacyWindowClass")', '_T("SubnetDeskWeimutter6688PrivacyWindowClass")'),
):
    if source.count(old) != 1:
        raise SystemExit(f"Pinned privacy helper identity changed: {old}")
    source = source.replace(old, new)
path.write_text(source, encoding="utf-8-sig")
