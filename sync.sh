#!/usr/bin/env bash
while true; do
    newsboat -x reload 2>/dev/null
    sleep 1800
done
