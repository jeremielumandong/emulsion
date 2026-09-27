/* Small offscreen adapter for the system WebKitGTK engine. stdout is exclusively
 * a bounded stream of EMP1 + little-endian dimensions + packed BGRA frames.
 * GTK never blocks on the pipe: one frame at a time is transferred by a writer.
 * There is deliberately no page-to-native JavaScript bridge. */
#include <gtk/gtk.h>
#include <webkit2/webkit2.h>
#include <gst/gst.h>
#include <sys/prctl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <math.h>

static WebKitWebView *view;
static GtkWidget *window;
static const char *initial_uri;
static GAsyncQueue *frames;
static volatile gint frame_busy;
static guint buttons;
static int width, height;
typedef struct { guint8 *data; gsize length; } Frame;

static gboolean write_all(const void *bytes, size_t count) {
    const char *p = bytes;
    while (count) {
        ssize_t done = write(STDOUT_FILENO, p, count);
        if (done <= 0) return FALSE;
        p += done;
        count -= (size_t)done;
    }
    return TRUE;
}
static gpointer writer(gpointer unused) {
    (void)unused;
    for (;;) {
        Frame *frame = g_async_queue_pop(frames);
        gboolean ok = write_all(frame->data, frame->length);
        g_free(frame->data);
        g_free(frame);
        if (!ok) _exit(0); /* parent closed the reader */
        g_atomic_int_set(&frame_busy, 0);
    }
    return NULL;
}
static void snapshot_done(GObject *source, GAsyncResult *result, gpointer unused) {
    (void)unused;
    GError *error = NULL;
    cairo_surface_t *surface = webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(source), result, &error);
    if (!surface) { g_clear_error(&error); g_atomic_int_set(&frame_busy, 0); return; }
    if (cairo_surface_get_type(surface) != CAIRO_SURFACE_TYPE_IMAGE) {
        cairo_surface_destroy(surface); g_atomic_int_set(&frame_busy, 0); return;
    }
    /* GTK may choose a compositor scale independently of GDK_SCALE on Wayland.
     * Normalize the snapshot to our negotiated pixel size, matching input coords. */
    int sw = cairo_image_surface_get_width(surface), sh = cairo_image_surface_get_height(surface);
    if (sw != width || sh != height) {
        cairo_surface_t *scaled = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, width, height);
        cairo_t *cr = cairo_create(scaled);
        cairo_scale(cr, (double)width / sw, (double)height / sh);
        cairo_set_source_surface(cr, surface, 0, 0);
        cairo_paint(cr);
        cairo_destroy(cr);
        cairo_surface_destroy(surface);
        surface = scaled;
    }
    cairo_surface_flush(surface);
    int w = cairo_image_surface_get_width(surface), h = cairo_image_surface_get_height(surface);
    int stride = cairo_image_surface_get_stride(surface);
    if (w < 1 || h < 1 || w > 1920 || h > 1920 || w * h > 1920 * 1080) {
        cairo_surface_destroy(surface); g_atomic_int_set(&frame_busy, 0); return;
    }
    Frame *frame = g_new0(Frame, 1);
    frame->length = 12 + (gsize)w * h * 4;
    frame->data = g_malloc(frame->length);
    memcpy(frame->data, "EMP1", 4);
    guint32 le_w = GUINT32_TO_LE(w), le_h = GUINT32_TO_LE(h);
    memcpy(frame->data + 4, &le_w, 4); memcpy(frame->data + 8, &le_h, 4);
    const unsigned char *pixels = cairo_image_surface_get_data(surface);
    for (int y = 0; y < h; y++) {
        memcpy(frame->data + 12 + (gsize)y * w * 4, pixels + (gsize)y * stride, (gsize)w * 4);
    }
    cairo_surface_destroy(surface);
    g_async_queue_push(frames, frame);
}
static gboolean snapshot(gpointer unused) {
    (void)unused;
    if (g_atomic_int_compare_and_exchange(&frame_busy, 0, 1))
        webkit_web_view_get_snapshot(view, WEBKIT_SNAPSHOT_REGION_VISIBLE, WEBKIT_SNAPSHOT_OPTIONS_NONE, NULL, snapshot_done, NULL);
    return G_SOURCE_CONTINUE;
}
static gboolean allowed_uri(const char *uri) {
    return uri && (!strcmp(uri, initial_uri) ||
        g_str_has_prefix(uri, "https://www.youtube-nocookie.com/embed/") ||
        g_str_has_prefix(uri, "https://www.youtube.com/embed/"));
}
static gboolean policy(WebKitWebView *web, WebKitPolicyDecision *decision, WebKitPolicyDecisionType type, gpointer unused) {
    (void)web; (void)unused;
    if (type == WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION) {
        webkit_policy_decision_ignore(decision); return TRUE;
    }
    if (type == WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION) {
        WebKitNavigationAction *action = webkit_navigation_policy_decision_get_navigation_action(WEBKIT_NAVIGATION_POLICY_DECISION(decision));
        const char *uri = webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
        if (!allowed_uri(uri)) { webkit_policy_decision_ignore(decision); return TRUE; }
    }
    if (type == WEBKIT_POLICY_DECISION_TYPE_RESPONSE && !webkit_response_policy_decision_is_mime_type_supported(WEBKIT_RESPONSE_POLICY_DECISION(decision))) {
        webkit_policy_decision_ignore(decision); return TRUE;
    }
    return FALSE;
}
static gboolean permission(WebKitWebView *web, WebKitPermissionRequest *request, gpointer unused) {
    (void)web; (void)unused;
    webkit_permission_request_deny(request); return TRUE;
}
static gboolean context_menu(WebKitWebView *web, WebKitContextMenu *menu, GdkEvent *event, WebKitHitTestResult *hit, gpointer unused) {
    (void)web; (void)menu; (void)event; (void)hit; (void)unused;
    return TRUE;
}
static gboolean deny_fullscreen(WebKitWebView *web, gpointer unused) { (void)web; (void)unused; return TRUE; }
static void process_terminated(WebKitWebView *web, WebKitWebProcessTerminationReason reason, gpointer unused) {
    (void)web; (void)unused;
    fprintf(stderr, "EMULSION_PLAYER_ERROR: WebKit media process stopped (%d).\n", reason);
    gtk_main_quit();
}
static GdkDevice *device(gboolean keyboard) {
    GdkSeat *seat = gdk_display_get_default_seat(gdk_display_get_default());
    return keyboard ? gdk_seat_get_keyboard(seat) : gdk_seat_get_pointer(seat);
}
static void dispatch(GdkEvent *event, gboolean keyboard) {
    event->any.window = g_object_ref(gtk_widget_get_window(GTK_WIDGET(view)));
    event->any.send_event = TRUE;
    gdk_event_set_device(event, device(keyboard));
    gtk_widget_event(GTK_WIDGET(view), event);
    gdk_event_free(event);
}
static gboolean input(GIOChannel *channel, GIOCondition condition, gpointer unused) {
    (void)unused;
    if (condition & (G_IO_HUP | G_IO_ERR)) { gtk_main_quit(); return G_SOURCE_REMOVE; }
    gchar *line = NULL; gsize length = 0;
    GIOStatus status = g_io_channel_read_line(channel, &line, &length, NULL, NULL);
    if (status == G_IO_STATUS_EOF || status == G_IO_STATUS_ERROR) { gtk_main_quit(); return G_SOURCE_REMOVE; }
    if (status != G_IO_STATUS_NORMAL) return G_SOURCE_CONTINUE;
    if (length > 256) { g_free(line); gtk_main_quit(); return G_SOURCE_REMOVE; }
    float x, y, dx, dy; unsigned press, button, modifiers; int w,h; char key[41];
    if (sscanf(line, "R %d %d", &w, &h) == 2 && w > 0 && h > 0 && w <= 1920 && h <= 1920 && w*h <= 1920*1080) {
        width=w; height=h;
        gtk_widget_set_size_request(GTK_WIDGET(view), w, h);
        gtk_window_resize(GTK_WINDOW(window), w, h);
    } else if (sscanf(line, "M %f %f", &x, &y) == 2 && isfinite(x) && isfinite(y)) {
        GdkEvent *event=gdk_event_new(GDK_MOTION_NOTIFY);
        event->motion.time=GDK_CURRENT_TIME; event->motion.x=x; event->motion.y=y; event->motion.state=buttons;
        dispatch(event, FALSE);
    } else if (sscanf(line, "B %u %u %f %f", &press, &button, &x, &y) == 4 && button>=1 && button<=3 && isfinite(x) && isfinite(y)) {
        gtk_widget_grab_focus(GTK_WIDGET(view));
        GdkEvent *event=gdk_event_new(press ? GDK_BUTTON_PRESS : GDK_BUTTON_RELEASE);
        event->button.time=GDK_CURRENT_TIME; event->button.button=button;
        event->button.x=x; event->button.y=y; event->button.state=buttons;
        dispatch(event, FALSE);
        guint mask=GDK_BUTTON1_MASK << (button-1);
        if (press) buttons |= mask; else buttons &= ~mask;
    } else if (sscanf(line, "S %f %f %f %f", &dx, &dy, &x, &y) == 4 && isfinite(dx) && isfinite(dy) && isfinite(x) && isfinite(y)) {
        GdkEvent *event=gdk_event_new(GDK_SCROLL);
        event->scroll.time=GDK_CURRENT_TIME; event->scroll.x=x; event->scroll.y=y;
        event->scroll.direction=GDK_SCROLL_SMOOTH; event->scroll.delta_x=dx; event->scroll.delta_y=dy;
        dispatch(event, FALSE);
    } else if (sscanf(line, "K %u %u %40s", &press, &modifiers, key) == 3) {
        guint keyval=gdk_keyval_from_name(key);
        if (keyval != GDK_KEY_VoidSymbol) {
            GdkEvent *event=gdk_event_new(press ? GDK_KEY_PRESS : GDK_KEY_RELEASE);
            event->key.time=GDK_CURRENT_TIME; event->key.keyval=keyval; event->key.state=modifiers;
            GdkKeymapKey *keys=NULL; gint count=0;
            if (gdk_keymap_get_entries_for_keyval(gdk_keymap_get_for_display(gdk_display_get_default()),keyval,&keys,&count) && count) {
                event->key.hardware_keycode=keys[0].keycode; event->key.group=keys[0].group;
            }
            g_free(keys); dispatch(event, TRUE);
        }
    }
    g_free(line); return G_SOURCE_CONTINUE;
}
static gboolean has_decoder(const char *name) {
    GstElementFactory *factory = gst_element_factory_find(name);
    if (!factory) return FALSE;
    gst_object_unref(factory); return TRUE;
}
int main(int argc, char **argv) {
    prctl(PR_SET_PDEATHSIG, SIGTERM);
    if (getppid() == 1) return 1;
    signal(SIGPIPE, SIG_IGN);
    if (argc != 4 || !g_str_has_prefix(argv[1], "http://127.0.0.1:")) return 2;
    initial_uri=argv[1]; width=atoi(argv[2]); height=atoi(argv[3]);
    if (width<1 || height<1 || width>1920 || height>1920 || width*height>1920*1080) return 2;
    gst_init(NULL, NULL);
    if (!has_decoder("avdec_h264") && !has_decoder("vp9dec") && !has_decoder("vah264dec") && !has_decoder("vavp9dec")) {
        fprintf(stderr, "EMULSION_PLAYER_ERROR: No H.264 or VP9 video decoder. Install the GStreamer good and libav plugin packages.\n");
        return 3;
    }
    if (!gtk_init_check(NULL, NULL)) { fprintf(stderr,"EMULSION_PLAYER_ERROR: Cannot connect to the desktop display.\n"); return 4; }
    WebKitWebContext *context=webkit_web_context_new_ephemeral();
    view=WEBKIT_WEB_VIEW(webkit_web_view_new_with_context(context));
    g_object_unref(context);
    WebKitSettings *settings=webkit_web_view_get_settings(view);
    webkit_settings_set_hardware_acceleration_policy(settings, WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER);
    webkit_settings_set_media_playback_requires_user_gesture(settings, FALSE);
    webkit_settings_set_enable_fullscreen(settings, FALSE);
    webkit_settings_set_enable_developer_extras(settings, FALSE);
    webkit_settings_set_javascript_can_open_windows_automatically(settings, FALSE);
    window=gtk_offscreen_window_new();
    gtk_widget_set_size_request(GTK_WIDGET(view),width,height);
    gtk_container_add(GTK_CONTAINER(window),GTK_WIDGET(view));
    g_signal_connect(view,"decide-policy",G_CALLBACK(policy),NULL);
    g_signal_connect(view,"permission-request",G_CALLBACK(permission),NULL);
    g_signal_connect(view,"context-menu",G_CALLBACK(context_menu),NULL);
    g_signal_connect(view,"enter-fullscreen",G_CALLBACK(deny_fullscreen),NULL);
    g_signal_connect(view,"web-process-terminated",G_CALLBACK(process_terminated),NULL);
    gtk_widget_show_all(window);
    gtk_widget_grab_focus(GTK_WIDGET(view));
    frames=g_async_queue_new(); g_thread_unref(g_thread_new("player-frames",writer,NULL));
    GIOChannel *commands=g_io_channel_unix_new(STDIN_FILENO);
    g_io_channel_set_flags(commands,G_IO_FLAG_NONBLOCK,NULL);
    g_io_add_watch(commands,G_IO_IN|G_IO_HUP|G_IO_ERR,input,NULL);
    g_timeout_add(33,snapshot,NULL);
    webkit_web_view_load_uri(view,initial_uri);
    gtk_main();
    gtk_widget_destroy(window);
    g_io_channel_unref(commands);
    return 0;
}
