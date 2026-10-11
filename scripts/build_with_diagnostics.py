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
    command = sys.argv[1:]
    if command[0] in ("python", "python3"):
        command[0] = sys.executable
    try:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   universal_newlines=True, encoding="utf-8", errors="replace")
    except OSError as error:
        print("::error title=Build command could not start::" + str(error))
        return 127
    for line in process.stdout:
        print(line, end="", flush=True)
        recent.append(line.rstrip())
    status = process.wait()
    if status:
        # Check annotations can truncate large messages at the beginning. Keep
        # the final compiler/packaging error within the visible message limit.
        detail = "\n".join(recent)[-3500:]
        detail = detail.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        print("::error title=Build failure details::" + detail)
    return status if status >= 0 else 1


if __name__ == "__main__":
    sys.exit(main())
