cd /d %~dp0
open http://127.0.0.1:23333
$( readlink -f -- "$0" )/koharu -b 127.0.0.1:23333

