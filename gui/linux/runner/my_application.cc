#include "my_application.h"

#include <flutter_linux/flutter_linux.h>
#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif

#include "flutter/generated_plugin_registrant.h"

struct _MyApplication {
  GtkApplication parent_instance;
  char** dart_entrypoint_arguments;
};

G_DEFINE_TYPE(MyApplication, my_application, GTK_TYPE_APPLICATION)

// Called when first Flutter frame received.
static void first_frame_cb(MyApplication* self, FlView* view) {
  gtk_widget_show(gtk_widget_get_toplevel(GTK_WIDGET(view)));
}

// ── 标题栏按钮：最小化 / 最大化（还原）─────────────────────────────────────
//
// 为什么得手写：GTK3 的 `GtkHeaderBar` 只有 `set_show_close_button()`，
// 它**不读** `org.gnome.desktop.wm.preferences button-layout`，也不像 GTK4 /
// libadwaita 那样自带最小化、最大化按钮。所以走 CSD（client-side decoration）
// 时窗口就只剩一个关闭按钮 —— 而 Chrome / Qt 那些自己画标题栏的应用是三按钮，
// 看起来就不一致。这里补上两个按钮，图标随窗口状态切换。
static void on_maximize_clicked(GtkWindow* window) {
  if (gtk_window_is_maximized(window)) {
    gtk_window_unmaximize(window);
  } else {
    gtk_window_maximize(window);
  }
}

static gboolean on_window_state_changed(GtkWidget* widget,
                                        GdkEventWindowState* event,
                                        gpointer user_data) {
  (void)event;
  GtkButton* button = GTK_BUTTON(user_data);
  gboolean maximized = gtk_window_is_maximized(GTK_WINDOW(widget));
  gtk_button_set_image(
      button,
      gtk_image_new_from_icon_name(
          maximized ? "window-restore-symbolic" : "window-maximize-symbolic",
          GTK_ICON_SIZE_MENU));
  return FALSE;
}

// ── 窗口尺寸记忆（P10）：关掉再开，别每次都从屏幕的 45% 重新算 ──
typedef struct {
  int w;
  int h;
  gboolean maximized;
} WindowSizeState;

static gchar* window_state_dir(void) {
  const gchar* xdg = g_getenv("XDG_STATE_HOME");
  if (xdg != NULL && *xdg != '\0') {
    return g_strdup_printf("%s/yshjy", xdg);
  }
  const gchar* home = g_getenv("HOME");
  return g_strdup_printf("%s/.local/state/yshjy", home != NULL ? home : ".");
}

static gboolean load_window_size(WindowSizeState* st) {
  gchar* dir = window_state_dir();
  gchar* path = g_build_filename(dir, "gui-window.txt", NULL);
  gchar* text = NULL;
  gboolean ok = FALSE;
  if (g_file_get_contents(path, &text, NULL, NULL)) {
    int w = 0, h = 0, m = 0;
    // 太小/太大的值一律不认（换过屏幕、文件被改坏都走默认比例）
    if (sscanf(text, "%d %d %d", &w, &h, &m) >= 2 && w >= 420 && h >= 320) {
      st->w = w;
      st->h = h;
      st->maximized = (m != 0);
      ok = TRUE;
    }
    g_free(text);
  }
  g_free(path);
  g_free(dir);
  return ok;
}

static gboolean on_configure(GtkWidget* widget, GdkEventConfigure* event,
                             gpointer data) {
  WindowSizeState* st = (WindowSizeState*)data;
  // 只记「非最大化」尺寸：最大化时 configure 给的是整块屏幕
  if (!gtk_window_is_maximized(GTK_WINDOW(widget))) {
    st->w = event->width;
    st->h = event->height;
  }
  return FALSE;
}

static gboolean on_state_track(GtkWidget* widget, GdkEventWindowState* event,
                               gpointer data) {
  (void)widget;
  WindowSizeState* st = (WindowSizeState*)data;
  st->maximized = (event->new_window_state & GDK_WINDOW_STATE_MAXIMIZED) != 0;
  return FALSE;
}

static void on_destroy_save_size(GtkWidget* widget, gpointer data) {
  (void)widget;
  WindowSizeState* st = (WindowSizeState*)data;
  gchar* dir = window_state_dir();
  g_mkdir_with_parents(dir, 0700);
  gchar* path = g_build_filename(dir, "gui-window.txt", NULL);
  gchar* text = g_strdup_printf("%d %d %d\n", st->w, st->h, st->maximized ? 1 : 0);
  g_file_set_contents(path, text, -1, NULL);
  g_free(text);
  g_free(path);
  g_free(dir);
  g_free(st);
}

