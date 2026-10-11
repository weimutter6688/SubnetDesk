#!/usr/bin/env python3
"""Exercise build-log forwarding with restrictive Windows-style encodings."""
import os
from pathlib import Path
import subprocess
import sys
import unittest


class BuildDiagnosticsTests(unittest.TestCase):
    def invoke(self, *command):
        wrapper = Path(__file__).with_name("build_with_diagnostics.py")
        return subprocess.run([sys.executable, str(wrapper), *command],
                              env=dict(os.environ, PYTHONIOENCODING="cp1252"),
                              capture_output=True, encoding="utf-8", timeout=20)

    def test_unicode_success_does_not_break_build(self):
        result = self.invoke(sys.executable, "-c", "print('\\u221a Built SubnetDesk \\u4e2d\\u6587')")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("\u221a Built SubnetDesk \u4e2d\u6587", result.stdout)
        self.assertNotIn("::error", result.stdout)

    def test_failure_keeps_exit_code_and_final_error_visible(self):
        result = self.invoke(sys.executable, "-c",
                             "import sys; print('earlier log\\n' * 1500); "
                             "print('final error: 100%\\nmissing package'); sys.exit(7)")
        self.assertEqual(result.returncode, 7)
        annotation = result.stdout.split("::error title=Build failure details::", 1)[1].strip()
        self.assertLess(len(annotation), 4096)
        self.assertIn("final error: 100%25%0Amissing package", annotation)

    def test_missing_command_is_reported(self):
        result = self.invoke("subnetdesk-nonexistent-build-command")
        self.assertEqual(result.returncode, 127)
        self.assertIn("::error title=Build command could not start::", result.stdout)


if __name__ == "__main__":
    unittest.main()
