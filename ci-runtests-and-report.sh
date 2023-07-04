#!/usr/bin/env bash

set -eu

source "$HOME/.cargo/env"

EMAIL_SUBJECT_PREFIX="App test results"
SENDER_EMAIL_ADDR="test@app-test-linux"
REPORT_ON_SUCCESS=1

if [[ -z "${RECIPIENT_EMAIL_ADDRS+x}" ]]; then
    echo "'RECIPIENT_EMAIL_ADDRS' must be specified" 1>&2
    exit 1
fi

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

rm -f "$SCRIPT_DIR/.ci-logs/last-version.log"
rm -rf "$SCRIPT_DIR/.ci-logs/os"

set +e
exec 3>&1
REPORT=$(./ci-runtests.sh $@ 2>&1 | tee >(cat >&3); exit "${PIPESTATUS[0]}")
EXIT_STATUS=$?
set -e

if [[ $REPORT_ON_SUCCESS -eq 0 && $EXIT_STATUS -eq 0 ]]; then
    echo "Not sending email report since tests were successful"
    exit 0
fi

tested_version=$(cat "$SCRIPT_DIR/.ci-logs/last-version.log" || echo "unknown version")

if [[ $EXIT_STATUS -eq 0 ]]; then
    EMAIL_SUBJECT_SUFFIX=" for $tested_version: Succeeded"
else
    EMAIL_SUBJECT_SUFFIX=" for $tested_version: Failed"
fi

echo "Sending email reports"

REPORT_PATH="${SCRIPT_DIR}/.ci-logs/app-testing-$(date +%Y-%m-%d_%H_%M).log"
cat -v - <<<"${REPORT}">"${REPORT_PATH}"

# Attach individual OS logs
ATTACHMENT_PATHS=()
for file in $(find "$SCRIPT_DIR/.ci-logs/os" -type f); do
    ATTACHMENT_PATHS=("${ATTACHMENT_PATHS[@]}" -a "${file}")
done

/usr/bin/mailx \
    -s "${EMAIL_SUBJECT_PREFIX}${EMAIL_SUBJECT_SUFFIX}" \
    -r "${SENDER_EMAIL_ADDR}" \
    -S sendcharsets=utf-8 \
    -S sendwait \
    -a "${REPORT_PATH}" \
    "${ATTACHMENT_PATHS[@]}" \
    "${RECIPIENT_EMAIL_ADDRS}" <<<""
