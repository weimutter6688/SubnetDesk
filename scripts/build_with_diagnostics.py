#!/usr/bin/env python3
"""Run a build and attach its failure details to the Actions check annotation."""

import collections
import subprocess
import sys


def main():
    if len(sys.argv) < 2:
        print("usage: build_with_diagnostics.py <build command> [arguments]", file=sys.stderr)
        return 2
    recent = collections.deque(maxlen=120)
    process = subprocess.Popen(sys.argv[1:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               universal_newlines=True, encoding="utf-8", errors="replace")
    for line in process.stdout:
        print(line, end="", flush=True)
        recent.append(line.rstrip())
    status = process.wait()
    if status:
        detail = "\n".join(recent)[-24000:]
        detail = detail.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        print("::error title=Build failure details::" + detail)
    return status if status >= 0 else 1


if __name__ == "__main__":
    sys.exit(main())
