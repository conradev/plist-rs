#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
lock_file="$root_dir/verification/reference.lock"
reference_dir="$(mktemp -d)"
trap 'rm -rf "$reference_dir"' EXIT

lock_value() {
  sed -n "s/^$1=//p" "$lock_file"
}

download_and_verify() {
  local url="$1"
  local expected="$2"
  local destination="$3"
  local actual

  curl --fail --location --silent --show-error "$url" --output "$destination"
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$destination" | awk '{print $1}')"
  else
    actual="$(shasum -a 256 "$destination" | awk '{print $1}')"
  fi
  test "$actual" = "$expected"
}

primary_repository="$(lock_value repository)"
primary_commit="$(lock_value commit)"
hardening_repository="$(lock_value hardening_repository)"
hardening_commit="$(lock_value hardening_commit)"

download_and_verify \
  "${primary_repository/github.com/raw.githubusercontent.com}/${primary_commit}/$(lock_value binary_path)" \
  "$(lock_value binary_sha256)" \
  "$reference_dir/CFBinaryPList.c"
download_and_verify \
  "${primary_repository/github.com/raw.githubusercontent.com}/${primary_commit}/$(lock_value xml_path)" \
  "$(lock_value xml_sha256)" \
  "$reference_dir/CFPropertyList.c"
download_and_verify \
  "${primary_repository/github.com/raw.githubusercontent.com}/${primary_commit}/$(lock_value license_path)" \
  "$(lock_value license_sha256)" \
  "$reference_dir/APPLE_LICENSE"
download_and_verify \
  "${primary_repository/github.com/raw.githubusercontent.com}/${primary_commit}/$(lock_value format_header_path)" \
  "$(lock_value format_header_sha256)" \
  "$reference_dir/ForFoundationOnly.h"
download_and_verify \
  "${primary_repository/github.com/raw.githubusercontent.com}/${primary_commit}/$(lock_value date_path)" \
  "$(lock_value date_sha256)" \
  "$reference_dir/CFDate.c"
download_and_verify \
  "${hardening_repository/github.com/raw.githubusercontent.com}/${hardening_commit}/$(lock_value hardening_binary_path)" \
  "$(lock_value hardening_binary_sha256)" \
  "$reference_dir/Corelibs-CFBinaryPList.c"
download_and_verify \
  "${hardening_repository/github.com/raw.githubusercontent.com}/${hardening_commit}/$(lock_value hardening_xml_path)" \
  "$(lock_value hardening_xml_sha256)" \
  "$reference_dir/Corelibs-CFPropertyList.c"

printf 'Verified pinned CoreFoundation reference hashes.\n'
