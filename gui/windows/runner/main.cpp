#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <flutter_windows.h>
#include <windows.h>

#include <algorithm>

#include "flutter_window.h"
#include "utils.h"

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);
  // 初始窗口按「工作区」算，不写死 1280x720：
  // 150% 缩放下 1280 逻辑像素正好铺满整块屏，一开就像全屏 —— 小工具不该这样。
  // 取工作区 92% 以内、最多 1100x760 逻辑像素，然后居中。
  RECT work{};
  ::SystemParametersInfoW(SPI_GETWORKAREA, 0, &work, 0);
  // DPI 从「窗口要落的那块屏」取，和 Win32Window::Create 里的折算口径一致；
  // 不能用 GetDpiForSystem —— 进程还没建窗口时它给的是虚拟化的 96。
  const POINT anchor{work.left + (work.right - work.left) / 2,
                     work.top + (work.bottom - work.top) / 2};
  HMONITOR monitor = ::MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
  const double scale = ::FlutterDesktopGetDpiForMonitor(monitor) / 96.0;
  const int work_w = static_cast<int>((work.right - work.left) / scale);
  const int work_h = static_cast<int>((work.bottom - work.top) / scale);
  const int width = std::max(640, std::min(1100, static_cast<int>(work_w * 0.92)));
  const int height = std::max(480, std::min(760, static_cast<int>(work_h * 0.92)));
  const int x = static_cast<int>(work.left / scale) + (work_w - width) / 2;
  const int y = static_cast<int>(work.top / scale) + (work_h - height) / 2;
  if (!window.Create(L"杨三海解压", Win32Window::Point(x, y),
                     Win32Window::Size(width, height))) {
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  return EXIT_SUCCESS;
}
