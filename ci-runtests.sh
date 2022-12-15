#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

BUILD_RELEASE_REPOSITORY="https://releases.mullvad.net/releases/"
BUILD_DEV_REPOSITORY="https://releases.mullvad.net/builds/"

APP_REPO_URL="https://github.com/mullvad/mullvadvpn-app"

# Infer version from GitHub repo
commit=$(git ls-remote "${APP_REPO_URL}" master)
commit=${commit:0:6}
# TODO: make sure OLD_APP_VERSION is stable
OLD_APP_VERSION=$(curl -f https://raw.githubusercontent.com/mullvad/mullvadvpn-app/${commit}/dist-assets/desktop-product-version.txt)
NEW_APP_VERSION=${OLD_APP_VERSION}-dev-${commit}

echo "$NEW_APP_VERSION" > "$SCRIPT_DIR/.ci-logs/last-version.log"

rustup update
git pull --verify-signatures

function nice_time {
    SECONDS=0
    if $@; then
        result=0
    else
        result=$?
    fi
    s=$SECONDS
    echo "\"$@\" completed in $(($s/60))m:$(($s%60))s"
    return $result
}

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
    local target=$2
    if is_dev_version $version; then
        # only save 6 chars of the hash
        local commit="${BASH_REMATCH[3]}"
        version="${BASH_REMATCH[1]}${commit:0:6}"
    fi
    case $target in
        "x86_64-unknown-linux-gnu")
            echo "MullvadVPN-${version}_amd64.deb"
            ;;
        "x86_64-pc-windows-gnu")
            echo "MullvadVPN-${version}.exe"
            ;;
        *)
            echo "Unsupported target: $target" 1>&2
            return 1
            ;;
    esac
}

function get_target_for_os {
    local os=$1
    case $os in
        debian*)
            echo "x86_64-unknown-linux-gnu"
            ;;
        windows*)
            echo "x86_64-pc-windows-gnu"
            ;;
        *)
            echo "Unsupported OS: $os" 1>&2
            return 1
            ;;
    esac
}

function download_app_package {
    local version=$1
    local target=$2
    local package_repo=""

    if is_dev_version $version; then
        package_repo="${BUILD_DEV_REPOSITORY}"
    else
        package_repo="${BUILD_RELEASE_REPOSITORY}"
    fi

    local filename=$(get_app_filename $version $target)
    local url="${package_repo}/$version/$filename"

    # TODO: integrity check

    echo "Downloading build for $version ($target) from $url"
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

echo "**********************************"
echo "* Version to upgrade from: $OLD_APP_VERSION"
echo "* Version to test: $NEW_APP_VERSION"
echo "**********************************"

echo "**********************************"
echo "* Updating Cargo manifests"
echo "**********************************"

backup_version_metadata
trap "restore_version_metadata" EXIT

function update_manifest_versions {
    pushd ${SCRIPT_DIR}/test-manager
    for new_dep in mullvad-management-interface mullvad-types mullvad-api talpid-types; do
        cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} ${new_dep}
    done
    cargo add --git "${APP_REPO_URL}" --rev ${old_app_commit} --rename old-mullvad-management-interface mullvad-management-interface
    popd

    pushd ${SCRIPT_DIR}/test-runner
    cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} mullvad-management-interface
    cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} mullvad-paths
    cargo add --git "${APP_REPO_URL}" --rev ${new_app_commit} talpid-windows-net --target "cfg(target_os=\"windows\")"
    popd
}

nice_time update_manifest_versions

function run_tests_for_os {
    local os=$1

    local target=$(get_target_for_os $os)
    local prev_filename=$(get_app_filename $OLD_APP_VERSION $target)
    local cur_filename=$(get_app_filename $NEW_APP_VERSION $target)

    OS=$os \
    SKIP_COMPILATION=1 \
    PREVIOUS_APP_FILENAME=$prev_filename \
    CURRENT_APP_FILENAME=$cur_filename \
    ./runtests.sh
}

echo "**********************************"
echo "* Building test runners"
echo "**********************************"

# Clean up packages. Leaving stable versions as they rarely change.
rm -f ${SCRIPT_DIR}/packages/*-dev-*

function build_test_runners {
    for target in x86_64-unknown-linux-gnu x86_64-pc-windows-gnu; do
        nice_time download_app_package $OLD_APP_VERSION $target || true
        nice_time download_app_package $NEW_APP_VERSION $target || true
        TARGET=$target ./build.sh
    done
}

nice_time build_test_runners

echo "**********************************"
echo "* Building test manager"
echo "**********************************"

cargo build -p test-manager

echo "**********************************"
echo "* Clear devices from account"
echo "**********************************"

access_token=$(curl -X POST https://api.mullvad.net/auth/v1/token -d "{\"account_number\":\"$ACCOUNT_TOKEN\"}" -H "Content-Type:application/json" | jq -r .access_token)
device_ids=$(curl https://api.mullvad.net/accounts/v1/devices -H "AUTHORIZATION:Bearer $access_token" | jq -r '.[].id')
for d_id in $(xargs <<< $device_ids)
do
    curl -X DELETE https://api.mullvad.net/accounts/v1/devices/$d_id -H "AUTHORIZATION:Bearer $access_token" &> /dev/null
done

#
# Launch tests in all VMs
#

echo "**********************************"
echo "* Running tests"
echo "**********************************"

i=0
testjobs=""
OSES=(debian11 windows10)

for os in ${OSES[@]}; do

    if [[ $i -gt 0 ]]; then
        # Certain things are racey during setup, like obtaining a pty.
        sleep 5
    fi

    mkdir -p "$SCRIPT_DIR/.ci-logs"

    nice_time run_tests_for_os $os &> "$SCRIPT_DIR/.ci-logs/${os}.log" &
    testjobs[$i]=$!

    let "i=i+1"

done

#
# Wait for them to finish
#

i=0
failed_builds=0

for os in ${OSES[@]}; do
    if wait -fn ${testjobs[$i]}; then
        echo "**********************************"
        echo "* TESTS SUCCEEDED FOR OS: $os"
        echo "**********************************"
        tail -n 1 "$SCRIPT_DIR/.ci-logs/${os}.log"
    else
        let "failed_builds=failed_builds+1"

        echo "**********************************"
        echo "* TESTS FAILED FOR OS: $os"
        echo "* BEGIN LOGS"
        echo "**********************************"
        echo ""

        cat "$SCRIPT_DIR/.ci-logs/${os}.log"

        echo ""
        echo "**********************************"
        echo "* END LOGS FOR OS: $os"
        echo "**********************************"
    fi

    echo ""
    echo ""

    let "i=i+1"
done

exit $failed_builds
