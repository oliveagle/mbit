#!/usr/bin/env bash
# Bump version for devine_infra.
#
# Release model (v2, 2026-07-03; see devine_doc/docs/release-management_20260703.md
# and AGENTS.md §"Release 管理规范(强制)"):
#   - VERSION file is the single source of truth (bare x.y.z, no `v`).
#   - Tags use the `v` prefix (`v0.1.4`).
#   - Bump happens on a dev branch (main / chore/* / feat/*).
#     The release branch `release/vX.Y.Z` is protected: pushes are rejected
#     by GitLab; merges go through MR.
#   - Tagging is a web-side step (GitLab Repository → Tags → New tag,
#     targeting `release/vX.Y.Z` HEAD). This script NEVER creates or
#     pushes tags.
#   - Bump commit is independent — touches only the VERSION file. This
#     is enforced by `scripts/check_version.sh` (VersionGate) for any
#     `release/v*` branch and is a hard invariant for release branches.
#   - Core invariant (must hold after every release):
#       origin/main == origin/release/vX.Y.Z == vX.Y.Z^{commit}
#
# Usage:
#   scripts/bump_version.sh current                          # 查看当前版本
#   scripts/bump_version.sh patch                            # 0.1.3 → 0.1.4
#   scripts/bump_version.sh minor                            # 0.1.3 → 0.2.0
#   scripts/bump_version.sh major                            # 0.1.3 → 1.0.0
#   scripts/bump_version.sh release-branch                   # 打印 release 分支名
#   scripts/bump_version.sh create-release-branch 0.1.5      # GitLab API 创建 release/v0.1.5 (从 main)
#   scripts/bump_version.sh patch -n                         # dry-run
#   scripts/bump_version.sh current --no-remote              # 不访问 remote
#
# Exit codes:
#   0  success
#   1  usage / parse error
#   2  precondition failed (no VERSION, dirty tree, …)
#   3  remote out of sync / version invariant violated
#   4  quality gate failed
#
# Post-bump flow (NOT done by this script — handled by maintainer):
#   1. `git push origin <dev-branch>`  (pre-push hook runs quality-check)
#   2. Open MR: <dev-branch> → release/vX.Y.Z (creates the release branch
#      on first push)
#   3. After MR merges, on GitLab web: Repository → Tags → New tag
#      `vX.Y.Z` targeting `release/vX.Y.Z` HEAD.
#   4. Verify with `scripts/check_version.sh -v` (also runs in CI
#      VersionGate stage on release/v* branches).

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

VERSION_FILE="VERSION"
REMOTE_NAME="${DEVINE_REMOTE:-devine_infra}"
DRY_RUN=0
NO_REMOTE=0

# Parse arguments. Flags can appear in any position; ACTION is the
# first non-flag argument.
ARGS=()
for arg in "$@"; do
    case "$arg" in
        -n)        DRY_RUN=1 ;;
        --no-remote) NO_REMOTE=1 ;;
        -h|--help)
            sed -n '2,38p' "$0"
            exit 0
            ;;
        -*)        echo "未知 flag: $arg" >&2; exit 1 ;;
        *)         ARGS+=("$arg") ;;
    esac
done
ACTION="${ARGS[0]:-current}"

# --- helpers --------------------------------------------------------------

die() {
    echo -e "${RED}错误: $1${NC}" >&2
    exit "${2:-1}"
}

run() {
    if [ "$DRY_RUN" -eq 1 ]; then
        echo -e "${YELLOW}(dry-run) $*${NC}"
    else
        "$@"
    fi
}

current_version() {
    if [ ! -f "$VERSION_FILE" ]; then
        die "$VERSION_FILE 不存在" 2
    fi
    cat "$VERSION_FILE" | tr -d '[:space:]'
}

require_clean_tree() {
    if [ -n "$(git status --porcelain)" ]; then
        die "工作树不干净,提交或 stash 后再试" 2
    fi
}

