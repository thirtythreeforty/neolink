# Reolink Push Notifications

This is the test crate to experiment with RE-ing the push notifications for a
camera

The goal here is to connect to Google's firebase cloud messaging system (FCM)
and recieve push notificaions for motion updates. This way we can get motion
notifications without being logged in to the camera and this will let us
disconnect from battery cameras and save power.

The following is the reolink data for google extracted from ios.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>AD_UNIT_ID_FOR_BANNER_TEST</key>
    <string>ca-app-pub-3940256099942544/2934735716</string>
    <key>AD_UNIT_ID_FOR_INTERSTITIAL_TEST</key>
    <string>ca-app-pub-3940256099942544/4411468910</string>
    <key>API_KEY</key>
    <string>AIzaSyAV5l5DbbAkYB5X61wgn81Lb2f7s1h-ANE</string>
    <key>BUNDLE_ID</key>
    <string>com.reo.link</string>
    <key>CLIENT_ID</key>
    <string>696841269229-mfqfluqdkb0s2raassbsul48h09l2eiv.apps.googleusercontent.com</string>
    <key>DATABASE_URL</key>
    <string>https://reolink-for-ios.firebaseio.com</string>
    <key>GCM_SENDER_ID</key>
    <string>696841269229</string>
    <key>GOOGLE_APP_ID</key>
    <string>1:696841269229:ios:690529b18debb2e3</string>
    <key>IS_ADS_ENABLED</key>
    <false/>
    <key>IS_ANALYTICS_ENABLED</key>
    <false/>
    <key>IS_APPINVITE_ENABLED</key>
    <false/>
    <key>IS_GCM_ENABLED</key>
    <false/>
    <key>IS_SIGNIN_ENABLED</key>
    <false/>
    <key>PLIST_VERSION</key>
    <string>1</string>
    <key>PROJECT_ID</key>
    <string>reolink-for-ios</string>
    <key>REVERSED_CLIENT_ID</key>
    <string>com.googleusercontent.apps.696841269229-mfqfluqdkb0s2raassbsul48h09l2eiv</string>
    <key>STORAGE_BUCKET</key>
    <string>reolink-for-ios.appspot.com</string>
</dict>
</plist>
```

Key information from this is

```xml
<key>GCM_SENDER_ID</key>
<string>696841269229</string>
```

Which we use to register for FCM

---

p.s. maybe some one can do something fun with the API
key not sure what thats doing there since anyone can
extract that.

```xml
<key>API_KEY</key>
<string>AIzaSyAV5l5DbbAkYB5X61wgn81Lb2f7s1h-ANE</string>
```

---

On android the details are little harder for me to extract (ios here). By
extracting the apk with apktook I found the following:

```xml
<string name="gcm_defaultSenderId">743639030586</string>
<string name="google_api_key">AIzaSyBEUIuWHnnOEwFahxWgQB4Yt4NsgOmkPyE</string>
<string name="google_app_id">1:743639030586:android:86f60a4fb7143876</string>
<string name="google_client_id">743639030586-ove5uqrausoqtjjkev4b1tk4vjdfpt4l.apps.googleusercontent.com</string>
```

Where the key we need is `743639030586`

To emulate the push recievier we use the `fcm-push-listener` crate. Which
is a rust implementation of the good
[RE work of Matthieu Lemoine.](https://medium.com/@MatthieuLemoine/my-journey-to-bring-web-push-support-to-node-and-electron-ce70eea1c0b0)
hats off to you.

We also need to send the PushInfo command to the camera with the token we get
from FCM and some client ID. On IOS the token is the APNS number.

The client ID is an all CAPS UID of hexadecimal of 33 chars. Not sure how
these are generated but they seem to be unique to the device, perhaps a MD5 of
something or some generated UID on app initial login.

To experiment I did a data wipe on the reolink app and found it generates a new UID.
I also found out that if you do this while you have Push Notifications turned on
then you cannot turn them off anymore. To disable the push notifications you need
the UID which I just wiped.... Fortunatly I saved before I wiped it so I could restore.
p.d. If you every wipe your reolink data and need to stop the push notifications
you can try doing manually using the API listed at the end.

This is the data of the `PushInfo` command that is sent to the camera:

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<body>
<PushInfo version="1.1">
<token>TOKEN_FROM_FCM_OR_APNS</token>
<phoneType>reo_iphone</phoneType>
<clientID>SOMEID</clientID>
</PushInfo>
</body>
```

For ios the phonetype is `reo_iphone` which I observed live but for android
no such luck.

First I just tried `reo_android` but the camera replied with an error code.
Which at least told me that I can test other string quickly and see if it
returns the error code.

Next thing to do was dumping the binaries from the apk and searching for a string
like `reo_`

