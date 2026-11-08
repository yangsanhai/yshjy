#!/usr/bin/env python3
"""在指定 X 屏幕上模拟鼠标点击 / 移动（走 libXtst，不用装 xdotool）。

用法：
    python3 scripts/xclick.py <x> <y> [DISPLAY] [--move-only]
"""

import ctypes
import sys
import time

x, y = int(sys.argv[1]), int(sys.argv[2])
disp = ":99"
move_only = "--move-only" in sys.argv
if len(sys.argv) > 3 and sys.argv[3].startswith(":"):
    disp = sys.argv[3]

x11 = ctypes.CDLL("libX11.so.6")
xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(disp.encode())
if not d:
    sys.exit("打不开 DISPLAY " + disp)

xtst.XTestFakeMotionEvent(ctypes.c_void_p(d), -1, x, y, 0)
x11.XFlush(ctypes.c_void_p(d))
time.sleep(0.2)

if not move_only:
    xtst.XTestFakeButtonEvent(ctypes.c_void_p(d), 1, 1, 0)
    x11.XFlush(ctypes.c_void_p(d))
    time.sleep(0.07)
    xtst.XTestFakeButtonEvent(ctypes.c_void_p(d), 1, 0, 0)
    x11.XFlush(ctypes.c_void_p(d))

time.sleep(0.2)
print(f"clicked {x},{y} on {disp}")