# release_branch_name [version]  — print "release/vX.Y.Z" for given
# version (defaults to current VERSION file).
release_branch_name() {
    local v
    if [ "${1:-}" != "" ]; then
        v="$1"
    else
        v=$(current_version 2>/dev/null || true)
        if [ -z "$v" ]; then
            die "无法读取 $VERSION_FILE" 2
        fi
    fi
    echo "release/v$v"
}

# create_release_branch <version>  — create release/v<version> branch on
# the GitLab remote via the API, branching from origin/main. The branch
# is created EMPTY (no commits cherry-picked into it) — the dev branch
# (e.g. chore/bump-v0.1.5) is later merged into it via MR #3 of the
# 3-MR flow.
#
# Idempotent: if the branch already exists on origin, print a notice
# and exit 0 (no error, no force-push).
#
# Requires: dotvault-managed GitLab token (DEVINE_GIT_GROUP_ACCESS_KEY)
# exposed as the env var DEVINE_GIT_KEY. If unset, fall back to reading
# from dotvault at $DEVINE_GROUPS_ROOT.
create_release_branch() {
    local v="${1:-}"
    if [ -z "$v" ]; then
        die "用法: $0 create-release-branch <X.Y.Z>" 1
    fi
    if ! [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        die "版本号格式错: '$v' (需要 X.Y.Z)" 1
    fi
    local branch="release/v$v"
    local remote_branch_sha
    local try_remote
    for try_remote in "$REMOTE_NAME" origin; do
        remote_branch_sha=$(git ls-remote --heads "$try_remote" "$branch" 2>/dev/null | awk '{print $1}' | head -1 || true)
        if [ -n "$remote_branch_sha" ]; then
            break
        fi
    done
    if [ -n "$remote_branch_sha" ]; then
        echo "$branch 已存在 on $try_remote ($remote_branch_sha) — 跳过创建"
        return 0
    fi
    if [ "$DRY_RUN" -eq 1 ]; then
        echo "(dry-run) 会调 GitLab API 创建 $branch from main"
        return 0
    fi

    # Resolve GitLab token: prefer $DEVINE_GIT_KEY (callers can pre-load);
    # fall back to dotvault at $DEVINE_GROUPS_ROOT (walks up from script
    # location looking for .dotvault_key).
    local token="${DEVINE_GIT_KEY:-}"
    if [ -z "$token" ]; then
        local groups_root="${DEVINE_GROUPS_ROOT:-}"
        if [ -z "$groups_root" ]; then
            local dir
            dir=$(cd "$(dirname "$0")/.." && pwd)
            while [ "$dir" != "/" ]; do
                if [ -f "$dir/.dotvault_key" ]; then
                    groups_root="$dir"
                    break
                fi
                dir=$(dirname "$dir")
            done
        fi
        if [ -z "$groups_root" ]; then
            die "找不到 devine_groups 根目录(无 .dotvault_key),无法解析 GitLab token。设置 DEVINE_GIT_KEY 环境变量后重试。" 3
        fi
        token=$(cd "$groups_root" && dotvault --key ~/.ssh/id_ed25519 get DEVINE_GIT_GROUP_ACCESS_KEY 2>/dev/null || true)
        if [ -z "$token" ]; then
            die "dotvault 解析 GitLab token 失败(检查 dotvault 配置 + DEVINE_GIT_GROUP_ACCESS_KEY secret)" 3
        fi
    fi

    # Determine the GitLab project path from the remote URL. Tries
    # $REMOTE_NAME first (default: devine_infra), then falls back to
    # origin (this repo's actual remote name). Accepts both SSH form
    # (git@host:group/proj.git) and HTTPS form (https://host/group/proj.git).
    local project_path
    local try_remote
    for try_remote in "$REMOTE_NAME" origin; do
        project_path=$(git remote get-url "$try_remote" 2>/dev/null \
            | sed -E 's#^[^:/]+://[^/]+/##; s#^[^@]+@[^:]+:##; s#\.git$##' \
            | head -1 || true)
        if [ -n "$project_path" ]; then
            break
        fi
    done
    if [ -z "$project_path" ]; then
        die "无法从 remote '$REMOTE_NAME' (或 origin) 解析 GitLab project path" 3
    fi
    local api_base="https://git.dev.sh.ctripcorp.com"
    local project_enc
    project_enc=$(printf '%s' "$project_path" | python3 -c "import sys, urllib.parse; print(urllib.parse.quote(sys.stdin.read(), safe=''))")

    echo "正在创建 $branch on $project_path (from main)..."
    local response
    response=$(curl -sS --connect-timeout 5 -X POST \
        -H "PRIVATE-TOKEN: $token" \
        -H "Content-Type: application/json" \
        -d "{\"branch\": \"$branch\", \"ref\": \"main\"}" \
        "$api_base/api/v4/projects/$project_enc/repository/branches")
    if echo "$response" | python3 -c "import sys, json; d=json.load(sys.stdin); sys.exit(0 if 'name' in d and d.get('name')==\"$branch\" else 1)" 2>/dev/null; then
        local created_sha
        created_sha=$(echo "$response" | python3 -c "import sys, json; print(json.load(sys.stdin)['commit']['id'])" 2>/dev/null)
        echo "✓ $branch created ($created_sha)"
        return 0
    fi
    local err
    err=$(echo "$response" | python3 -c "import sys, json; d=json.load(sys.stdin); print(d.get('message', d))" 2>/dev/null || echo "$response")
    die "GitLab API 创建分支失败: $err" 3
}

# --- main -----------------------------------------------------------------

# `release-branch` is a query that doesn't need a real VERSION file path
# resolution via the main pipeline — handle it up front. Re-read VERSION
# from disk if it exists, otherwise just print the literal default.
if [ "$ACTION" = "release-branch" ]; then
    if [ -f "$VERSION_FILE" ]; then
        v=$(cat "$VERSION_FILE" | tr -d '[:space:]')
        if [ -n "$v" ]; then
            echo "release/v$v"
            exit 0
        fi
    fi
    echo "release/v<unknown>" >&2
    exit 1
fi

CURRENT=$(current_version)

# Split version
IFS='.' read -ra PARTS <<< "$CURRENT"
MAJOR="${PARTS[0]}"
MINOR="${PARTS[1]}"
PATCH="${PARTS[2]}"

case "$ACTION" in
    current)
        echo "当前版本: $CURRENT"
        echo "release 分支: release/v$CURRENT"
        echo "remote:      $REMOTE_NAME"
        exit 0
        ;;
    patch)
        NEW_PATCH=$((PATCH + 1))
        NEW="$MAJOR.$MINOR.$NEW_PATCH"
        ;;
    minor)
        NEW_MINOR=$((MINOR + 1))
        NEW="$MAJOR.$NEW_MINOR.0"
        ;;
    major)
        NEW_MAJOR=$((MAJOR + 1))
        NEW="$NEW_MAJOR.0.0"
        ;;
    create-release-branch)
        create_release_branch "$2"
        exit $?
        ;;
    *)
        die "未知操作 '$ACTION'; 用法: $0 {current|patch|minor|major|release-branch|create-release-branch} [-n] [--no-remote]" 1
        ;;
