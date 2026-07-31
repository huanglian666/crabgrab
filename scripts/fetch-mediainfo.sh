#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$project_root/tools/mediainfo-manifest.toml"
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

platform_value() {
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

runtime_value() {
    awk -v wanted="$target" -v wanted_key="$1" '
        /^\[\[platform\]\]/ { active = 0; runtime = 0 }
        /^target[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            active = (value == wanted)
        }
        /^\[\[platform\.runtime_file\]\]/ { runtime = active }
        runtime && $0 ~ ("^" wanted_key "[[:space:]]*=") {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            print value
            exit
        }
    ' "$manifest"
}

archive_url=$(platform_value archive_url)
archive_sha256=$(platform_value archive_sha256)
executable_name=$(platform_value executable)
executable_sha256=$(platform_value executable_sha256)
runtime_name=$(runtime_value name)
runtime_sha256=$(runtime_value sha256)

if [ -z "$archive_url" ] || [ -z "$archive_sha256" ] || \
    [ -z "$executable_name" ] || [ -z "$executable_sha256" ]; then
    echo "target not found or incomplete in $manifest: $target" >&2
    exit 2
fi

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

destination="$project_root/.crabgrab-tools/mediainfo/$target"
installed_is_valid() {
    [ -f "$destination/$executable_name" ] || return 1
    [ "$(sha256_file "$destination/$executable_name")" = "$executable_sha256" ] || return 1
    if [ -n "$runtime_name" ]; then
        [ -f "$destination/$runtime_name" ] || return 1
        [ "$(sha256_file "$destination/$runtime_name")" = "$runtime_sha256" ] || return 1
    fi
}

if installed_is_valid; then
    echo "MediaInfo is already installed at $destination/$executable_name"
    exit 0
fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/crabgrab-mediainfo.XXXXXX")
mount_point=
stage=
backup=
cleanup() {
    if [ -n "$mount_point" ]; then
        # macOS may expose /var paths as /private/var in `mount`, so matching the
        # displayed mount path can miss an active disk image. Detaching an
        # already-unmounted path is harmless here and cleanup must continue.
        hdiutil detach "$mount_point" -quiet >/dev/null 2>&1 || true
    fi
    [ -z "$stage" ] || rm -rf "$stage"
    [ -z "$backup" ] || rm -rf "$backup"
    rm -rf "$temporary"
}
trap cleanup EXIT INT TERM

archive="$temporary/archive"
curl -L --fail --show-error --silent "$archive_url" -o "$archive"
if [ "$(sha256_file "$archive")" != "$archive_sha256" ]; then
    echo "MediaInfo archive SHA-256 mismatch" >&2
    exit 1
fi

extracted_executable="$temporary/$executable_name"
case "$target" in
    x86_64-pc-windows-msvc)
        executable_entry=$(unzip -Z1 "$archive" | awk -F/ -v name="$executable_name" 'tolower($NF) == tolower(name) { print; exit }')
        if [ -z "$executable_entry" ]; then
            echo "MediaInfo executable is missing from archive" >&2
            exit 1
        fi
        unzip -p "$archive" "$executable_entry" > "$extracted_executable"
        if [ -n "$runtime_name" ]; then
            runtime_entry=$(unzip -Z1 "$archive" | awk -F/ -v name="$runtime_name" 'tolower($NF) == tolower(name) { print; exit }')
            if [ -z "$runtime_entry" ]; then
                echo "MediaInfo runtime file is missing from archive: $runtime_name" >&2
                exit 1
            fi
            unzip -p "$archive" "$runtime_entry" > "$temporary/$runtime_name"
        fi
        ;;
    aarch64-apple-darwin)
        mount_point="$temporary/mount"
        mkdir "$mount_point"
        hdiutil attach "$archive" -readonly -nobrowse -mountpoint "$mount_point" -quiet
        source_executable=$(find "$mount_point" -type f -name "$executable_name" -print | head -1)
        if [ -z "$source_executable" ]; then
            package=$(find "$mount_point" -type f -name '*.pkg' -print | head -1)
            if [ -n "$package" ]; then
                pkgutil --expand-full "$package" "$temporary/package"
                source_executable=$(find "$temporary/package" -type f -name "$executable_name" -print | head -1)
            fi
        fi
        if [ -z "$source_executable" ]; then
            echo "MediaInfo executable is missing from disk image" >&2
            exit 1
        fi
        cp "$source_executable" "$extracted_executable"
        chmod 755 "$extracted_executable"
        ;;
    *)
        echo "unsupported target: $target" >&2
        exit 2
        ;;
esac

if [ "$(sha256_file "$extracted_executable")" != "$executable_sha256" ]; then
    echo "MediaInfo executable SHA-256 mismatch" >&2
    exit 1
fi
if [ -n "$runtime_name" ] && \
    [ "$(sha256_file "$temporary/$runtime_name")" != "$runtime_sha256" ]; then
    echo "MediaInfo runtime SHA-256 mismatch: $runtime_name" >&2
    exit 1
fi

parent=$(dirname "$destination")
mkdir -p "$parent"
stage=$(mktemp -d "$parent/.install.XXXXXX")
cp "$extracted_executable" "$stage/$executable_name"
[ -z "$runtime_name" ] || cp "$temporary/$runtime_name" "$stage/$runtime_name"
cp "$project_root/licenses/MediaInfo.txt" "$stage/LICENSE"

if [ -e "$destination" ]; then
    backup="$parent/.backup.$$"
    if [ -e "$backup" ]; then
        echo "MediaInfo backup path already exists: $backup" >&2
        exit 1
    fi
    mv "$destination" "$backup"
fi
if ! mv "$stage" "$destination"; then
    [ -z "$backup" ] || mv "$backup" "$destination"
    exit 1
fi
stage=
[ -z "$backup" ] || rm -rf "$backup"
backup=

echo "installed MediaInfo at $destination/$executable_name"
