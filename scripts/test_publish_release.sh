#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$repo_root/build"
task_dir="$(mktemp -d "$repo_root/build/publish-test.XXXXXX")"
case "$task_dir" in "$repo_root"/build/publish-test.*) ;; *) exit 2 ;; esac
trap 'rm -rf -- "$task_dir"' EXIT
mkdir "$task_dir/bin" "$task_dir/packages"
printf 'package' > "$task_dir/packages/subnetdesk-test.msi"
printf 'notes' > "$task_dir/notes.md"
cat > "$task_dir/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == 'ls-remote' ]]
EOF
cat > "$task_dir/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$PUBLISH_TEST_LOG"
if [[ "$1 $2" == 'release view' ]]; then
  if [[ "$*" == *'apiUrl'* ]]; then
    printf 'https://api.github.com/repos/owner/repo/releases/123\n'
  elif [[ "$PUBLISH_TEST_MODE" == 'published' ]]; then
    printf '{"isDraft":false}\n'
  else
    printf '{"isDraft":true}\n'
  fi
elif [[ "$1 $2" == 'release upload' && "$PUBLISH_TEST_MODE" == 'failed-upload' ]]; then
  exit 1
elif [[ "$1" == 'api' && "$2" == */releases/tags/* ]]; then
  printf '123\n'
fi
EOF
cat > "$task_dir/bin/jq" <<'EOF'
#!/usr/bin/env bash
input=$(cat)
if [[ "$input" == *false* ]]; then printf 'false\n'; else printf 'true\n'; fi
EOF
chmod +x "$task_dir/bin/gh" "$task_dir/bin/git" "$task_dir/bin/jq"
export PATH="$task_dir/bin:$PATH"
export GITHUB_REPOSITORY='owner/repo' GITHUB_SHA='0123456789abcdef0123456789abcdef01234567'
export PUBLISH_TEST_LOG="$task_dir/commands.log"
export RELEASE_UPLOAD_MAX_ATTEMPTS=1 RELEASE_UPLOAD_RETRY_DELAY_SECONDS=0
cd "$repo_root"
export PUBLISH_TEST_MODE='published'
if bash scripts/publish_release.sh v1.3.0.85 "$task_dir/packages" "$task_dir/notes.md"; then
  echo 'Published releases must not be overwritten' >&2
  exit 1
fi
! grep -q 'release upload\|release edit' "$PUBLISH_TEST_LOG"
export PUBLISH_TEST_MODE='failed-upload'
printf '' > "$PUBLISH_TEST_LOG"
if bash scripts/publish_release.sh v1.3.0.85 "$task_dir/packages" "$task_dir/notes.md"; then
  echo 'Failed upload must not publish a draft' >&2
  exit 1
fi
! grep -q 'release edit' "$PUBLISH_TEST_LOG"
export PUBLISH_TEST_MODE='success'
printf '' > "$PUBLISH_TEST_LOG"
bash scripts/publish_release.sh v1.3.0.85 "$task_dir/packages" "$task_dir/notes.md"
grep -q 'release upload' "$PUBLISH_TEST_LOG"
grep -q 'release edit.*--draft=false' "$PUBLISH_TEST_LOG"
echo 'Release publication guards passed.'
