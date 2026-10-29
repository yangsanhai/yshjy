#include "flutter_window.h"

#include <optional>

#include "flutter/generated_plugin_registrant.h"
#include "utils.h"
#include "resource.h"

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  // ── "yshjy/tray" 通道（W9）：与 Linux 侧 my_application.cc 同一份协议 ──
  tray_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), "yshjy/tray",
          &flutter::StandardMethodCodec::GetInstance());
  tray_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
       std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        if (call.method_name() == "hide_window") {
          HideToTray();
          result->Success();
        } else if (call.method_name() == "set_status") {
          const auto* args = std::get_if<flutter::EncodableMap>(call.arguments());
          if (args) {
            auto it = args->find(flutter::EncodableValue(std::string("text")));
            if (it != args->end()) {
              if (const auto* text = std::get_if<std::string>(&it->second)) {
                SetTrayStatus(Utf8ToWide(*text));
              }
            }
          }
          result->Success();
        } else if (call.method_name() == "notify") {
          const auto* args = std::get_if<flutter::EncodableMap>(call.arguments());
          std::string title, body;
          if (args) {
            auto t = args->find(flutter::EncodableValue(std::string("title")));
            auto b = args->find(flutter::EncodableValue(std::string("body")));
            if (t != args->end()) {
              if (const auto* v = std::get_if<std::string>(&t->second)) {
                title = *v;
              }
            }
            if (b != args->end()) {
              if (const auto* v = std::get_if<std::string>(&b->second)) {
                body = *v;
              }
            }
          }
          ShowTrayBalloon(Utf8ToWide(title), Utf8ToWide(body));
          result->Success();
        } else if (call.method_name() == "quit_now") {
          // Dart 已取消引擎、收拾干净（见 tray.dart 的说明），这里真退
          QuitFromTray();
          result->Success();
        } else {
          result->NotImplemented();
        }
      });
  // 托盘菜单点「退出」→ 绕回 Dart 取消引擎 → Dart 再调 quit_now
  SetQuitRequestCallback([this]() {
    if (tray_channel_) {
      tray_channel_->InvokeMethod("request_quit", nullptr, nullptr);
    }
  });

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  // 先撤通道再拆 Flutter 引擎：挂着 handler 的通道指向 this，悬空就是崩
  if (tray_channel_) {
    tray_channel_->SetMethodCallHandler(nullptr);
    tray_channel_ = nullptr;
  }
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
