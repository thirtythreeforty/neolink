#! /bin/bash

installer -pkg base-system.pkg -target "/"
installer -pkg gstreamer-core.pkg -target "/"
installer -pkg gstreamer-net.pkg -target "/"
installer -pkg gstreamer-codecs.pkg -target "/"