esac

echo "版本变更: $CURRENT → $NEW"
echo "新 release 分支: release/v$NEW"

# Pre-flight checks (read-only — safe in dry-run)
require_clean_tree

if [ "$DRY_RUN" -eq 1 ]; then
    echo -e "${YELLOW}(dry-run) 未实际修改文件、未创建 commit、未 push${NC}"
    echo -e "${YELLOW}(dry-run) 会执行: echo '$NEW' > $VERSION_FILE → quality-check → commit(只动 VERSION)→ 提示你在 dev 分支上 push 并开 MR 到 release/v$NEW${NC}"
    exit 0
fi

# Invariant: every existing v* tag must be reachable from the dev branch
# we're bumping on. If not, this branch cannot serve as a release base —
# opening an MR from it would re-introduce commits that were never part
# of any shipped release.
verify_version_invariant() {
    local current_branch
    current_branch=$(git rev-parse --abbrev-ref HEAD)
    local broken
    broken=$(git tag -l 'v*' | while read -r t; do
        if ! git merge-base --is-ancestor "$t" "$current_branch" 2>/dev/null; then
            echo "$t"
        fi
    done)
    if [ -n "$broken" ]; then
        die "version invariant 违反: 以下 tag 不是当前分支 '$current_branch' 的祖先 — 当前分支不能作为新 release 的基线: $(echo $broken | tr '\n' ' ')" 3
    fi
}

