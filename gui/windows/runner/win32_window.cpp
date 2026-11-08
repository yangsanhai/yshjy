#include "win32_window.h"

#include <shellapi.h>

#include "resource.h"

#include <dwmapi.h>
#include <flutter_windows.h>

#include "resource.h"

namespace {

/// Window attribute that enables dark mode window decorations.
///
/// Redefined in case the developer's machine has a Windows SDK older than
/// version 10.0.22000.0.
/// See: https://docs.microsoft.com/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
#ifndef DWMWA_USE_IMMERSIVE_DARK_MODE
#define DWMWA_USE_IMMERSIVE_DARK_MODE 20
#endif

constexpr const wchar_t kWindowClassName[] = L"FLUTTER_RUNNER_WIN32_WINDOW";

/// Registry key for app theme preference.
///
/// A value of 0 indicates apps should use dark mode. A non-zero or missing
/// value indicates apps should use light mode.
constexpr const wchar_t kGetPreferredBrightnessRegKey[] =
  L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
constexpr const wchar_t kGetPreferredBrightnessRegValue[] = L"AppsUseLightTheme";

// The number of Win32Window objects that currently exist.
static int g_active_window_count = 0;

using EnableNonClientDpiScaling = BOOL __stdcall(HWND hwnd);

// Scale helper to convert logical scaler values to physical using passed in
// scale factor
int Scale(int source, double scale_factor) {
  return static_cast<int>(source * scale_factor);
}

// Dynamically loads the |EnableNonClientDpiScaling| from the User32 module.
// This API is only needed for PerMonitor V1 awareness mode.
void EnableFullDpiSupportIfAvailable(HWND hwnd) {
  HMODULE user32_module = LoadLibraryA("User32.dll");
  if (!user32_module) {
    return;
  }
  auto enable_non_client_dpi_scaling =
      reinterpret_cast<EnableNonClientDpiScaling*>(
          GetProcAddress(user32_module, "EnableNonClientDpiScaling"));
  if (enable_non_client_dpi_scaling != nullptr) {
    enable_non_client_dpi_scaling(hwnd);
  }
  FreeLibrary(user32_module);
}

}  // namespace

// Manages the Win32Window's window class registration.
class WindowClassRegistrar {
 public:
  ~WindowClassRegistrar() = default;

  // Returns the singleton registrar instance.
  static WindowClassRegistrar* GetInstance() {
    if (!instance_) {
      instance_ = new WindowClassRegistrar();
    }
    return instance_;
  }

  // Returns the name of the window class, registering the class if it hasn't
  // previously been registered.
  const wchar_t* GetWindowClass();

  // Unregisters the window class. Should only be called if there are no
  // instances of the window.
  void UnregisterWindowClass();

 private:
  WindowClassRegistrar() = default;

  static WindowClassRegistrar* instance_;

  bool class_registered_ = false;
};

WindowClassRegistrar* WindowClassRegistrar::instance_ = nullptr;

const wchar_t* WindowClassRegistrar::GetWindowClass() {
  if (!class_registered_) {
    WNDCLASS window_class{};
    window_class.hCursor = LoadCursor(nullptr, IDC_ARROW);
    window_class.lpszClassName = kWindowClassName;
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.cbClsExtra = 0;
    window_class.cbWndExtra = 0;
    window_class.hInstance = GetModuleHandle(nullptr);
    window_class.hIcon =
        LoadIcon(window_class.hInstance, MAKEINTRESOURCE(IDI_APP_ICON));
    window_class.hbrBackground = 0;
    window_class.lpszMenuName = nullptr;
    window_class.lpfnWndProc = Win32Window::WndProc;
    RegisterClass(&window_class);
    class_registered_ = true;
  }
  return kWindowClassName;
}

void WindowClassRegistrar::UnregisterWindowClass() {
  UnregisterClass(kWindowClassName, nullptr);
  class_registered_ = false;
}

Win32Window::Win32Window() {
  ++g_active_window_count;
}

Win32Window::~Win32Window() {
  --g_active_window_count;
  Destroy();
}

bool Win32Window::Create(const std::wstring& title,
                         const Point& origin,
                         const Size& size) {
  Destroy();

  const wchar_t* window_class =
      WindowClassRegistrar::GetInstance()->GetWindowClass();

  const POINT target_point = {static_cast<LONG>(origin.x),
                              static_cast<LONG>(origin.y)};
  HMONITOR monitor = MonitorFromPoint(target_point, MONITOR_DEFAULTTONEAREST);
  UINT dpi = FlutterDesktopGetDpiForMonitor(monitor);
  double scale_factor = dpi / 96.0;

  HWND window = CreateWindow(
      window_class, title.c_str(), WS_OVERLAPPEDWINDOW,
      Scale(origin.x, scale_factor), Scale(origin.y, scale_factor),
      Scale(size.width, scale_factor), Scale(size.height, scale_factor),
      nullptr, nullptr, GetModuleHandle(nullptr), this);

  if (!window) {
    return false;
  }

  UpdateTheme(window);

  return OnCreate();
}

