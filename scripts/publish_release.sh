#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <tag> <package-directory> <notes-file>" >&2
  exit 2
fi
tag="$1"
packages="$2"
notes="$3"
: "${GITHUB_REPOSITORY:?Missing repository}"
: "${GITHUB_SHA:?Missing source commit}"
export GH_REPO="$GITHUB_REPOSITORY"

# A manually supplied existing tag must point to the code being packaged.
remote_tag=$(git ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}")
if [[ -n "$remote_tag" && "$tag" != 'nightly' ]]; then
  git fetch --depth=1 --force origin "tag" "$tag"
  tagged_sha=$(git rev-list -n 1 "$tag")
  if [[ "$tagged_sha" != "$GITHUB_SHA" ]]; then
    echo "Release tag points to a different source commit" >&2
    exit 1
  fi
fi

if release=$(gh release view "$tag" --json isDraft); then
  if [[ "$tag" != 'nightly' && $(printf '%s' "$release" | jq -r .isDraft) != 'true' ]]; then
    echo "Release $tag is already published; bump the Flutter build number for a new release." >&2
    exit 1
  fi
else
  if [[ "$tag" == 'nightly' && -z "$remote_tag" ]]; then
    gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" -f ref=refs/tags/nightly -f sha="$GITHUB_SHA"
  fi
  gh release create "$tag" --draft --target "$GITHUB_SHA" --title "SubnetDesk $tag" --notes-file "$notes"
fi

for asset in "$packages"/*; do
  bash scripts/upload_release_asset_with_retry.sh "$tag" "$asset"
done

# Prune stale files only after every replacement was built, checked, and uploaded.
release_api=$(gh release view "$tag" --json apiUrl --jq .apiUrl)
release_id="${release_api##*/}"
if ! [[ "$release_id" =~ ^[0-9]+$ ]]; then
  echo "Invalid release identifier: $release_id" >&2
  exit 1
fi
release_assets=$(gh api --paginate "repos/$GITHUB_REPOSITORY/releases/$release_id/assets?per_page=100" --jq '.[] | [.id, .name] | @tsv')
while IFS=$'\t' read -r asset_id asset_name; do
  [[ -n "$asset_id" ]] || continue
  if [[ ! -f "$packages/$asset_name" ]]; then
    gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/assets/$asset_id"
  fi
done <<< "$release_assets"

if [[ "$tag" == 'nightly' ]]; then
  gh api --method PATCH "repos/$GITHUB_REPOSITORY/git/refs/tags/nightly" -f sha="$GITHUB_SHA" -F force=true
  gh release edit "$tag" --draft=false --prerelease --title 'SubnetDesk Nightly' --notes-file "$notes"
else
  gh release edit "$tag" --draft=false --prerelease=false --latest --title "SubnetDesk $tag" --notes-file "$notes"
fi