# Additional remote-aware check: if a `release/vX.Y.Z` branch already
# exists on origin, our local branch must be its descendant (or a fast-
# forward of it) — otherwise the MR would rewind the release line.
verify_release_lineage() {
    if [ "$NO_REMOTE" -eq 1 ]; then
        return 0
    fi
    if ! git remote get-url "$REMOTE_NAME" >/dev/null 2>&1; then
        return 0
    fi
    git fetch "$REMOTE_NAME" --prune >/dev/null 2>&1 || true
    local current_branch
    current_branch=$(git rev-parse --abbrev-ref HEAD)
    local existing
    existing=$(git ls-remote --heads "$REMOTE_NAME" "release/v$NEW" 2>/dev/null | awk '{print $1}' | head -1 || true)
    if [ -n "$existing" ]; then
        if ! git merge-base --is-ancestor "$existing" "HEAD" 2>/dev/null; then
            die "origin/release/v$NEW ($existing) 不是当前 HEAD 的祖先 — 在新基线发布前,需先合入现有 release/v$NEW 的修复(或切到该分支的 descendant)。" 3
        fi
    fi
}

verify_version_invariant
verify_release_lineage

# 1. Update VERSION file
echo "$NEW" > "$VERSION_FILE"

# 2. Run quality gate (BLOCKING)
echo -e "${YELLOW}运行质量门禁…${NC}"
if [ -x "./scripts/quality-check.sh" ]; then
    if ! ./scripts/quality-check.sh; then
        # Roll back VERSION so the tree returns to the pre-bump state
        # (the bump commit hasn't been made yet, so the tree will
        # revert on its own once VERSION is rewritten).
        echo "$CURRENT" > "$VERSION_FILE"
        die "质量门禁失败,已回滚 VERSION → $CURRENT" 4
    fi
else
    echo -e "${YELLOW}警告: 质量门禁脚本不存在或不可执行,跳过${NC}"
fi

# 3. Independent bump commit (touches ONLY the VERSION file)
COMMIT_MSG="bump: version $CURRENT → $NEW"
run git add "$VERSION_FILE"
run git commit -m "$COMMIT_MSG"

# 4. Sanity: confirm last commit only touched VERSION
DIFF_FILES=$(git diff-tree --no-commit-id --name-only -r HEAD~1..HEAD 2>/dev/null || true)
if [ -n "$DIFF_FILES" ] && [ "$(printf '%s' "$DIFF_FILES" | tr -d '[:space:]')" != "$VERSION_FILE" ]; then
    die "internal error: bump commit 触碰了非 VERSION 文件,拒绝继续。\nTouched:\n$DIFF_FILES" 3
fi

TAG="v$NEW"

echo -e "${GREEN}✓ 版本已更新: $CURRENT → $NEW${NC}"
echo "  Commit: $COMMIT_MSG"
echo "  Tag:    $TAG (NOT created — 在 GitLab web 对 release/v$NEW HEAD 打)"
echo ""
echo -e "${YELLOW}下一步(维护者手工执行):${NC}"
echo "  1. push 当前 dev 分支(pre-push hook 跑 quality-check):"
echo "       git push origin HEAD"
echo "  2. 在 GitLab web 开 MR: HEAD → release/v$NEW"
echo "  3. review + 质量门禁通过后合并"
echo "  4. GitLab web: Repository → Tags → New tag '$TAG' targeting release/v$NEW HEAD"
echo "  5. 验证三轨一致: scripts/check_version.sh -v"
echo ""
echo "  内部消费者: 'go get git.dev.sh.ctripcorp.com/devine/devine_infra@$TAG'"
