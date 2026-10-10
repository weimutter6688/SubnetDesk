"""Read-only upstream change/advisory report; never downloads app updates."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
SECURITY_TERMS = re.compile(
    r"security|CVE-|vulnerab|auth|password|unsafe|permission|overflow|inject|bypass|encrypt|nonce|token|leak",
    re.IGNORECASE,
)


def fetch(url):
    headers = {
        "User-Agent": "SubnetDesk-upstream-security-monitor",
        "Accept": "application/vnd.github+json",
    }
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(url, headers=headers)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=40) as response:
                return json.load(response)
        except urllib.error.HTTPError:
            raise
        except (OSError, ValueError):
            if attempt == 2:
                raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    baseline = json.loads((ROOT / "docs/upstream-baseline.json").read_text(encoding="utf-8"))
    repository = baseline["repository"]
    if not re.fullmatch(r"[\w-]+/[\w-]+", repository):
        raise ValueError("Invalid upstream repository")
    base_url = f"https://api.github.com/repos/{repository}"
    head = fetch(base_url + "/commits/master")["sha"]
    commits = []
    for page in range(1, 21):
        query = urllib.parse.urlencode({
            "sha": head, "since": baseline["monitor_since_utc"], "per_page": 100, "page": page,
        })
        batch = fetch(base_url + "/commits?" + query)
        commits.extend(batch)
        if len(batch) < 100:
            break
    else:
        raise ValueError("Upstream history exceeds 2000 commits; narrow the reviewed monitoring interval")

    advisory_status = ""
    try:
        advisories = fetch(base_url + "/security-advisories?per_page=100")
        advisory_status = f"Public repository advisories returned: {len(advisories)}."
        for advisory in advisories:
            advisory_status += f"\n- {advisory.get('ghsa_id', '')}: {advisory.get('html_url', '')}"
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        advisory_status = "Public repository advisory API is unavailable (HTTP 404); this does not establish absence of vulnerabilities."

    candidates = [c for c in commits if SECURITY_TERMS.search(c["commit"]["message"])]
    lines = [
        "# RustDesk upstream security review queue", "",
        f"Generated: {datetime.now(timezone.utc).isoformat()}",
        f"Fork point: `{baseline['fork_point_commit']}`; observed head: `{head}`.",
        f"Commits returned since {baseline['monitor_since_utc']}: {len(commits)}; keyword candidates: {len(candidates)}.",
        "", advisory_status, "",
        "Commit-message matching is a triage aid. Date-filtered history can miss merged commits with older author dates; review upstream advisories and the full fork-point comparison too.",
        "No change is applied automatically. The baseline is a fork point, not a claim that all later security fixes have been integrated.", "",
        "| Candidate | Subject |", "| --- | --- |",
    ]
    for commit in candidates:
        title = commit["commit"]["message"].splitlines()[0].replace("|", "\\|")
        lines.append(f"| [{commit['sha'][:12]}]({commit['html_url']}) | {title} |")
    lines += ["", "## Complete returned commit list", ""]
    for commit in commits:
        title = commit["commit"]["message"].splitlines()[0]
        lines.append(f"- [{commit['sha'][:12]}]({commit['html_url']}): {title}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Upstream head {head}; {len(commits)} commits; {len(candidates)} review candidates; report: {args.output}")


if __name__ == "__main__":
    main()
