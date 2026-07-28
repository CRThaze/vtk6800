#!/bin/sh
# Runs after install and after removal of the vtk6800 package. Reloading udev
# makes the packaged rule (/usr/lib/udev/rules.d/70-vortex-pc68.rules) take
# effect for already-plugged keyboards without a replug. Best-effort: a missing
# udevadm (containers, minimal systems) is not an error.
set -e

if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules >/dev/null 2>&1 || true
    udevadm trigger >/dev/null 2>&1 || true
fi

exit 0
