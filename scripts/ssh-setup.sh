#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd $SCRIPT_DIR

RUNNER_DIR="$1"
CURRENT_APP="$2"
PREVIOUS_APP="$3"
UI_RUNNER="$4"

DAEMON_PATH="/Library/LaunchDaemons/net.mullvad.testunner.plist"

# Copy over test runner to correct place

echo "Copying test-runner to $RUNNER_DIR"

mkdir -p $RUNNER_DIR

for file in test-runner $CURRENT_APP $PREVIOUS_APP $UI_RUNNER; do
    echo "Moving $file to $RUNNER_DIR"
    cp -f "$SCRIPT_DIR/$file" $RUNNER_DIR
done

chown -R root "$RUNNER_DIR/"

# Create service

echo "Creating test runner service as $DAEMON_PATH"

cat > $DAEMON_PATH << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>net.mullvad.testrunner</string>

    <key>ProgramArguments</key>
    <array>
        <string>$RUNNER_DIR/test-runner</string>
        <string>/dev/tty.virtio</string>
        <string>serve</string>
    </array>

    <key>UserName</key>
    <string>root</string>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <true/>

    <key>StandardOutPath</key>
    <string>/tmp/runner.out</string>

    <key>StandardErrorPath</key>
    <string>/tmp/runner.err</string>
</dict>
</plist>
EOF

echo "Starting test runner service"

launchctl load -w $DAEMON_PATH
