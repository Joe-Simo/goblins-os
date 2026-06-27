// Goblins OS window management.
//
// Mutter remains the real compositor/window manager. This extension adds the
// macOS-grade surfaces Goblins OS owns: Mission Control, Spaces, snap previews,
// a window-actions HUD, and a thumbnail app switcher. Every thumbnail is a live
// Clutter.Clone of an actual window actor; every move/resize uses Meta.Window.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const THUMB_W = 232;
const THUMB_H = 142;
const SWITCH_THUMB_W = 188;
const SWITCH_THUMB_H = 112;
const OVERLAY_FADE_MS = 180;
const SNAP_PREVIEW_FADE_MS = 140;
const SNAP_PREVIEW_VISIBLE_MS = 420;
const TOUCH_SWIPE_MIN = 84;
const ACTION_MODE = Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW | Shell.ActionMode.POPUP;

const KEYBINDINGS = [
    ['mission-control', '_showMissionControl'],
    ['app-expose', '_showAppExpose'],
    ['window-switcher', '_showSwitcher'],
    ['window-hud', '_showHud'],
    ['snap-left', '_snapLeft'],
    ['snap-right', '_snapRight'],
    ['snap-top-left', '_snapTopLeft'],
    ['snap-top-right', '_snapTopRight'],
    ['snap-bottom-left', '_snapBottomLeft'],
    ['snap-bottom-right', '_snapBottomRight'],
    ['restore-window', '_restoreFocusedWindow'],
    ['center-window', '_centerFocusedWindow'],
    ['space-left', '_activatePreviousWorkspace'],
    ['space-right', '_activateNextWorkspace'],
];

function now() {
    return global.get_current_time();
}

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

function safeTitle(win) {
    try {
        return win?.get_title?.() || 'Untitled';
    } catch (_error) {
        return 'Untitled';
    }
}

function spaceStripLabel(index, count) {
    if (count === 0)
        return `Space ${index + 1} - Empty`;
    if (count === 1)
        return `Space ${index + 1} - 1 window`;
    return `Space ${index + 1} - ${count} windows`;
}

