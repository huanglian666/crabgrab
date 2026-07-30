#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$project_root/tools/ffmpeg-manifest.toml"
target=${1:-}

if [ -z "$target" ]; then
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) target="aarch64-apple-darwin" ;;
        *)
            echo "unsupported host; pass a target explicitly" >&2
            exit 2
            ;;
    esac
fi

manifest_value() {
    awk -v wanted="$target" -v wanted_key="$1" '
        /^\[\[platform\]\]/ { active = 0 }
        /^target[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            active = (value == wanted)
        }
        active && $0 ~ ("^" wanted_key "[[:space:]]*=") {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            print value
            exit
        }
    ' "$manifest"
}

archive_url=$(manifest_value archive_url)
archive_sha256=$(manifest_value archive_sha256)
archive_entry=$(manifest_value archive_entry)
executable_name=$(manifest_value executable)
executable_sha256=$(manifest_value executable_sha256)
license_url=$(awk -F '"' '/^license_url[[:space:]]*=/ { print $2; exit }' "$manifest")
license_sha256=$(awk -F '"' '/^license_sha256[[:space:]]*=/ { print $2; exit }' "$manifest")

if [ -z "$archive_url" ] || [ -z "$executable_sha256" ]; then
    echo "target not found in $manifest: $target" >&2
    exit 2
fi

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

destination="$project_root/.crabgrab-tools/ffmpeg/$target"
installed="$destination/$executable_name"
if [ -f "$installed" ] && [ "$(sha256_file "$installed")" = "$executable_sha256" ]; then
    echo "FFmpeg is already installed at $installed"
    exit 0
fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/crabgrab-ffmpeg.XXXXXX")
trap 'rm -rf "$temporary"' EXIT INT TERM

curl -L --fail --show-error --silent "$archive_url" -o "$temporary/ffmpeg.zip"
if [ "$(sha256_file "$temporary/ffmpeg.zip")" != "$archive_sha256" ]; then
    echo "FFmpeg archive SHA-256 mismatch" >&2
    exit 1
fi
unzip -p "$temporary/ffmpeg.zip" "$archive_entry" > "$temporary/$executable_name"
if [ "$(sha256_file "$temporary/$executable_name")" != "$executable_sha256" ]; then
    echo "FFmpeg executable SHA-256 mismatch" >&2
    exit 1
fi
chmod 755 "$temporary/$executable_name"

curl -L --fail --show-error --silent "$license_url" -o "$temporary/LICENSE"
if [ "$(sha256_file "$temporary/LICENSE")" != "$license_sha256" ]; then
    echo "FFmpeg license SHA-256 mismatch" >&2
    exit 1
fi

mkdir -p "$destination"
mv "$temporary/$executable_name" "$installed.new"
mv "$temporary/LICENSE" "$destination/LICENSE.new"
mv "$installed.new" "$installed"
mv "$destination/LICENSE.new" "$destination/LICENSE"

echo "installed FFmpeg at $installed"
