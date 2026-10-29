#ifndef RUNNER_FLUTTER_WINDOW_H_
#define RUNNER_FLUTTER_WINDOW_H_

#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <memory>

#include "win32_window.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  explicit FlutterWindow(const flutter::DartProject& project);
  virtual ~FlutterWindow();

 protected:
  // Win32Window:
  bool OnCreate() override;
  void OnDestroy() override;
  LRESULT MessageHandler(HWND window, UINT const message, WPARAM const wparam,
                         LPARAM const lparam) noexcept override;

 private:
  // The project to run.
  flutter::DartProject project_;

  // The Flutter instance hosted by this window.
  std::unique_ptr<flutter::FlutterViewController> flutter_controller_;

  // "yshjy/tray" 通道（与 Linux 侧 my_application.cc 同一份协议）：
  // Dart → 原生：hide_window / set_status / notify / quit_now
  // 原生 → Dart：request_quit（托盘菜单点「退出」，由 Dart 收拾完引擎再 quit_now）
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> tray_channel_;
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