export default class GoblinsWindowManagement extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._interfaceSettings = new Gio.Settings({schema_id: 'org.gnome.desktop.interface'});
        this._tracker = Shell.WindowTracker.get_default();
        this._appSystem = Shell.AppSystem.get_default();
        this._geometryBeforeSnap = new Map();
        this._recentWindows = [];
        this._selectedIndex = 0;
        this._groupFilter = null;
        this._snapApplyTimeout = 0;
        this._signals = [];
        this._screenshotMonitor = null;
        this._screenshotThumb = null;
        this._screenshotTimeout = 0;
        this._screenshotThumbTimeout = 0;
        this._lastShot = null;

        for (const [name, method] of KEYBINDINGS) {
            Main.wm.addKeybinding(
                name,
                this._settings,
                Meta.KeyBindingFlags.NONE,
                ACTION_MODE,
                () => this[method]()
            );
        }

        this._signals.push([
            global.display,
            global.display.connect('notify::focus-window', () => this._rememberFocus(global.display.focus_window)),
        ]);
        this._rememberFocus(global.display.focus_window);

        this._setupScreenshotWatch();

        globalThis.goblinsWindowManager = this;
    }

    disable() {
        for (const [name] of KEYBINDINGS)
            Main.wm.removeKeybinding(name);

        for (const [actor, id] of this._signals)
            actor.disconnect(id);
        this._signals = [];

        this.hide();
        this._clearSnapPreview();
        this._teardownScreenshotWatch();
        if (globalThis.goblinsWindowManager === this)
            delete globalThis.goblinsWindowManager;

        this._settings = null;
        this._interfaceSettings = null;
        this._tracker = null;
        this._appSystem = null;
        this._geometryBeforeSnap = null;
        this._recentWindows = null;
    }

    // ── macOS-style screenshot thumbnail ──────────────────────────────────────
    // Watch the folder GNOME's capture overlay writes to; when a new shot lands,
    // drop a framed thumbnail in the corner that fades on its own, or — clicked —
    // opens the Goblins markup editor on that file. The macOS capture-thumbnail
    // idiom, riding GNOME's own Wayland-correct capture stack. The whole path is
    // contained so a watch failure can never take the rest of the shell down.
    _setupScreenshotWatch() {
        try {
            const base =
                GLib.get_user_special_dir(GLib.UserDirectory.DIRECTORY_PICTURES) ||
                GLib.get_home_dir();
            this._screenshotDir = GLib.build_filenamev([base, 'Screenshots']);
            const dir = Gio.File.new_for_path(this._screenshotDir);
            if (!dir.query_exists(null))
                dir.make_directory_with_parents(null);
            this._screenshotMonitor = dir.monitor_directory(Gio.FileMonitorFlags.WATCH_MOVES, null);
            this._screenshotMonitor.connect('changed', (_monitor, file, _other, type) => {
                if (
                    type === Gio.FileMonitorEvent.CREATED ||
                    type === Gio.FileMonitorEvent.MOVED_IN
                )
                    this._onScreenshotCreated(file);
            });
        } catch (_error) {
            this._screenshotMonitor = null;
        }
    }

    _teardownScreenshotWatch() {
        if (this._screenshotTimeout) {
            GLib.source_remove(this._screenshotTimeout);
            this._screenshotTimeout = 0;
        }
        this._dismissScreenshotThumb();
        if (this._screenshotMonitor) {
            this._screenshotMonitor.cancel();
            this._screenshotMonitor = null;
        }
        this._screenshotDir = null;
        this._lastShot = null;
    }

    _onScreenshotCreated(file) {
        const path = file?.get_path?.();
        if (!path || !path.toLowerCase().endsWith('.png') || path === this._lastShot)
            return;
        this._lastShot = path;
        // Let the capture finish writing before we read it for the thumbnail.
        if (this._screenshotTimeout)
            GLib.source_remove(this._screenshotTimeout);
        this._screenshotTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 350, () => {
            this._screenshotTimeout = 0;
            this._showScreenshotThumbnail(path);
            return GLib.SOURCE_REMOVE;
        });
    }

    _showScreenshotThumbnail(path) {
        this._dismissScreenshotThumb();
        const width = 224;
        const height = 140;
        const thumb = new St.Button({
            style_class: 'goblins-screenshot-thumb',
            can_focus: true,
            reactive: true,
            track_hover: true,
            width,
            height,
        });
        thumb.set_style(`background-image: url("file://${path}");`);

        const monitor = Main.layoutManager.primaryMonitor;
        const margin = 28;
        Main.layoutManager.addChrome(thumb);
        thumb.set_position(
            monitor.x + monitor.width - width - margin,
            monitor.y + monitor.height - height - margin
        );
        thumb.set_pivot_point(0.5, 1.0);
        thumb.opacity = 0;
        thumb.scale_x = 0.92;
        thumb.scale_y = 0.92;
        thumb.ease({
            opacity: 255,
            scale_x: 1,
            scale_y: 1,
            duration: OVERLAY_FADE_MS,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });

        thumb.connect('clicked', () => {
            this._dismissScreenshotThumb();
            try {
                Gio.Subprocess.new(
                    ['/usr/libexec/goblins-os/goblins-os-markup', path],
                    Gio.SubprocessFlags.NONE
                );
            } catch (_error) {
                // Best-effort: the saved screenshot already exists on disk.
            }
        });

        this._screenshotThumb = thumb;
        this._screenshotThumbTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 5000, () => {
            this._screenshotThumbTimeout = 0;
            this._dismissScreenshotThumb();
            return GLib.SOURCE_REMOVE;
        });
    }

    _dismissScreenshotThumb() {
        if (this._screenshotThumbTimeout) {
            GLib.source_remove(this._screenshotThumbTimeout);
            this._screenshotThumbTimeout = 0;
        }
        const thumb = this._screenshotThumb;
        if (!thumb)
            return;
        this._screenshotThumb = null;
        thumb.ease({
            opacity: 0,
            duration: OVERLAY_FADE_MS,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => {
                Main.layoutManager.removeChrome(thumb);
                thumb.destroy();
            },
        });
    }

    // Stable hooks used by render-desktop.sh for real-pixel proof captures.
    showMissionControlDemo() {
        this._showMissionControl();
    }

    showSpacesDemo() {
        this._showMissionControl({spacesFocus: true});
    }

    // App Exposé — spread only the focused app's windows (macOS App Exposé). Falls
    // back to full Mission Control when nothing is focused / the app is unknown.
    _showAppExpose() {
        const focus = global.display.focus_window;
        const app = focus ? this._tracker.get_window_app(focus) : null;
        const appId = app?.get_id?.();
        if (!appId) {
            this._showMissionControl();
            return;
        }
        this._showMissionControl({appExpose: appId, title: app.get_name?.() || 'App Exposé'});
    }

    showSwitcherDemo() {
        this._showSwitcher();
    }

    showSnapPreviewDemo() {
        this._showSnapPreviewForFocused('right');
    }

    showHudDemo() {
        this._showHud();
    }

    hide() {
        this._groupFilter = null;
        if (this._overlay) {
            if (this._overlayKeyId)
                this._overlay.disconnect(this._overlayKeyId);
            if (this._overlayTouchId)
                this._overlay.disconnect(this._overlayTouchId);
            if (this._overlayMonitorId)
                Main.layoutManager.disconnect(this._overlayMonitorId);
            Main.layoutManager.removeChrome(this._overlay);
            this._overlay.destroy();
        }
        this._overlay = null;
        this._overlayKeyId = null;
        this._overlayTouchId = null;
        this._overlayMonitorId = null;
        this._touchStart = null;
    }

    _showMissionControl(options = {}) {
        this.hide();
        this._selectedIndex = 0;
        // App Exposé reuses the Mission Control overlay, pre-filtered to one app
        // (the existing per-app rail filter); hide() clears the filter afterwards.
        if (options.appExpose)
            this._groupFilter = options.appExpose;
        this._createOverlay('goblins-wm-overlay goblins-wm-mission-control');
        if (options.spacesFocus)
            this._overlay.add_style_class_name('spaces-focus');
        if (options.appExpose)
            this._overlay.add_style_class_name('app-expose');

        const header = new St.BoxLayout({style_class: 'goblins-wm-header'});
        header.add_child(new St.Label({
            text: options.title || 'Mission Control',
            style_class: 'goblins-wm-title',
        }));
        const spacer = new St.Widget({x_expand: true});
        header.add_child(spacer);
        const search = new St.Entry({
            style_class: 'goblins-wm-search',
            hint_text: 'Search windows',
            can_focus: true,
        });
        header.add_child(search);
        this._overlay.add_child(header);

        const body = new St.BoxLayout({style_class: 'goblins-wm-body', y_expand: true});
        const stageRail = new St.BoxLayout({
            style_class: 'goblins-wm-stage-rail',
            vertical: true,
            y_expand: true,
        });
        const windowArea = new St.BoxLayout({
            style_class: 'goblins-wm-window-area',
            vertical: true,
            x_expand: true,
            y_expand: true,
        });
        body.add_child(stageRail);
        body.add_child(windowArea);
        this._overlay.add_child(body);

        const spaces = new St.BoxLayout({
            style_class: 'goblins-wm-spaces-strip',
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.END,
        });
        this._overlay.add_child(spaces);

        const rebuild = () => {
            this._rebuildMissionControl({
                query: search.get_text().trim().toLowerCase(),
                stageRail,
                windowArea,
                spaces,
            });
        };

        search.clutter_text.connect('text-changed', rebuild);
        this._overlayKeyId = this._overlay.connect('key-press-event', (_actor, event) => {
            const symbol = event.get_key_symbol();
            if (symbol === Clutter.KEY_Escape) {
                this.hide();
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Return || symbol === Clutter.KEY_KP_Enter) {
                this._activateSelectedWindow();
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Left || symbol === Clutter.KEY_Up) {
                this._moveSelection(-1, rebuild);
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Right || symbol === Clutter.KEY_Down || symbol === Clutter.KEY_Tab) {
                this._moveSelection(1, rebuild);
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });

        rebuild();
        this._overlay.grab_key_focus();
    }

    _rebuildMissionControl({query, stageRail, windowArea, spaces}) {
        stageRail.destroy_all_children();
        windowArea.destroy_all_children();
        spaces.destroy_all_children();

        const windows = this._windowEntries().filter(entry => {
            const appName = entry.app?.get_name?.() || '';
            const haystack = `${entry.title} ${appName}`.toLowerCase();
            if (query && !haystack.includes(query))
                return false;
            if (this._groupFilter && entry.appId !== this._groupFilter)
                return false;
            return true;
        });
        this._visibleEntries = windows;
        if (this._selectedIndex >= windows.length)
            this._selectedIndex = Math.max(0, windows.length - 1);

        this._buildStageRail(stageRail);
        this._buildWorkspaceGroups(windowArea, windows);
        this._buildSpacesStrip(spaces);
    }

    _buildStageRail(stageRail) {
        const allButton = this._railButton('All Windows', null);
        stageRail.add_child(allButton);

        const groups = new Map();
        for (const entry of this._windowEntries()) {
            if (!groups.has(entry.appId))
                groups.set(entry.appId, {name: entry.appName, count: 0});
            groups.get(entry.appId).count += 1;
        }

        for (const [appId, group] of groups) {
            const button = this._railButton(`${group.name} (${group.count})`, appId);
            stageRail.add_child(button);
        }

        const divider = new St.Widget({style_class: 'goblins-wm-rail-divider'});
        stageRail.add_child(divider);
        const switcher = new St.Button({
            label: 'Switcher',
            style_class: 'goblins-wm-rail-action',
            can_focus: true,
            accessible_name: 'Open window switcher',
        });
        switcher.connect('clicked', () => this._showSwitcher());
        stageRail.add_child(switcher);
    }

    _railButton(label, appId) {
        const active = this._groupFilter === appId || (!this._groupFilter && !appId);
        const button = new St.Button({
            label,
            style_class: active ? 'goblins-wm-rail-button active' : 'goblins-wm-rail-button',
            can_focus: true,
            accessible_name: `Show ${label}`,
            x_align: Clutter.ActorAlign.START,
        });
        button.connect('clicked', () => {
            this._groupFilter = appId;
            this._showMissionControl();
        });
        return button;
    }

    _buildWorkspaceGroups(windowArea, windows) {
        const manager = global.workspace_manager;
        const buckets = new Map();
        for (const entry of windows) {
            const index = entry.workspaceIndex;
            if (!buckets.has(index))
                buckets.set(index, []);
            buckets.get(index).push(entry);
        }

        if (windows.length === 0) {
            windowArea.add_child(new St.Label({
                text: 'No matching windows',
                style_class: 'goblins-wm-empty',
            }));
            return;
        }

        for (let index = 0; index < manager.n_workspaces; index++) {
            const entries = buckets.get(index) || [];
            if (entries.length === 0)
                continue;
            const group = new St.BoxLayout({style_class: 'goblins-wm-workspace-group', vertical: true});
            const active = index === manager.get_active_workspace_index();
            group.add_child(new St.Label({
                text: active ? `Space ${index + 1} - Active` : `Space ${index + 1}`,
                style_class: active ? 'goblins-wm-space-heading active' : 'goblins-wm-space-heading',
            }));
            const grid = new St.BoxLayout({style_class: 'goblins-wm-window-grid'});
            for (const entry of entries)
                grid.add_child(this._windowCard(entry));
            group.add_child(grid);
            windowArea.add_child(group);
        }
    }

    _windowCard(entry) {
        const selected = this._visibleEntries?.[this._selectedIndex]?.key === entry.key;
        const button = new St.Button({
            style_class: selected ? 'goblins-wm-window-card selected' : 'goblins-wm-window-card',
            can_focus: true,
            reactive: true,
            accessible_name: `Activate ${entry.title}`,
        });
        const card = new St.BoxLayout({vertical: true});
        card.add_child(this._thumbnail(entry, THUMB_W, THUMB_H));

        const footer = new St.BoxLayout({style_class: 'goblins-wm-window-footer'});
        if (entry.app)
            footer.add_child(entry.app.create_icon_texture(22));
        footer.add_child(new St.Label({
            text: entry.title,
            style_class: 'goblins-wm-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        card.add_child(footer);
        button.set_child(card);
        button.connect('clicked', () => this._activateEntry(entry));
        return button;
    }

    _buildSpacesStrip(spaces) {
        const manager = global.workspace_manager;
        const active = manager.get_active_workspace_index();
        const counts = new Map();
        for (const entry of this._windowEntries())
            counts.set(entry.workspaceIndex, (counts.get(entry.workspaceIndex) || 0) + 1);

        const left = new St.Button({
            label: '<',
            style_class: 'goblins-wm-space-control',
            can_focus: true,
            accessible_name: 'Move to previous space',
        });
        left.connect('clicked', () => this._moveActiveWorkspace(-1));
        spaces.add_child(left);

        for (let index = 0; index < manager.n_workspaces; index++) {
            const label = spaceStripLabel(index, counts.get(index) || 0);
            const button = new St.Button({
                label,
                style_class: index === active ? 'goblins-wm-space-button active' : 'goblins-wm-space-button',
                can_focus: true,
                accessible_name: `Switch to ${label}`,
            });
            button.connect('clicked', () => this._activateWorkspace(index));
            spaces.add_child(button);
        }

        const right = new St.Button({
            label: '>',
            style_class: 'goblins-wm-space-control',
            can_focus: true,
            accessible_name: 'Move to next space',
        });
        right.connect('clicked', () => this._moveActiveWorkspace(1));
        spaces.add_child(right);

        const add = new St.Button({
            label: '+',
            style_class: 'goblins-wm-space-control add',
            can_focus: true,
            accessible_name: 'Add a space',
        });
        add.connect('clicked', () => {
            try {
                manager.append_new_workspace(false, now());
                this._showMissionControl({spacesFocus: true});
            } catch (error) {
                logError(error, 'goblins-wm: failed to add workspace');
            }
        });
        spaces.add_child(add);

        const remove = new St.Button({
            label: '-',
            style_class: 'goblins-wm-space-control',
            can_focus: true,
            accessible_name: 'Remove the current empty space',
        });
        remove.connect('clicked', () => {
            this._removeActiveWorkspaceIfEmpty();
            this._showMissionControl({spacesFocus: true});
        });
        spaces.add_child(remove);
    }

    _showSwitcher() {
        this.hide();
        this._selectedIndex = 0;
        this._createOverlay('goblins-wm-overlay goblins-wm-switcher-overlay');
        const panel = new St.BoxLayout({style_class: 'goblins-wm-switcher'});
        this._overlay.add_child(new St.Widget({y_expand: true}));
        this._overlay.add_child(panel);
        this._overlay.add_child(new St.Widget({y_expand: true}));

        const rebuild = () => {
            panel.destroy_all_children();
            const entries = this._windowEntries();
            this._visibleEntries = entries;
            if (this._selectedIndex >= entries.length)
                this._selectedIndex = Math.max(0, entries.length - 1);
            for (const entry of entries)
                panel.add_child(this._switcherCard(entry));
        };

        this._overlayKeyId = this._overlay.connect('key-press-event', (_actor, event) => {
            const symbol = event.get_key_symbol();
            if (symbol === Clutter.KEY_Escape) {
                this.hide();
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Return || symbol === Clutter.KEY_KP_Enter) {
                this._activateSelectedWindow();
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Tab || symbol === Clutter.KEY_Right || symbol === Clutter.KEY_Down) {
                this._moveSelection(1, rebuild);
                return Clutter.EVENT_STOP;
            }
            if (symbol === Clutter.KEY_Left || symbol === Clutter.KEY_Up) {
                this._moveSelection(-1, rebuild);
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });

        rebuild();
        this._overlay.grab_key_focus();
    }

    _switcherCard(entry) {
        const selected = this._visibleEntries?.[this._selectedIndex]?.key === entry.key;
        const button = new St.Button({
            style_class: selected ? 'goblins-wm-switch-card selected' : 'goblins-wm-switch-card',
            can_focus: true,
            accessible_name: `Switch to ${entry.title}`,
        });
        const box = new St.BoxLayout({style_class: 'goblins-wm-switch-card-inner', vertical: true});
        box.add_child(this._thumbnail(entry, SWITCH_THUMB_W, SWITCH_THUMB_H));
        const row = new St.BoxLayout({style_class: 'goblins-wm-switch-title-row'});
        if (entry.app)
            row.add_child(entry.app.create_icon_texture(20));
        row.add_child(new St.Label({text: entry.title, style_class: 'goblins-wm-switch-title'}));
        box.add_child(row);
        button.set_child(box);
        button.connect('clicked', () => this._activateEntry(entry));
        return button;
    }

    _showHud() {
        this.hide();
        const entry = this._focusedEntry() || this._windowEntries()[0];
        if (!entry)
            return;

        this._createOverlay('goblins-wm-overlay goblins-wm-hud-overlay');
        const panel = new St.BoxLayout({style_class: 'goblins-wm-hud', vertical: true});
        const title = new St.BoxLayout({style_class: 'goblins-wm-hud-title-row'});
        if (entry.app)
            title.add_child(entry.app.create_icon_texture(24));
        title.add_child(new St.Label({text: entry.title, style_class: 'goblins-wm-hud-title'}));
        panel.add_child(title);

        const actions = new St.BoxLayout({style_class: 'goblins-wm-hud-actions'});
        actions.add_child(this._hudButton('Close', () => entry.window.delete(now())));
        actions.add_child(this._hudButton('Minimize', () => entry.window.minimize()));
        actions.add_child(this._hudButton('Maximize', () => this._toggleMaximize(entry.window)));
        actions.add_child(this._hudButton('Fullscreen', () => this._toggleFullscreen(entry.window)));
        panel.add_child(actions);

        const tiling = new St.BoxLayout({style_class: 'goblins-wm-hud-actions'});
        tiling.add_child(this._hudButton('Left', () => this._snapWindow(entry.window, 'left')));
        tiling.add_child(this._hudButton('Right', () => this._snapWindow(entry.window, 'right')));
        tiling.add_child(this._hudButton('Center', () => this._centerWindow(entry.window)));
        tiling.add_child(this._hudButton('Restore', () => this._restoreWindow(entry.window)));
        panel.add_child(tiling);

        const spaces = new St.BoxLayout({style_class: 'goblins-wm-hud-spaces'});
        const manager = global.workspace_manager;
        for (let index = 0; index < Math.min(manager.n_workspaces, 6); index++) {
            const button = this._hudButton(`Space ${index + 1}`, () => this._moveWindowToWorkspace(entry.window, index));
            spaces.add_child(button);
        }
        panel.add_child(spaces);

        const detail = this._hudButton('App Details', () => this._openAppDetails(entry), {
            disabled: entry.title === 'Untitled',
            styleClass: entry.title === 'Untitled' ? 'disabled' : 'primary',
        });
        panel.add_child(detail);

        this._overlay.add_child(new St.Widget({y_expand: true}));
        const center = new St.BoxLayout({x_align: Clutter.ActorAlign.CENTER});
        center.add_child(panel);
        this._overlay.add_child(center);
        this._overlay.add_child(new St.Widget({y_expand: true}));

        this._overlayKeyId = this._overlay.connect('key-press-event', (_actor, event) => {
            if (event.get_key_symbol() === Clutter.KEY_Escape) {
                this.hide();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        this._overlay.grab_key_focus();
    }

    _hudButton(label, callback, options = {}) {
        const disabled = Boolean(options.disabled);
        const styleClass = ['goblins-wm-hud-button', options.styleClass]
            .filter(Boolean)
            .join(' ');
        const button = new St.Button({
            label,
            style_class: styleClass,
            can_focus: !disabled,
            reactive: !disabled,
            accessible_name: options.accessibleName || label,
        });
        if (disabled)
            return button;
        button.connect('clicked', () => {
            try {
                callback();
            } catch (error) {
                logError(error, `goblins-wm: action failed: ${label}`);
            }
            this.hide();
        });
        return button;
    }

    _thumbnail(entry, width, height) {
        const frame = new St.Widget({
            style_class: 'goblins-wm-thumbnail',
            clip_to_allocation: true,
            width,
            height,
        });
        const [actorWidth, actorHeight] = entry.actor.get_size();
        if (actorWidth <= 0 || actorHeight <= 0) {
            frame.add_child(new St.Label({text: entry.title, style_class: 'goblins-wm-thumbnail-placeholder'}));
            return frame;
        }

        const clone = new Clutter.Clone({source: entry.actor});
        const scale = Math.min(width / actorWidth, height / actorHeight);
        clone.set_scale(scale, scale);
        clone.set_position(
            Math.round((width - actorWidth * scale) / 2),
            Math.round((height - actorHeight * scale) / 2)
        );
        frame.add_child(clone);
        return frame;
    }

    _createOverlay(styleClass) {
        const monitor = Main.layoutManager.primaryMonitor;
        this._overlay = new St.BoxLayout({
            style_class: `${styleClass} ${this._schemeClass()}`,
            vertical: true,
            reactive: true,
            can_focus: true,
        });
        Main.layoutManager.addChrome(this._overlay, {affectsStruts: false, trackFullscreen: true});
        this._overlayMonitorId = Main.layoutManager.connect('monitors-changed', () => this._syncOverlayBounds());
        this._overlayTouchId = this._overlay.connect('touch-event', (_actor, event) => this._handleTouchEvent(event));
        this._overlay.set_opacity(0);
        this._syncOverlayBounds(monitor);
        this._fadeIn(this._overlay, OVERLAY_FADE_MS);
    }

    _syncOverlayBounds() {
        if (!this._overlay)
            return;
        const monitor = Main.layoutManager.primaryMonitor;
        this._overlay.set_position(monitor.x, monitor.y);
        this._overlay.set_size(monitor.width, monitor.height);
    }

    _schemeClass() {
        try {
            return this._interfaceSettings.get_string('color-scheme') === 'prefer-dark' ? 'dark' : 'light';
        } catch (_error) {
            return 'dark';
        }
    }

    _handleTouchEvent(event) {
        const type = event.type();
        if (type === Clutter.EventType.TOUCH_BEGIN) {
            this._touchStart = this._eventCoords(event);
            return Clutter.EVENT_PROPAGATE;
        }
        if (type === Clutter.EventType.TOUCH_CANCEL) {
            this._touchStart = null;
            return Clutter.EVENT_PROPAGATE;
        }
        if (type !== Clutter.EventType.TOUCH_END || !this._touchStart)
            return Clutter.EVENT_PROPAGATE;

        const end = this._eventCoords(event);
        const dx = end.x - this._touchStart.x;
        const dy = end.y - this._touchStart.y;
        this._touchStart = null;

        if (Math.abs(dx) >= TOUCH_SWIPE_MIN && Math.abs(dx) > Math.abs(dy) * 1.25) {
            this._activateWorkspaceRelative(dx < 0 ? 1 : -1);
            this._showMissionControl({spacesFocus: true});
            return Clutter.EVENT_STOP;
        }
        if (dy >= TOUCH_SWIPE_MIN * 1.15 && Math.abs(dy) > Math.abs(dx) * 1.25) {
            this.hide();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    _eventCoords(event) {
        try {
            const [x, y] = event.get_coords();
            return {x, y};
        } catch (_error) {
            return {x: 0, y: 0};
        }
    }

    _windowEntries() {
        const actors = global.get_window_actors();
        const entries = [];
        for (const actor of actors) {
            const win = actor.get_meta_window();
            if (!this._isUsableWindow(win))
                continue;
            const app = this._tracker.get_window_app(win);
            const appId = app?.get_id?.() || 'unknown';
            const workspace = win.get_workspace?.();
            entries.push({
                actor,
                window: win,
                key: this._windowKey(win),
                title: safeTitle(win),
                app,
                appId,
                appName: app?.get_name?.() || 'Window',
                workspaceIndex: workspace?.index?.() ?? 0,
            });
        }

        const recent = this._recentWindows || [];
        entries.sort((a, b) => {
            const aIndex = recent.indexOf(a.key);
            const bIndex = recent.indexOf(b.key);
            if (aIndex !== bIndex) {
                if (aIndex === -1)
                    return 1;
                if (bIndex === -1)
                    return -1;
                return aIndex - bIndex;
            }
            if (a.workspaceIndex !== b.workspaceIndex)
                return a.workspaceIndex - b.workspaceIndex;
            return a.title.localeCompare(b.title);
        });
        return entries;
    }

    _isUsableWindow(win) {
        if (!win)
            return false;
        try {
            const type = win.get_window_type?.();
            if (type === Meta.WindowType.DESKTOP || type === Meta.WindowType.DOCK || type === Meta.WindowType.SPLASHSCREEN)
                return false;
            if (win.is_skip_taskbar?.() || win.skip_taskbar)
                return false;
            if (win.is_override_redirect?.())
                return false;
            if (!win.showing_on_its_workspace?.())
                return false;
        } catch (_error) {
            return false;
        }
        return true;
    }

    _focusedEntry() {
        const focused = global.display.focus_window;
        if (!focused)
            return null;
        const key = this._windowKey(focused);
        return this._windowEntries().find(entry => entry.key === key) || null;
    }

    _windowKey(win) {
        try {
            return `${win.get_stable_sequence()}`;
        } catch (_error) {
            return `${safeTitle(win)}:${win.get_pid?.() || 0}`;
        }
    }

    _rememberFocus(win) {
        if (!this._recentWindows || !this._isUsableWindow(win))
            return;
        const key = this._windowKey(win);
        this._recentWindows = [key, ...this._recentWindows.filter(item => item !== key)].slice(0, 48);
    }

    _moveSelection(delta, rebuild) {
        const count = this._visibleEntries?.length || 0;
        if (count === 0)
            return;
        this._selectedIndex = (this._selectedIndex + delta + count) % count;
        rebuild();
    }

    _activateSelectedWindow() {
        const entry = this._visibleEntries?.[this._selectedIndex];
        if (entry)
            this._activateEntry(entry);
    }

    _activateEntry(entry) {
        try {
            entry.window.get_workspace?.()?.activate(now());
            entry.window.activate(now());
            this._rememberFocus(entry.window);
        } catch (error) {
            logError(error, 'goblins-wm: failed to focus window');
        }
        this.hide();
    }

    _activateWorkspace(index) {
        const workspace = global.workspace_manager.get_workspace_by_index(index);
        workspace?.activate(now());
        this.hide();
    }

    _activatePreviousWorkspace() {
        this._activateWorkspaceRelative(-1);
    }

    _activateNextWorkspace() {
        this._activateWorkspaceRelative(1);
    }

    _activateWorkspaceRelative(delta) {
        const manager = global.workspace_manager;
        const index = clamp(manager.get_active_workspace_index() + delta, 0, manager.n_workspaces - 1);
        manager.get_workspace_by_index(index)?.activate(now());
    }

    _moveActiveWorkspace(delta) {
        const manager = global.workspace_manager;
        const index = manager.get_active_workspace_index();
        const workspace = manager.get_workspace_by_index(index);
        const target = clamp(index + delta, 0, manager.n_workspaces - 1);
        if (workspace && target !== index && manager.reorder_workspace)
            manager.reorder_workspace(workspace, target);
        this._showMissionControl({spacesFocus: true});
    }

    _removeActiveWorkspaceIfEmpty() {
        const manager = global.workspace_manager;
        if (manager.n_workspaces <= 1 || !manager.remove_workspace)
            return;
        const index = manager.get_active_workspace_index();
        const hasWindows = this._windowEntries().some(entry => entry.workspaceIndex === index);
        if (!hasWindows)
            manager.remove_workspace(manager.get_workspace_by_index(index), now());
    }

    _moveWindowToWorkspace(win, index) {
        const workspace = global.workspace_manager.get_workspace_by_index(index);
        if (!workspace)
            return;
        win.change_workspace(workspace);
        workspace.activate(now());
        win.activate(now());
    }

    _snapLeft() {
        this._snapFocusedWindow('left');
    }

    _snapRight() {
        this._snapFocusedWindow('right');
    }

    _snapTopLeft() {
        this._snapFocusedWindow('top-left');
    }

    _snapTopRight() {
        this._snapFocusedWindow('top-right');
    }

    _snapBottomLeft() {
        this._snapFocusedWindow('bottom-left');
    }

    _snapBottomRight() {
        this._snapFocusedWindow('bottom-right');
    }

    _snapFocusedWindow(zone) {
        const win = global.display.focus_window;
        if (win)
            this._snapWindow(win, zone);
    }

    _showSnapPreviewForFocused(zone) {
        const win = global.display.focus_window || this._windowEntries()[0]?.window;
        if (win)
            this._showSnapPreview(this._rectForZone(win, zone));
    }

    _snapWindow(win, zone) {
        if (!this._isUsableWindow(win))
            return;
        const key = this._windowKey(win);
        if (!this._geometryBeforeSnap.has(key))
            this._geometryBeforeSnap.set(key, win.get_frame_rect());
        const rect = this._rectForZone(win, zone);
        this._showSnapPreview(rect);
        this._snapApplyTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 90, () => {
            this._snapApplyTimeout = 0;
            try {
                win.unmaximize(Meta.MaximizeFlags.BOTH);
                win.move_resize_frame(false, rect.x, rect.y, rect.width, rect.height);
                win.activate(now());
            } catch (error) {
                logError(error, 'goblins-wm: snap failed');
            }
            return GLib.SOURCE_REMOVE;
        });
    }

    _restoreFocusedWindow() {
        const win = global.display.focus_window;
        if (win)
            this._restoreWindow(win);
    }

    _restoreWindow(win) {
        const key = this._windowKey(win);
        const rect = this._geometryBeforeSnap.get(key);
        if (!rect)
            return;
        try {
            win.unmaximize(Meta.MaximizeFlags.BOTH);
            win.move_resize_frame(false, rect.x, rect.y, rect.width, rect.height);
            win.activate(now());
            this._geometryBeforeSnap.delete(key);
        } catch (error) {
            logError(error, 'goblins-wm: restore failed');
        }
    }

    _centerFocusedWindow() {
        const win = global.display.focus_window;
        if (win)
            this._centerWindow(win);
    }

    _centerWindow(win) {
        const area = this._workArea(win);
        const frame = win.get_frame_rect();
        const width = Math.min(frame.width, Math.round(area.width * 0.76));
        const height = Math.min(frame.height, Math.round(area.height * 0.78));
        const x = area.x + Math.round((area.width - width) / 2);
        const y = area.y + Math.round((area.height - height) / 2);
        this._showSnapPreview({x, y, width, height});
        win.unmaximize(Meta.MaximizeFlags.BOTH);
        win.move_resize_frame(false, x, y, width, height);
        win.activate(now());
    }

    _rectForZone(win, zone) {
        const area = this._workArea(win);
        const halfW = Math.round(area.width / 2);
        const halfH = Math.round(area.height / 2);
        switch (zone) {
        case 'left':
            return {x: area.x, y: area.y, width: halfW, height: area.height};
        case 'right':
            return {x: area.x + halfW, y: area.y, width: area.width - halfW, height: area.height};
        case 'top-left':
            return {x: area.x, y: area.y, width: halfW, height: halfH};
        case 'top-right':
            return {x: area.x + halfW, y: area.y, width: area.width - halfW, height: halfH};
        case 'bottom-left':
            return {x: area.x, y: area.y + halfH, width: halfW, height: area.height - halfH};
        case 'bottom-right':
            return {x: area.x + halfW, y: area.y + halfH, width: area.width - halfW, height: area.height - halfH};
        default:
            return {x: area.x, y: area.y, width: area.width, height: area.height};
        }
    }

    _workArea(win) {
        const workspace = win.get_workspace?.() || global.workspace_manager.get_active_workspace();
        const monitor = win.get_monitor?.() ?? Main.layoutManager.primaryIndex;
        try {
            return workspace.get_work_area_for_monitor(monitor);
        } catch (_error) {
            const primary = Main.layoutManager.primaryMonitor;
            return {x: primary.x, y: primary.y + Main.panel.height, width: primary.width, height: primary.height - Main.panel.height};
        }
    }

    _showSnapPreview(rect) {
        this._clearSnapPreview();
        this._snapPreview = new St.Widget({style_class: `goblins-wm-snap-preview ${this._schemeClass()}`});
        Main.layoutManager.addChrome(this._snapPreview, {affectsStruts: false, trackFullscreen: true});
        this._snapPreview.set_position(rect.x + 10, rect.y + 10);
        this._snapPreview.set_size(Math.max(80, rect.width - 20), Math.max(80, rect.height - 20));
        this._snapPreview.set_opacity(0);
        this._fadeIn(this._snapPreview, SNAP_PREVIEW_FADE_MS);
        this._snapPreviewTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SNAP_PREVIEW_VISIBLE_MS, () => {
            this._clearSnapPreview();
            return GLib.SOURCE_REMOVE;
        });
    }

    _fadeIn(actor, duration) {
        if (!this._animationsEnabled()) {
            actor.set_opacity(255);
            return;
        }
        actor.ease({
            opacity: 255,
            duration,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
    }

    _animationsEnabled() {
        try {
            return this._interfaceSettings.get_boolean('enable-animations');
        } catch (_error) {
            return true;
        }
    }

    _clearSnapPreview() {
        if (this._snapPreviewTimeout) {
            GLib.source_remove(this._snapPreviewTimeout);
            this._snapPreviewTimeout = null;
        }
        if (this._snapApplyTimeout) {
            GLib.source_remove(this._snapApplyTimeout);
            this._snapApplyTimeout = 0;
        }
        if (this._snapPreview) {
            Main.layoutManager.removeChrome(this._snapPreview);
            this._snapPreview.destroy();
            this._snapPreview = null;
        }
    }

    _toggleMaximize(win) {
        const maximized = (win.get_maximized?.() || 0) === Meta.MaximizeFlags.BOTH;
        if (maximized)
            win.unmaximize(Meta.MaximizeFlags.BOTH);
        else
            win.maximize(Meta.MaximizeFlags.BOTH);
        win.activate(now());
    }

    _toggleFullscreen(win) {
        if (win.is_fullscreen?.())
            win.unmake_fullscreen();
        else
            win.make_fullscreen();
        win.activate(now());
    }

    _openAppDetails(entry) {
        try {
            Gio.Subprocess.new(
                ['/usr/libexec/goblins-os/goblins-os-shell', '--open-app', entry.title],
                Gio.SubprocessFlags.NONE
            );
        } catch (error) {
            logError(error, 'goblins-wm: failed to open app details');
        }
    }
}
