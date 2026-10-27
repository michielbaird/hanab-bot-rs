#!/bin/sh
# Runs the given command as the user and group in PUID / PGID (default 10001), so files written to the
# mounted /data volume belong to whoever owns it on the host (e.g. on a NAS).
#
# If the container is started as a non-root user already (docker run --user, compose `user:`, or an
# orchestrator that sets it), the command is simply run as that user and /data must be writable by it.
set -eu

if [ "$#" -eq 0 ]; then
    echo "No command given. Pass the program and its arguments, for example:" >&2
    echo "    bot index=0,1,2" >&2
    echo "Programs: bot, analyze_history, analyze, replay, self_play." >&2
    exit 64
fi

if [ "$(id -u)" = "0" ]; then
    PUID="${PUID:-10001}"
    PGID="${PGID:-10001}"

    case "$PUID$PGID" in
        "" | *[!0-9]*)
            echo "PUID and PGID must be numbers (got PUID=$PUID PGID=$PGID)." >&2
            exit 64
            ;;
    esac
    if [ "$PUID" = "0" ]; then
        echo "warning: PUID=0, the command will run as root." >&2
    fi

    mkdir -p /data
    chown -R "$PUID:$PGID" /data || echo "warning: could not change the owner of /data to $PUID:$PGID." >&2

    export HOME=/data
    exec setpriv --reuid="$PUID" --regid="$PGID" --clear-groups "$@"
fi

exec "$@"
