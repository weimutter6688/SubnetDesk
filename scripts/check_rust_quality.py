"""Format gate for fork-owned modules, without reformatting inherited RustDesk."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MODULES = (
    "src/ipc/input_client.rs",
    "src/server/connection/queue.rs",
    "src/server/connection/ipc_bridge.rs",
    "src/server/uinput_client.rs",
    "src/lan_protocol.rs",
    "src/lan_mdns.rs",
    "src/lan_identity.rs",
    "src/lan_server.rs",
    "src/client/lan_connect.rs",
    "src/diagnostics.rs",
    "libs/hbb_common/src/config/recent_lan.rs",
    "libs/hbb_common/src/tcp/connect.rs",
)

if __name__ == "__main__":
    for name in MODULES:
        if "#![deny(warnings, clippy::all)]" not in (ROOT / name).read_text(encoding="utf-8"):
            raise SystemExit(f"Missing strict compiler/Clippy gate: {name}")
    subprocess.run(
        ["rustfmt", "--check", "--edition", "2021", "--config", "skip_children=true", *MODULES],
        cwd=ROOT, check=True,
    )
