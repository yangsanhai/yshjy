#ifndef RUNNER_WIN32_WINDOW_H_
#define RUNNER_WIN32_WINDOW_H_

#include <windows.h>

#include <functional>
#include <memory>
#include <string>

// A class abstraction for a high DPI-aware Win32 Window. Intended to be
// inherited from by classes that wish to specialize with custom
// rendering and input handling
class Win32Window {
 public:
  struct Point {
    unsigned int x;
    unsigned int y;
    Point(unsigned int x, unsigned int y) : x(x), y(y) {}
  };

  struct Size {
    unsigned int width;
    unsigned int height;
    Size(unsigned int width, unsigned int height)
        : width(width), height(height) {}
  };

  Win32Window();
  virtual ~Win32Window();

  // Creates a win32 window with |title| that is positioned and sized using
  // |origin| and |size|. New windows are created on the default monitor. Window
  // sizes are specified to the OS in physical pixels, hence to ensure a
  // consistent size this function will scale the inputted width and height as
  // as appropriate for the default monitor. The window is invisible until
  // |Show| is called. Returns true if the window was created successfully.
  bool Create(const std::wstring& title, const Point& origin, const Size& size);

  // Show the current window. Returns true if the window was successfully shown.
  bool Show();

  // Release OS resources associated with window.
  void Destroy();

  // Inserts |content| into the window tree.
  void SetChildContent(HWND content);

  // Returns the backing Window handle to enable clients to set icon and other
  // window properties. Returns nullptr if the window has been destroyed.
  HWND GetHandle();

  // If true, closing this window will quit the application.
  void SetQuitOnClose(bool quit_on_close);

  // Return a RECT representing the bounds of the current client area.
  RECT GetClientArea();

  // ── 托盘（W9）─────────────────────────────────────────────────────
  // 与 Linux 侧 gui/linux/runner/my_application.cc 的 "yshjy/tray" 通道同一套协议：
  // Dart 决定「藏不藏 / 退不退」，原生只负责画图标、收鼠标、传话。

  // 把窗口藏进托盘（进程、引擎照跑）。首次调用会顺带把图标挂上去。
  void HideToTray();

  // 托盘气球通知（解压完成 / 失败提醒用；窗口可见时也照发）。
  void ShowTrayBalloon(const std::wstring& title, const std::wstring& body);

  // 同步托盘悬停提示里的状态行（节流过的进度文案）。
  void SetTrayStatus(const std::wstring& text);

  // Dart 已把引擎收拾干净，原生这里真退（摘图标 → PostQuitMessage）。
  void QuitFromTray();

  // 托盘菜单点「退出」时回话给 Dart（由 Dart 取消引擎后再调 QuitFromTray）。
  // 为什么绕这一圈：引擎是子进程，原生单方面退会让引擎读 stdin 撞 EOF 硬中断，
  // 和 Linux 侧 request_quit → quit_now 的两段式是同一个道理。
  void SetQuitRequestCallback(std::function<void()> cb);

 protected:
  // Processes and route salient window messages for mouse handling,
  // size change and DPI. Delegates handling of these to member overloads that
  // inheriting classes can handle.
  virtual LRESULT MessageHandler(HWND window,
                                 UINT const message,
                                 WPARAM const wparam,
                                 LPARAM const lparam) noexcept;

  // Called when CreateAndShow is called, allowing subclass window-related
  // setup. Subclasses should return false if setup fails.
  virtual bool OnCreate();

  // Called when Destroy is called.
  virtual void OnDestroy();

 private:
  friend class WindowClassRegistrar;

  // OS callback called by message pump. Handles the WM_NCCREATE message which
  // is passed when the non-client area is being created and enables automatic
  // non-client DPI scaling so that the non-client area automatically
  // responds to changes in DPI. All other messages are handled by
  // MessageHandler.
  static LRESULT CALLBACK WndProc(HWND const window,
                                  UINT const message,
                                  WPARAM const wparam,
                                  LPARAM const lparam) noexcept;

  // Retrieves a class instance pointer for |window|
  static Win32Window* GetThisFromHandle(HWND const window) noexcept;

  // Update the window frame's theme to match the system theme.
  static void UpdateTheme(HWND const window);

  bool quit_on_close_ = false;

  // window handle for top level window.
  HWND window_handle_ = nullptr;

  // ── 托盘状态（W9）──
  // Shell_NotifyIcon 的回传消息号：WM_APP 起，避开系统保留区间
  static constexpr UINT kTrayCallbackMsg = WM_APP + 1;
  // 托盘菜单项 ID（TrackPopupMenu TPM_RETURNCMD 的返回值判断用）
  static constexpr int kMenuRestore = 1;
  static constexpr int kMenuQuit = 2;
  NOTIFYICONDATAW tray_{};
  bool tray_added_ = false;
  std::wstring tray_status_;
  std::function<void()> quit_request_callback_;

  void EnsureTrayIcon();
  void RemoveTrayIcon();
  void RestoreFromTray();
  void ShowTrayMenu();

  // window handle for hosted content.
  HWND child_content_ = nullptr;
};

#endif  // RUNNER_WIN32_WINDOW_H_