// Implements GApplication::activate.
static void my_application_activate(GApplication* application) {
  MyApplication* self = MY_APPLICATION(application);
  GtkWindow* window =
      GTK_WINDOW(gtk_application_window_new(GTK_APPLICATION(application)));

  // Use a header bar when running in GNOME as this is the common style used
  // by applications and is the setup most users will be using (e.g. Ubuntu
  // desktop).
  // If running on X and not using GNOME then just use a traditional title bar
  // in case the window manager does more exotic layout, e.g. tiling.
  // If running on Wayland assume the header bar will work (may need changing
  // if future cases occur).
  gboolean use_header_bar = TRUE;
#ifdef GDK_WINDOWING_X11
  GdkScreen* screen = gtk_window_get_screen(window);
  if (GDK_IS_X11_SCREEN(screen)) {
    const gchar* wm_name = gdk_x11_screen_get_window_manager_name(screen);
    if (g_strcmp0(wm_name, "GNOME Shell") != 0) {
      use_header_bar = FALSE;
    }
  }
#endif
  if (use_header_bar) {
    GtkHeaderBar* header_bar = GTK_HEADER_BAR(gtk_header_bar_new());
    gtk_widget_show(GTK_WIDGET(header_bar));
    gtk_header_bar_set_title(header_bar, "杨三海解压");
    gtk_header_bar_set_show_close_button(header_bar, TRUE);

    // 顺序说明：`gtk_header_bar_pack_end` 先加的紧挨关闭按钮，后加的往左，
    // 所以最终是 [最小化][最大化/还原][关闭]。
    GtkWidget* maximize_button =
        gtk_button_new_from_icon_name("window-maximize-symbolic", GTK_ICON_SIZE_MENU);
    // ⚠️ GTK3 新建的 widget 默认是 hidden，pack 进 header bar 也不会自动显示
    gtk_widget_show(maximize_button);
    gtk_widget_set_valign(maximize_button, GTK_ALIGN_CENTER);
    gtk_button_set_relief(GTK_BUTTON(maximize_button), GTK_RELIEF_NONE);
    gtk_widget_set_tooltip_text(maximize_button, "最大化 / 还原");
    g_signal_connect_swapped(maximize_button, "clicked",
                             G_CALLBACK(on_maximize_clicked), window);
    gtk_header_bar_pack_end(header_bar, maximize_button);
    g_signal_connect(window, "window-state-event",
                     G_CALLBACK(on_window_state_changed), maximize_button);

    GtkWidget* minimize_button =
        gtk_button_new_from_icon_name("window-minimize-symbolic", GTK_ICON_SIZE_MENU);
    gtk_widget_show(minimize_button);
    gtk_widget_set_valign(minimize_button, GTK_ALIGN_CENTER);
    gtk_button_set_relief(GTK_BUTTON(minimize_button), GTK_RELIEF_NONE);
    gtk_widget_set_tooltip_text(minimize_button, "最小化");
    g_signal_connect_swapped(minimize_button, "clicked",
                             G_CALLBACK(gtk_window_iconify), window);
    gtk_header_bar_pack_end(header_bar, minimize_button);

    gtk_window_set_titlebar(window, GTK_WIDGET(header_bar));
  } else {
    gtk_window_set_title(window, "杨三海解压");
  }

  // 窗口尺寸：**纯比例自适应**，不给任何屏幕写死数字。
  //
  // 为什么这么算：
  // 1. 用 `gdk_monitor_get_workarea` 而不是整块屏幕 —— 自动扣掉顶栏和 Dock，
  //    拿到的是"真正能用的区域"（GTK 返回的已是逻辑像素，HiDPI 下也正确）
  // 2. 宽 45%、高 66%（约 30% 面积）：小到能看见旁边的文件管理器（拖拽要用），
  //    又不至于连清单都放不下
  // 3. 上下限**也按比例算**，只有"内容放得下的物理最小值"才用绝对数（520×400），
  //    而且还要受屏幕限制（不能比屏幕还大）
  GdkDisplay *display = gdk_display_get_default();
  GdkMonitor *monitor = display ? gdk_display_get_primary_monitor(display) : nullptr;
  GdkRectangle work = {0, 0, 1280, 720};
  if (monitor != nullptr) {
    gdk_monitor_get_workarea(monitor, &work);
  }
  if (work.width < 320) work.width = 320;   // 极端情况兜底，防止算出负数
  if (work.height < 240) work.height = 240;

  // 内容能放下的最小尺寸（逻辑像素），但不能超过工作区的 85%
  int min_w = MIN(520, (int)(work.width * 0.85));
  // 560 而不是 400：预览页 = 格式行 + 智能解压提示 + 内容卡 + 挑选行 + 文件列表 + 按钮行，
  // 400 高度下列表会被压成 0 高、滚轮看起来「没反应」（用户实测报过）。
  int min_h = MIN(560, (int)(work.height * 0.85));
  // 上限同样按比例收，避免超宽屏上拉成一整条
  int max_w = MIN(1000, (int)(work.width * 0.85));
  int max_h = MIN(800, (int)(work.height * 0.90));
  if (max_w < min_w) max_w = min_w;
  if (max_h < min_h) max_h = min_h;

  int win_w = CLAMP((int)(work.width * 0.45), min_w, max_w);
  int win_h = CLAMP((int)(work.height * 0.66), min_h, max_h);

  // 上次关窗时的尺寸（P10）；换屏/改分辨率后 clamp 回当前工作区
  WindowSizeState* size_state = g_new0(WindowSizeState, 1);
  size_state->w = win_w;
  size_state->h = win_h;
  if (load_window_size(size_state)) {
    win_w = CLAMP(size_state->w, min_w, (int)(work.width * 0.98));
    win_h = CLAMP(size_state->h, min_h, (int)(work.height * 0.98));
    size_state->w = win_w;
    size_state->h = win_h;
  }
  gtk_window_set_default_size(window, win_w, win_h);
  // 用户手动拖小时也别小到布局散架
  gtk_widget_set_size_request(GTK_WIDGET(window), min_w, min_h);

  g_signal_connect(window, "configure-event", G_CALLBACK(on_configure), size_state);
  g_signal_connect(window, "window-state-event", G_CALLBACK(on_state_track), size_state);
  g_signal_connect(window, "destroy", G_CALLBACK(on_destroy_save_size), size_state);
  if (size_state->maximized) {
    gtk_window_maximize(window);
  }

  g_autoptr(FlDartProject) project = fl_dart_project_new();
  fl_dart_project_set_dart_entrypoint_arguments(
      project, self->dart_entrypoint_arguments);

  FlView* view = fl_view_new(project);
  GdkRGBA background_color;
  // Background defaults to black, override it here if necessary, e.g. #00000000
  // for transparent.
  gdk_rgba_parse(&background_color, "#000000");
  fl_view_set_background_color(view, &background_color);
  gtk_widget_show(GTK_WIDGET(view));
  gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));

  // Show the window when Flutter renders.
  // Requires the view to be realized so we can start rendering.
  g_signal_connect_swapped(view, "first-frame", G_CALLBACK(first_frame_cb),
                           self);
  gtk_widget_realize(GTK_WIDGET(view));

  fl_register_plugins(FL_PLUGIN_REGISTRY(view));

  gtk_widget_grab_focus(GTK_WIDGET(view));
}

