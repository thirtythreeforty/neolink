#! /bin/bash

installer -pkg "$(pwd)/gstreamer-runtime.pkg" -target "/"

sample_toml=/usr/local/share/neolink/sample_config.toml
live_toml=usr/local/etc/neolink.toml
if [ ! -e "${live_toml}" ]; then
  cp "${sample_toml}" "${live_toml}"
fi
