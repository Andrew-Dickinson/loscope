#!/bin/sh
# Builds the nginx upstream list at container start, so the frontend works
# whether backend-2 is running or was scaled to 0
# (`docker compose up --scale backend-2=0`).
#
# Set BACKEND_COUNT=1 or BACKEND_COUNT=2 to skip auto-detection and pick the
# upstream list directly (useful to avoid the probe's startup delay when you
# already know how many backends you're running). Leave it unset to
# auto-detect whether backend-2 is up.
set -e

MAX_ATTEMPTS=30

backend_up() {
    wget -q -T 2 -O /dev/null "http://$1:8000/api/healthcheck" 2>/dev/null
}

write_upstream() {
    include_backend_2="$1"
    {
        echo "upstream backend_pool {"
        echo "    server backend-1:8000;"
        [ "$include_backend_2" = "1" ] && echo "    server backend-2:8000;"
        echo "}"
    } > /etc/nginx/conf.d/upstream.conf
}

case "$BACKEND_COUNT" in
    1)
        write_upstream 0
        ;;
    2)
        write_upstream 1
        ;;
    "")
        attempt=0
        while [ "$attempt" -lt "$MAX_ATTEMPTS" ] && ! backend_up backend-2; do
            attempt=$((attempt + 1))
            sleep 1
        done
        if backend_up backend-2; then
            write_upstream 1
        else
            write_upstream 0
        fi
        ;;
    *)
        echo "BACKEND_COUNT must be 1, 2, or unset (got '$BACKEND_COUNT')" >&2
        exit 1
        ;;
esac

echo "Generated /etc/nginx/conf.d/upstream.conf:"
cat /etc/nginx/conf.d/upstream.conf

exec nginx -g 'daemon off;'