// Implements GApplication::local_command_line.
static gboolean my_application_local_command_line(GApplication* application,
                                                  gchar*** arguments,
                                                  int* exit_status) {
  MyApplication* self = MY_APPLICATION(application);
  // Strip out the first argument as it is the binary name.
  self->dart_entrypoint_arguments = g_strdupv(*arguments + 1);

  g_autoptr(GError) error = nullptr;
  if (!g_application_register(application, nullptr, &error)) {
    g_warning("Failed to register: %s", error->message);
    *exit_status = 1;
    return TRUE;
  }

  g_application_activate(application);
  *exit_status = 0;

  return TRUE;
}

// Implements GApplication::startup.
static void my_application_startup(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application startup.

  G_APPLICATION_CLASS(my_application_parent_class)->startup(application);
}

// Implements GApplication::shutdown.
static void my_application_shutdown(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application shutdown.

  G_APPLICATION_CLASS(my_application_parent_class)->shutdown(application);
}

// Implements GObject::dispose.
static void my_application_dispose(GObject* object) {
  MyApplication* self = MY_APPLICATION(object);
  g_clear_pointer(&self->dart_entrypoint_arguments, g_strfreev);
  G_OBJECT_CLASS(my_application_parent_class)->dispose(object);
}

static void my_application_class_init(MyApplicationClass* klass) {
  G_APPLICATION_CLASS(klass)->activate = my_application_activate;
  G_APPLICATION_CLASS(klass)->local_command_line =
      my_application_local_command_line;
  G_APPLICATION_CLASS(klass)->startup = my_application_startup;
  G_APPLICATION_CLASS(klass)->shutdown = my_application_shutdown;
  G_OBJECT_CLASS(klass)->dispose = my_application_dispose;
}

static void my_application_init(MyApplication* self) {}

MyApplication* my_application_new() {
  // Set the program name to the application ID, which helps various systems
  // like GTK and desktop environments map this running application to its
  // corresponding .desktop file. This ensures better integration by allowing
  // the application to be recognized beyond its binary name.
  g_set_prgname(APPLICATION_ID);

  return MY_APPLICATION(g_object_new(my_application_get_type(),
                                     "application-id", APPLICATION_ID, "flags",
                                     G_APPLICATION_NON_UNIQUE, nullptr));
}
