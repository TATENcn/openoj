#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failed=0

report() {
  printf 'ERROR: %s\n' "$*" >&2
  failed=1
}

while IFS= read -r -d '' file; do
  case "$file" in
    ./.git/*) continue ;;
  esac

  if [[ ! -s "$file" ]]; then
    report "empty file: ${file#./}"
    continue
  fi

  if head -c 3 "$file" | od -An -tx1 | tr -d ' \n' | grep -qi '^efbbbf$'; then
    report "UTF-8 BOM is forbidden: ${file#./}"
  fi

  case "$file" in
    *.ps1|*.bat|*.cmd) ;;
    *)
      if LC_ALL=C grep -Iq . "$file" && LC_ALL=C grep -q $'\r' "$file"; then
        report "CRLF is forbidden for this file: ${file#./}"
      fi
      ;;
  esac
done < <(find . -type f -print0)

required_metadata=(status owners last_reviewed applies_to references)
allowed_status='^(Proposed|Accepted|Implemented|Validated|Deprecated)$'

while IFS= read -r -d '' file; do
  if [[ "$(head -n 1 "$file")" != '---' ]]; then
    report "missing YAML frontmatter: $file"
    continue
  fi

  frontmatter="$(sed -n '2,/^---$/p' "$file")"
  if [[ -z "$frontmatter" ]] || ! head -n 30 "$file" | tail -n +2 | grep -q '^---$'; then
    report "unterminated YAML frontmatter: $file"
    continue
  fi

  for key in "${required_metadata[@]}"; do
    if ! grep -q "^${key}:" <<<"$frontmatter"; then
      report "missing frontmatter key '$key': $file"
    fi
  done

  status="$(sed -n 's/^status:[[:space:]]*//p' <<<"$frontmatter" | head -n 1)"
  if [[ ! "$status" =~ $allowed_status ]]; then
    report "invalid document status '$status': $file"
  fi

  reviewed="$(sed -n 's/^last_reviewed:[[:space:]]*//p' <<<"$frontmatter" | head -n 1)"
  if [[ ! "$reviewed" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    report "invalid last_reviewed date '$reviewed': $file"
  fi

  while IFS= read -r reference; do
    [[ -z "$reference" ]] && continue
    case "$reference" in
      http://*|https://*|mailto:*|app://*|/*) continue ;;
    esac

    if [[ ! -e "$(dirname "$file")/$reference" ]]; then
      report "broken frontmatter reference in $file -> $reference"
    fi
  done < <(sed -n '/^references:/,/^---$/p' "$file" | sed -n 's/^  - //p')
done < <(find docs -type f -name '*.md' -print0)

while IFS=: read -r file line match; do
  target="${match#](}"
  target="${target%)}"
  target="${target%%#*}"

  [[ -z "$target" ]] && continue
  case "$target" in
    http://*|https://*|mailto:*|app://*|/*) continue ;;
  esac

  if [[ ! -e "$(dirname "$file")/$target" ]]; then
    report "broken relative link at $file:$line -> $target"
  fi
done < <(rg --no-heading --line-number --only-matching '\]\([^)]+\)' -g '*.md' . || true)

while IFS= read -r missing_id; do
  [[ -z "$missing_id" ]] && continue
  report "Accepted requirement has no acceptance mapping: $missing_id"
done < <(
  awk '
    /^### (FR|NFR)-/ {
      if (id != "" && accepted && !mapped) print id
      id = $2
      sub(/：.*/, "", id)
      accepted = 0
      mapped = 0
    }
    /^状态：Accepted/ { accepted = 1 }
    /^验收：/ { mapped = 1 }
    END { if (id != "" && accepted && !mapped) print id }
  ' docs/requirements/functional.md docs/requirements/non-functional.md
)

while IFS= read -r acceptance_id; do
  [[ -z "$acceptance_id" ]] && continue
  if ! rg -q "^### ${acceptance_id}：" docs/requirements/acceptance.md; then
    report "requirement references undeclared acceptance ID: $acceptance_id"
  fi
done < <(
  rg --no-filename --only-matching 'ACC-[A-Z0-9]+-[0-9]+' \
    docs/requirements/functional.md docs/requirements/non-functional.md | sort -u
)

duplicate_ids="$(
  rg --no-filename '^#{2,6} (FR|NFR|ACC|THR|RISK|ADR)-[A-Z0-9-]+' docs \
    | sed -E 's/^#+ ([^：: ]+).*/\1/' | sort | uniq -d
)"
if [[ -n "$duplicate_ids" ]]; then
  while IFS= read -r duplicate_id; do
    report "duplicate stable ID heading: $duplicate_id"
  done <<<"$duplicate_ids"
fi

while IFS= read -r -d '' skill_dir; do
  skill_name="$(basename "$skill_dir")"
  skill_file="$skill_dir/SKILL.md"
  ui_file="$skill_dir/agents/openai.yaml"

  [[ -f "$skill_file" ]] || { report "missing SKILL.md: $skill_dir"; continue; }
  [[ -f "$ui_file" ]] || { report "missing agents/openai.yaml: $skill_dir"; continue; }

  declared_name="$(sed -n 's/^name:[[:space:]]*//p' "$skill_file" | head -n 1)"
  [[ "$declared_name" == "$skill_name" ]] || report "skill name '$declared_name' does not match directory '$skill_name'"
  grep -q '^description: .\+' "$skill_file" || report "missing skill description: $skill_file"
  grep -q "\$${skill_name}" "$ui_file" || report "default_prompt must mention \$${skill_name}: $ui_file"
  if rg -n '\[TODO|TODO:' "$skill_dir" >/dev/null; then
    report "unresolved skill template TODO: $skill_dir"
  fi
done < <(find skills -mindepth 1 -maxdepth 1 -type d -print0)

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

printf 'Documentation and skill structure checks passed.\n'