bool Win32Window::Show() {
  return ShowWindow(window_handle_, SW_SHOWNORMAL);
}

// static
LRESULT CALLBACK Win32Window::WndProc(HWND const window,
                                      UINT const message,
                                      WPARAM const wparam,
                                      LPARAM const lparam) noexcept {
  if (message == WM_NCCREATE) {
    auto window_struct = reinterpret_cast<CREATESTRUCT*>(lparam);
    SetWindowLongPtr(window, GWLP_USERDATA,
                     reinterpret_cast<LONG_PTR>(window_struct->lpCreateParams));

    auto that = static_cast<Win32Window*>(window_struct->lpCreateParams);
    EnableFullDpiSupportIfAvailable(window);
    that->window_handle_ = window;
  } else if (Win32Window* that = GetThisFromHandle(window)) {
    return that->MessageHandler(window, message, wparam, lparam);
  }

  return DefWindowProc(window, message, wparam, lparam);
}

LRESULT
Win32Window::MessageHandler(HWND hwnd,
                            UINT const message,
                            WPARAM const wparam,
                            LPARAM const lparam) noexcept {
  switch (message) {
    case kTrayCallbackMsg:
      // 经典版 NOTIFYICONDATA：lparam 就是鼠标消息
      if (lparam == WM_LBUTTONUP || lparam == WM_LBUTTONDBLCLK) {
        RestoreFromTray();
      } else if (lparam == WM_RBUTTONUP || lparam == WM_CONTEXTMENU) {
        ShowTrayMenu();
      }
      return 0;

    case WM_DESTROY:
      // 兜底摘图标：正常路径 quit_now 已经摘过，重复 NIM_DELETE 无害
      RemoveTrayIcon();
      window_handle_ = nullptr;
      Destroy();
      if (quit_on_close_) {
        PostQuitMessage(0);
      }
      return 0;

    case WM_DPICHANGED: {
      auto newRectSize = reinterpret_cast<RECT*>(lparam);
      LONG newWidth = newRectSize->right - newRectSize->left;
      LONG newHeight = newRectSize->bottom - newRectSize->top;

      SetWindowPos(hwnd, nullptr, newRectSize->left, newRectSize->top, newWidth,
                   newHeight, SWP_NOZORDER | SWP_NOACTIVATE);

      return 0;
    }
    case WM_GETMINMAXINFO: {
      // 界面按「小工具」排的，再窄就挤成一团：给个下限。
      // 逻辑 760x560，按窗口所在屏的 DPI 折算成物理像素。
      auto* info = reinterpret_cast<MINMAXINFO*>(lparam);
      const UINT dpi = ::GetDpiForWindow(hwnd);
      const double scale = dpi / 96.0;
      info->ptMinTrackSize.x = static_cast<LONG>(760 * scale);
      info->ptMinTrackSize.y = static_cast<LONG>(560 * scale);
      return 0;
    }
    case WM_SIZE: {
      RECT rect = GetClientArea();
      if (child_content_ != nullptr) {
        // Size and position the child window.
        MoveWindow(child_content_, rect.left, rect.top, rect.right - rect.left,
                   rect.bottom - rect.top, TRUE);
      }
      return 0;
    }

    case WM_ACTIVATE:
      if (child_content_ != nullptr) {
        SetFocus(child_content_);
      }
      return 0;

    case WM_DWMCOLORIZATIONCOLORCHANGED:
      UpdateTheme(hwnd);
      return 0;
  }

  return DefWindowProc(window_handle_, message, wparam, lparam);
}

void Win32Window::Destroy() {
  OnDestroy();

  if (window_handle_) {
    DestroyWindow(window_handle_);
    window_handle_ = nullptr;
  }
  if (g_active_window_count == 0) {
    WindowClassRegistrar::GetInstance()->UnregisterWindowClass();
  }
}

Win32Window* Win32Window::GetThisFromHandle(HWND const window) noexcept {
  return reinterpret_cast<Win32Window*>(
      GetWindowLongPtr(window, GWLP_USERDATA));
}

void Win32Window::SetChildContent(HWND content) {
  child_content_ = content;
  SetParent(content, window_handle_);
  RECT frame = GetClientArea();

  MoveWindow(content, frame.left, frame.top, frame.right - frame.left,
             frame.bottom - frame.top, true);

  SetFocus(child_content_);
}

RECT Win32Window::GetClientArea() {
  RECT frame;
  GetClientRect(window_handle_, &frame);
  return frame;
}

HWND Win32Window::GetHandle() {
  return window_handle_;
}

void Win32Window::SetQuitOnClose(bool quit_on_close) {
  quit_on_close_ = quit_on_close;
}

// ── 托盘（W9）─────────────────────────────────────────────────────────
//
// 为什么自己写而不是找插件：协议只有五个动作（藏窗口/改提示/气球/请求退出/真退），
// Shell_NotifyIcon 一次注册就能全包，拉一个插件包反而多一份供应链与构建成本。
// 与 Linux 侧（GTK appindicator）保持同一份 Dart 可见行为。