```bash
grep -Ria  "reo_" .
./lib/armeabi-v7a/libavcodec.so:stereo_modeError splitting the input into NAL units.
./lib/arm64-v8a/libavcodec.so:stereo_modeError splitting the input into NAL units.
./smali_classes2/com/google/android/exoplayer2/extractor/mkv/MatroskaExtractor.smali:.field private static final ID_STEREO_MODE:I = 0x53b8
./smali_classes2/com/google/android/exoplayer2/C.smali:.field public static final STEREO_MODE_LEFT_RIGHT:I = 0x2
./smali_classes2/com/google/android/exoplayer2/C.smali:.field public static final STEREO_MODE_MONO:I = 0x0
./smali_classes2/com/google/android/exoplayer2/C.smali:.field public static final STEREO_MODE_STEREO_MESH:I = 0x3
./smali_classes2/com/google/android/exoplayer2/C.smali:.field public static final STEREO_MODE_TOP_BOTTOM:I = 0x1
./smali_classes2/com/android/bc/util/DeviceIdUtil$UpdatePushClientIdRequest.smali:    const-string v2, "reo_fcm"
./smali_classes2/com/android/bc/pushmanager/MyPushAdapterImpl.smali:    const-string v0, "reo_fcm"
./smali/androidx/core/text/BidiFormatter.smali:.field private static final FLAG_STEREO_RESET:I = 0x2
./smali/com/android/bc/component/BaseWebViewFragment.smali:    const-string v2, "REO_LANGUAGE="
```

The interesting one was `MyPushAdapterImpl.smali:    const-string v0, "reo_fcm"`
which suggested that it is `reo_fcm` stuck that in and voilla
no error messages from the camera.

Now to compile the test app with all this info
wait patiently for a motion event and...

viola!

Reolink was kinda enough to send this to my test app:

```json
{
    "data": {
        "SRVTIME": "2023-05-19T08:07:03.566Z",
        "ALMNAME": "Motion Alert from Cam01",
        "sound": "push.wav",
        "CHNAME": "Cam0",
        "messageType": "alarm",
        "DEVNAME": "Cam01",
        "pushVersion": "v1.1",
        "UID": "REDACTED",
        "ALMTYPE": "MD",
        "ALMCHN": "1",
        "alert": "Motion Alert from Cam01",
        "title": "Camera Alert"
    },
    "from": "743639030586",
    "priority": "normal",
    "notification": { "title": "Camera Alert", "body": "Motion Alert from Cam01" },
    "fcmMessageId": "REDACTED"
}
```

There's no push notifiation for motion stop event but I can wait for this message
then login and use normal motion detection to tell when it stops.

Yay.... now to just add all this to actual neolink so we can save battery.....

---

While experimenting with the ios app, in addition to observing all the
data send to the camera, I also looked at all https calls the app was making
(mitmproxy). This showed a set of APIs that could be useful. Including the api
for deleting a push notification.

---

To disable a PUSH request the following is done:

```bash
curl -v -X DELETE 'https://pushx.reolink.com/devices/<CAMERA_UID>/notifications/listeners/<clientID_FROM_PushInfo>' -H 'Cookie:'
```

Reply is:

```json
{
  "status" : {
    "detail" : "OK",
    "code" : 0
  }
}
```

Nothing seems to be sent to the camera just this to the reolink api

---

On ios application start reolink send this:

```bash
curl -v -X GET 'https://pushx.reolink.com/listeners/<clientID_FROM_PushInfo>/devices' -H 'Accept-Encoding: gzip, deflate, br' -H 'Accept: */*' -H 'Accept-Language: en-GB,en;q=0.9' -H 'Cookie:'
```

Which resonds with this json which contains a list of all currently
enabled push notification camera UIDs for this client

```json
{
  "status" : {
    "detail" : "OK",
    "code" : 0
  },
  "data" : [
    {
      "uid" : "<CAMERA_UID>",
      "secret" : ""
    }
  ]
}
```

---

You can also confirm a device has a listener with

```bash
curl -v -X GET 'https://pushx.reolink.com/devices/<CAMERA_UID>/listeners/<clientID_FROM_PushInfo>/config' -H 'Cookie:'
```

Which returns

```json
{
  "status": {
    "code":0,
    "detail":"OK"
  },
  "data": {
    "sound": {
      "soundType":0,
      "soundName":"",
      "soundVolume":0
    }
  }
}
```

---

There's also a profile one that the app does when a new camera is added
but I forgot to save the details of it.

It reports all the capabilities of the camera, such as it's model
and its resolution etc. Could be useful to test if a UID is valid
quickly

--

Checked it out again and found that the profile api is:

```bash
curl -v -X GET 'https://apis.reolink.com/v1.0/devices/<CAMERA_UID>/profile' -H 'Cookie: REO_LANGUAGE=;'
```

Which returns something like this:

```json
{
  "uid":"CAMERA_UID",
  "model":"CAMERA_MODEL e.g. E1",
  "board":"CAMERA_BOARD_MODEL_ID",
  "type":"IPC",
  "hwVersion":"LINUX_V1",
  "p2p": {
    "enabled":true,
    activated":true,
    activatedAt": SECONDS_SINCE_EPOCH,
    "firstUsedAt":0
  },
  "hwFeatures":{
    "ethInterface":0,
    cellNetwork":0,
    reproduced":0
  },
  "qrScan":{
    "mode":1,
    "distance":20,
    "languages":["en-us"],
    "showCountryCode":1,
    "attributes":1
  },
  "batteryType":0,
  "wifiType":0,
  "maxChannels":0
}
```
