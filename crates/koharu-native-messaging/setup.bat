@echo off
echo Registering Koharu Native Messaging Host with Chrome...
REG ADD "HKCU\Software\Google\Chrome\NativeMessagingHosts\com.koharu.native_host" /ve /t REG_SZ /d "D:\koharu\crates\koharu-native-messaging\com.koharu.native_host.json" /f
echo Done.
pause