void Win32Window::EnsureTrayIcon() {
  if (tray_added_ || !window_handle_) {
    return;
  }
  ZeroMemory(&tray_, sizeof(tray_));
  tray_.cbSize = sizeof(tray_);
  tray_.hWnd = window_handle_;
  tray_.uID = 1;
  tray_.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
  tray_.uCallbackMessage = kTrayCallbackMsg;
  // 图标用应用自己的（Runner.rc 的 IDI_APP_ICON），资源管理器/任务栏一眼认得出
  tray_.hIcon = LoadIconW(GetModuleHandleW(nullptr), MAKEINTRESOURCEW(IDI_APP_ICON));
  std::wstring tip = L"杨三海解压";
  if (!tray_status_.empty()) {
    tip += L"\n" + tray_status_;
  }
  lstrcpynW(tray_.szTip, tip.c_str(), ARRAYSIZE(tray_.szTip));
  if (!Shell_NotifyIconW(NIM_ADD, &tray_)) {
    // 挂不上（极端情况：资源没了）就当没有托盘 —— 藏窗口降级成普通最小化都没做，
    // 只是窗口真的藏了，用户还能从任务栏预览里找回（SW_HIDE 后任务栏无预览，
    // 但进程仍在，界面挂了时用户杀进程即可）—— 通知主流程不受影响
    return;
  }
  tray_added_ = true;
}

void Win32Window::RemoveTrayIcon() {
  if (!tray_added_) {
    return;
  }
  Shell_NotifyIconW(NIM_DELETE, &tray_);
  tray_added_ = false;
}

void Win32Window::HideToTray() {
  if (!window_handle_) {
    return;
  }
  EnsureTrayIcon();
  ShowWindow(window_handle_, SW_HIDE);
}

void Win32Window::RestoreFromTray() {
  if (!window_handle_) {
    return;
  }
  ShowWindow(window_handle_, SW_SHOW);
  // 托盘图标点回来不给前台，用户会以为「点了没反应」
  SetForegroundWindow(window_handle_);
}

void Win32Window::SetTrayStatus(const std::wstring& text) {
  tray_status_ = text;
  if (!tray_added_) {
    return;
  }
  std::wstring tip = L"杨三海解压";
  if (!tray_status_.empty()) {
    tip += L"\n" + tray_status_;
  }
  lstrcpynW(tray_.szTip, tip.c_str(), ARRAYSIZE(tray_.szTip));
  tray_.uFlags = NIF_TIP;
  Shell_NotifyIconW(NIM_MODIFY, &tray_);
}

void Win32Window::ShowTrayBalloon(const std::wstring& title,
                                  const std::wstring& body) {
  EnsureTrayIcon();
  if (!tray_added_) {
    return;
  }
  tray_.uFlags = NIF_INFO;
  lstrcpynW(tray_.szInfoTitle, title.c_str(), ARRAYSIZE(tray_.szInfoTitle));
  lstrcpynW(tray_.szInfo, body.c_str(), ARRAYSIZE(tray_.szInfo));
  tray_.dwInfoFlags = NIIF_INFO;
  Shell_NotifyIconW(NIM_MODIFY, &tray_);
}

void Win32Window::ShowTrayMenu() {
  if (!window_handle_) {
    return;
  }
  HMENU menu = CreatePopupMenu();
  if (!menu) {
    return;
  }
  AppendMenuW(menu, MF_STRING, kMenuRestore, L"显示窗口");
  AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
  AppendMenuW(menu, MF_STRING, kMenuQuit, L"退出");
  POINT pt;
  GetCursorPos(&pt);
  // TrackPopupMenu 前必须 SetForegroundWindow，否则菜单点外不消失（经典坑）
  SetForegroundWindow(window_handle_);
  int picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y,
                              0, window_handle_, nullptr);
  DestroyMenu(menu);
  if (picked == kMenuRestore) {
    RestoreFromTray();
  } else if (picked == kMenuQuit && quit_request_callback_) {
    quit_request_callback_();
  }
}

void Win32Window::QuitFromTray() {
  RemoveTrayIcon();
  PostQuitMessage(0);
}

void Win32Window::SetQuitRequestCallback(std::function<void()> cb) {
  quit_request_callback_ = std::move(cb);
}

bool Win32Window::OnCreate() {
  // No-op; provided for subclasses.
  return true;
}

void Win32Window::OnDestroy() {
  // No-op; provided for subclasses.
}

void Win32Window::UpdateTheme(HWND const window) {
  DWORD light_mode;
  DWORD light_mode_size = sizeof(light_mode);
  LSTATUS result = RegGetValue(HKEY_CURRENT_USER, kGetPreferredBrightnessRegKey,
                               kGetPreferredBrightnessRegValue,
                               RRF_RT_REG_DWORD, nullptr, &light_mode,
                               &light_mode_size);

  if (result == ERROR_SUCCESS) {
    BOOL enable_dark_mode = light_mode == 0;
    DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE,
                          &enable_dark_mode, sizeof(enable_dark_mode));
  }
}
