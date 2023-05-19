# Reolink Push Notifications

This is the test crate to experient with REing the push notifications for a
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

On andriod the details are little harder for me to extract (ios here). By
extracing the apk with apktook I found the following:

```xml
<string name="gcm_defaultSenderId">743639030586</string>
<string name="google_api_key">AIzaSyBEUIuWHnnOEwFahxWgQB4Yt4NsgOmkPyE</string>
<string name="google_app_id">1:743639030586:android:86f60a4fb7143876</string>
<string name="google_client_id">743639030586-ove5uqrausoqtjjkev4b1tk4vjdfpt4l.apps.googleusercontent.com</string>
```

Where the key we need is `743639030586`

We also need to send the PushInfo command to the camera with the token we get
from FCM and some client ID. On IOS the token is the APNS number.

The client ID is an all CAPS UID of hexadecimal of 33 chars. Not sure how
these are generated but they seem to be unique to the device, perhaps a MD5 of
something or some generated UID on app initial login. On data wipe the app
generates a new UID

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

For ios the phonetype is `reo_iphone` which I observed live but for andriod
no such luck.

First I just tried `reo_andriod` but the camera replied with an error code.
Which at least told me that I can test other string quickly by seeing if it
returns the error code.

Next attempt was dumping the binaries from the apk and searching for a string
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

Interesting one was `MyPushAdapterImpl.smali:    const-string v0, "reo_fcm"`
which suggested that it is `reo_fcm`

stuck that in and voilla no error messages from the camera.

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

We can use the
`https://pushx.reolink.com/listeners/<clientID_FROM_PushInfo>/devices`
api to confirm if our client is regeristed for push notifications.
Which was the next thing to do once we passed the token onto the camera

---

On motion reolink will send the following json

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
