// Goblins OS dock — a minimal, overview-independent bottom dock.
//
// dash-to-dock requires the GNOME overview, which the goblins-os session mode
// strips out. This extension instead adds a small chrome actor at bottom-center
// holding the pinned surfaces (org.gnome.shell favorite-apps) plus any running
// apps, styled by stylesheet.css to match the Goblins-native design language. No overview,
// no struts — it floats over the wallpaper like the macOS dock.

import Clutter from 'gi://Clutter';
import St from 'gi://St';
import Shell from 'gi://Shell';
import Gio from 'gi://Gio';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const ICON_SIZE = 44;
const BOTTOM_MARGIN = 14;

export default class GoblinsDock extends Extension {
    enable() {
        this._settings = new Gio.Settings({schema_id: 'org.gnome.shell'});
        this._appSystem = Shell.AppSystem.get_default();

        this._dock = new St.BoxLayout({
            style_class: 'goblins-dock',
            reactive: true,
            track_hover: true,
        });

        Main.layoutManager.addChrome(this._dock, {affectsStruts: false, trackFullscreen: true});

        const reposition = () => this._reposition();
        this._sizeId = this._dock.connect('notify::size', reposition);
        this._monitorsId = Main.layoutManager.connect('monitors-changed', reposition);
        this._favId = this._settings.connect('changed::favorite-apps', () => this._rebuild());
        this._runId = this._appSystem.connect('app-state-changed', () => this._rebuild());

        this._rebuild();
        reposition();
    }

    _rebuild() {
        if (!this._dock)
            return;
        this._dock.destroy_all_children();

        const ids = [...this._settings.get_strv('favorite-apps')];
        for (const app of this._appSystem.get_running()) {
            const id = app.get_id();
            // The home IS the shell — never dock the shell itself (it has no app
            // icon and would show a stray generic tile).
            if (id === 'org.goblins.OS.Shell.desktop')
                continue;
            if (!ids.includes(id))
                ids.push(id);
        }

        for (const id of ids) {
            const app = this._appSystem.lookup_app(id);
            if (!app)
                continue;
            const appName = app.get_name() || id.replace(/\.desktop$/, '');
            const isRunning = app.get_state() === Shell.AppState.RUNNING;
            const item = new St.Button({
                style_class: 'goblins-dock-item',
                can_focus: true,
                accessible_name: `Open ${appName}`,
            });
            // Icon + a running-indicator dot, stacked. The dot's space is reserved on
            // every item (transparent when not running) so the dock keeps a uniform
            // height; only running apps light it with the accent.
            const stack = new St.BoxLayout({
                vertical: true,
                x_align: Clutter.ActorAlign.CENTER,
            });
            const icon = app.create_icon_texture(ICON_SIZE);
            icon.set_x_align(Clutter.ActorAlign.CENTER);
            stack.add_child(icon);
            const dot = new St.Widget({
                style_class: 'goblins-dock-running-dot',
                x_align: Clutter.ActorAlign.CENTER,
                opacity: isRunning ? 255 : 0,
            });
            stack.add_child(dot);
            item.set_child(stack);
            item.connect('clicked', () => app.activate());
            if (isRunning)
                item.add_style_class_name('running');
            this._dock.add_child(item);
        }
        this._reposition();
    }

    _reposition() {
        if (!this._dock)
            return;
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const [width, height] = this._dock.get_size();
        this._dock.set_position(
            monitor.x + Math.max(0, Math.round((monitor.width - width) / 2)),
            monitor.y + monitor.height - height - BOTTOM_MARGIN
        );
    }

    disable() {
        if (this._sizeId)
            this._dock.disconnect(this._sizeId);
        if (this._monitorsId)
            Main.layoutManager.disconnect(this._monitorsId);
        if (this._favId)
            this._settings.disconnect(this._favId);
        if (this._runId)
            this._appSystem.disconnect(this._runId);
        if (this._dock) {
            Main.layoutManager.removeChrome(this._dock);
            this._dock.destroy();
        }
        this._dock = null;
        this._settings = null;
        this._appSystem = null;
    }
}
