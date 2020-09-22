#! /bin/bash


## Install gstreamer dependency
install_gstreamer=0
gstr_plist="/Library/Frameworks/GStreamer.framework/Versions/1.0/Resources/Info.plist"
if [ ! -e "${gstr_plist}" ]; then
  install_gstreamer=1
elif [ "$(defaults read "${gstr_plist}" CFBundleShortVersionString)" -lt 1162 ]; then
  install_gstreamer=1
fi

if [ "${install_gstreamer}" -eq 1 ]; then
  installer -pkg "$(pwd)/gstreamer-runtime.pkg" -target "/"
fi

## Install sample toml if one isn't there yet
sample_toml=/usr/local/share/neolink/sample_config.toml
live_toml=usr/local/etc/neolink.toml
if [ ! -e "${live_toml}" ]; then
  cp "${sample_toml}" "${live_toml}"
fi
