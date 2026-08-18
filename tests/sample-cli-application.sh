#!/bin/bash

# Sample greeter application (AI-generated), for testing Manuel

set -u

prog=$(basename "$0")

usage() {
    cat <<USAGE
Greets a user by name.

Usage: $prog [FIRST_NAME] [LAST_NAME]

Arguments:
  [FIRST_NAME]  The user's first name
  [LAST_NAME]   The user's last name

Options:
  -h, --help     Print help
  -V, --version  Print version
USAGE
}

fail() {
    echo "error: $1" >&2
    exit 1
}

# Reads a line into the variable named by $2, after printing the label $1.
prompt() {
    printf '%s' "$1"
    if ! IFS= read -r __reply; then
        return 1
    fi
    # trim surrounding whitespace
    __reply="${__reply#"${__reply%%[![:space:]]*}"}"
    __reply="${__reply%"${__reply##*[![:space:]]}"}"
    printf -v "$2" '%s' "$__reply"
}

positional=()
for arg in "$@"; do
    case "$arg" in
        -h|--help)    usage; exit 0 ;;
        -V|--version) echo "$prog 0.1.0"; exit 0 ;;
        -*)           usage >&2; fail "unexpected argument '$arg' found" ;;
        *)            positional+=("$arg") ;;
    esac
done

if [ "${#positional[@]}" -gt 2 ]; then
    usage >&2
    fail "unexpected argument '${positional[2]}' found"
fi

first_name="${positional[0]-}"
last_name="${positional[1]-}"

if [ "${#positional[@]}" -lt 2 ]; then
    if [ "${#positional[@]}" -lt 1 ]; then
        prompt "First name: " first_name || fail "failed to read first name"
    fi
    prompt "Last name: " last_name || fail "failed to read last name"
fi

if [ "$first_name" = "$last_name" ]; then
    fail "first name and last name must not be the same"
fi

echo "Hello, $first_name $last_name!"
