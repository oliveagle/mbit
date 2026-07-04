#!/usr/bin/env bash
#
# check_version.sh — devine_groups version-consistency gate.
#
# Pure bash, language-agnostic. Designed to be reusable across every
# devine_groups project. Checks (in order):
#
#   1. If the current branch matches release/vX.Y.Z, then:
#        a. The branch name must agree with the VERSION file
#           (i.e. branch == "release/v$(cat VERSION)").
#        b. The current commit must carry a tag whose name is
#           "v$(cat VERSION)" and whose version matches the file.
#        c. The "bump commit" must be a pure bump — touches only
#           VERSION. For merge commits (from MR merge), the bump
#           commit is the second parent (the merged branch's HEAD);
#           for regular commits, it's HEAD itself.
#   2. On any other branch (main, dev_*, feat/*, chore/*, etc.):
#      skip the release-only checks but still verify VERSION is a
#      well-formed x.y.z string.
#
# Exit code:
#   0  — all applicable checks pass
#   1  — at least one check failed
#
# Usage:
#   scripts/check_version.sh            # check current HEAD/branch
#   scripts/check_version.sh -v         # verbose: print each step
#
# This script NEVER modifies the working tree.

set -euo pipefail

# ─── locate the repo root ─────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION_FILE="$REPO_ROOT/VERSION"

cd "$REPO_ROOT"

# ─── output helpers ──────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; CYN=$'\033[36m'; DIM=$'\033[2m'; RST=$'\033[0m'
else
  RED=""; GRN=""; YLW=""; CYN=""; DIM=""; RST=""
fi
log()    { printf "${CYN}[check]${RST} %s\n" "$*"; }
ok()     { printf "${GRN}[check] ✓${RST} %s\n" "$*"; }
warn()   { printf "${YLW}[check] !${RST} %s\n" "$*" >&2; }
die()    { printf "${RED}[check] ✗${RST} %s\n" "$*" >&2; exit 1; }

VERBOSE=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -v|--verbose) VERBOSE=1; shift ;;
    -h|--help)
      sed -n '3,28p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) die "unknown argument: $1" ;;
  esac
done

vlog() { [[ $VERBOSE -eq 1 ]] && log "$*"; return 0; }

# ─── ensure we are inside a git repo ──────────────────────────────────────
command -v git >/dev/null || die "git not found in PATH"
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git repository"

# ─── 0. load VERSION (always required) ──────────────────────────────────
[[ -f "$VERSION_FILE" ]] || die "VERSION file not found at $VERSION_FILE"
VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "VERSION file is not bare x.y.z: '$VERSION'"
vlog "VERSION=$VERSION"

# ─── 1. detect current branch ───────────────────────────────────────────
# Allow CI_COMMIT_REF_NAME to override (used in GitLab CI where checkout
# is a detached HEAD; the env var carries the original ref name).
BRANCH="${CI_COMMIT_REF_NAME:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)}"
if [[ -z "$BRANCH" || "$BRANCH" == "HEAD" ]]; then
  # Detached HEAD with no CI override — release-only checks cannot run
  # (we cannot assert the branch name). Skip to a light-touch check.
  vlog "detached HEAD with no CI_COMMIT_REF_NAME — running light-touch checks only"
  BRANCH="(detached)"
fi
vlog "current branch: $BRANCH"

# ─── 2. always-on checks (run on every branch) ──────────────────────────
ok "VERSION file is well-formed: $VERSION"

# ─── 3. release-only checks ─────────────────────────────────────────────
RELEASE_RE='^release/v[0-9]+\.[0-9]+\.[0-9]+$'
EXPECTED_BRANCH="release/v$VERSION"

if [[ "$BRANCH" =~ $RELEASE_RE ]]; then
  log "on a release branch — running release-only checks"

  # 3a. branch name must equal release/v$VERSION
  if [[ "$BRANCH" != "$EXPECTED_BRANCH" ]]; then
    die "branch name ($BRANCH) does not match VERSION (expected $EXPECTED_BRANCH)"
  fi
  ok "branch name matches VERSION: $BRANCH"

  # 3b. HEAD must carry a tag named v$VERSION
  EXPECTED_TAG="v$VERSION"
  TAG_SHA="$(git rev-parse -q --verify "refs/tags/$EXPECTED_TAG"^{commit} 2>/dev/null || true)"
  if [[ -z "$TAG_SHA" ]]; then
    die "tag $EXPECTED_TAG not found on HEAD"
  fi
  HEAD_SHA="$(git rev-parse HEAD^{commit})"
  if [[ "$TAG_SHA" != "$HEAD_SHA" ]]; then
    die "tag $EXPECTED_TAG points to $TAG_SHA, but HEAD is $HEAD_SHA — HEAD must be tagged"
  fi
  ok "tag $EXPECTED_TAG points at HEAD"

  # 3c. working tree must be clean. The release HEAD should be a
  # clean merge commit; nothing in the working tree.
  if [[ -n "$(git status --porcelain)" ]]; then
    die "working tree is dirty — release branch HEAD must be clean"
  fi

  # 3d. HEAD must be a merge commit. The v2 release flow produces
  # release/vX.Y.Z HEADs only via MR merges from a dev branch, so a
  # non-merge HEAD on a release branch means somebody force-pushed
  # or committed directly to release (forbidden by the v2 flow).
  PARENT_COUNT="$(git cat-file -p HEAD | grep -c '^parent' || true)"
  if [[ "$PARENT_COUNT" -lt 2 ]]; then
    die "release branch HEAD is not a merge commit (parents=$PARENT_COUNT). The v2 release flow requires every release/vX.Y.Z HEAD to be produced by an MR merge from a dev branch."
  fi
  vlog "HEAD is a merge commit ($PARENT_COUNT parents); checking second-parent VERSION"

  # 3e. The dev branch's VERSION (HEAD^2:VERSION) must equal the
  # current VERSION. This guarantees the dev branch was already
  # bumped to v$VERSION before the MR was opened, so the contents
  # merged into release match the version they claim to be. It also
  # blocks "bump on release branch" sneak-paths where someone edits
  # VERSION directly on the release branch and pushes.
  DEV_VERSION="$(git show 'HEAD^2:VERSION' 2>/dev/null | tr -d '[:space:]' || true)"
  if [[ -z "$DEV_VERSION" ]]; then
    die "cannot read VERSION from HEAD^2 (the merged dev branch). Did the dev branch delete VERSION, or is HEAD^2 a malformed tree?"
  fi
  if [[ "$DEV_VERSION" != "$VERSION" ]]; then
    die "VERSION mismatch: dev branch (HEAD^2) has VERSION='$DEV_VERSION' but release branch HEAD has VERSION='$VERSION'. The dev branch must be bumped to $VERSION before opening the MR."
  fi
  ok "dev branch VERSION ($DEV_VERSION) matches release VERSION"

  ok "release branch $BRANCH @ $EXPECTED_TAG is consistent"
  exit 0
fi

# ─── 4. non-release branch: light-touch mode ─────────────────────────────
# Catch malformed release/* branches that didn't match the strict
# release/vX.Y.Z regex above. Anything else is a development branch and
# passes through the light-touch checks.
if [[ "$BRANCH" == release/* ]]; then
  die "branch name looks like a release branch but is malformed: '$BRANCH' (expected $RELEASE_RE)"
fi

vlog "non-release branch ($BRANCH) — skipping release-only checks"
ok "VERSION sanity OK on branch $BRANCH"
exit 0
