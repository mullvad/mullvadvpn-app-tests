#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

git pull

# Use complete version strings: e.g. 2022.5, or 2022.5-dev-6efde7
OLD_APP_VERSION=$1
NEW_APP_VERSION=$2

TARGET_OS="debian"

BUILD_RELEASE_REPOSITORY="https://releases.mullvad.net/releases/"
BUILD_DEV_REPOSITORY="https://releases.mullvad.net/builds/"

APP_REPO_URL="https://github.com/mullvad/mullvadvpn-app"

# Returns 0 if $1 is a development build. `BASH_REMATCH` contains match groups
# if that is the case.
function is_dev_version {
    if [[ $1 =~ (^[0-9.]+(-beta[0-9]+)?-dev-)([0-9a-z]+)$ ]]; then
        return 0
    fi
    return 1
}

function find_version_commit {
    local commit=""

    if is_dev_version $1; then
        # dev version
        commit="${BASH_REMATCH[3]}"
    else
        # release version
        commit=$(git ls-remote "${APP_REPO_URL}" $1)
    fi

    if [[ -z "${commit}" ]]; then
        echo "Failed to identify commit hash for version: $1" 1>&2
        return 1
    fi

    echo ${commit:0:6}
}

function get_app_filename {
    local version=$1
    if is_dev_version $version; then
        # only save 6 chars of the hash
        local commit="${BASH_REMATCH[3]}"
        version="${BASH_REMATCH[1]}${commit:0:6}"
    fi
    case $TARGET_OS in
        debian)
            echo "MullvadVPN-${version}_amd64.deb"
            ;;
        windows)
            echo "MullvadVPN-${version}.exe"
            ;;
        *)
            echo "Unsupported OS: $TARGET_OS" 1>&2
            return 1
            ;;
    esac
}

function download_app_package {
    local version=$1
    local package_repo=""

    if is_dev_version $1; then
        package_repo="${BUILD_DEV_REPOSITORY}"
    else
        package_repo="${BUILD_RELEASE_REPOSITORY}"
    fi

    local filename=$(get_app_filename $1)
    local url="${package_repo}/$1/$filename"

    # TODO: integrity check

    echo "Downloading build for $1 from $url"
    mkdir -p "$SCRIPT_DIR/packages/"
    if [[ ! -f "$SCRIPT_DIR/packages/$filename" ]]; then
        curl -f -o "$SCRIPT_DIR/packages/$filename" $url
    fi
}

function backup_version_metadata {
    cp ${SCRIPT_DIR}/Cargo.lock{,.bak}
    cp ${SCRIPT_DIR}/test-rpc/Cargo.toml{,.bak}
    cp ${SCRIPT_DIR}/test-runner/Cargo.toml{,.bak}
    cp ${SCRIPT_DIR}/test-manager/Cargo.toml{,.bak}
}

function restore_version_metadata {
    mv ${SCRIPT_DIR}/test-manager/Cargo.toml{.bak,}
    mv ${SCRIPT_DIR}/test-runner/Cargo.toml{.bak,}
    mv ${SCRIPT_DIR}/test-rpc/Cargo.toml{.bak,}
    mv ${SCRIPT_DIR}/Cargo.lock{.bak,}
}

old_app_commit=$(find_version_commit $OLD_APP_VERSION)
new_app_commit=$(find_version_commit $NEW_APP_VERSION)

echo "Version to upgrade from: $old_app_commit ($OLD_APP_VERSION)"
echo "Version to test: $new_app_commit ($NEW_APP_VERSION)"

download_app_package $OLD_APP_VERSION
download_app_package $NEW_APP_VERSION

echo "Updating Cargo manifests"

backup_version_metadata
trap "restore_version_metadata" EXIT

pushd ${SCRIPT_DIR}/test-manager
for new_dep in mullvad-management-interface mullvad-types mullvad-api talpid-types; do
    cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} ${new_dep}
done
cargo add --git "${APP_REPO_URL}" --rev ${old_app_commit} --rename old-mullvad-management-interface mullvad-management-interface
popd

pushd ${SCRIPT_DIR}/test-runner
cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} mullvad-management-interface
cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} talpid-windows-net --target "cfg(target_os=\"windows\")"
cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} mullvad-paths --target "cfg(target_os=\"windows\")"
popd

export PREVIOUS_APP_FILENAME=$(get_app_filename $OLD_APP_VERSION)
export CURRENT_APP_FILENAME=$(get_app_filename $NEW_APP_VERSION)

case $TARGET_OS in
    debian)
        export TARGET="x86_64-unknown-linux-gnu"
        ;;
    windows)
        export TARGET="x86_64-pc-windows-gnu"
        ;;
    *)
        echo "Unsupported OS: $TARGET_OS" 1>&2
        return 1
        ;;
esac

./runtests.sh
