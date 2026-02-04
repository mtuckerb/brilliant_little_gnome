#!/bin/bash
cd /Users/mtuckerb/workspace/mtuckerb/brilliant_little_gnome

# Use system ruby (mise shim)
RUBY_CMD="ruby"

# Standard Sync
$RUBY_CMD bin/rake sync >> logs/cron_sync.log 2>&1

# PSY-220 Specialized Browser Sync (to catch ungraded/bonus items)
# This uses the cookies from config/connection.json. If they expire, this will fail gracefully.
$RUBY_CMD bin/psy220_browser_sync.rb >> logs/psy220_sync.log 2>&1
