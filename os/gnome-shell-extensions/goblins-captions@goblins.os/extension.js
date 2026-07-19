// Goblins OS Live Captions overlay.
//
// This extension owns only the shell surface. Capture and STT stay in the core
// service and are honestly gated there; until the stream is live, the overlay
// shows a waiting/status line rather than fabricated captions.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as QuickSettings from 'resource:///org/gnome/shell/ui/quickSettings.js';

const SCHEMA_ID = 'org.goblins.shell.extensions.captions';
const MARGIN = 36;
const BOTTOM_DOCK_CLEARANCE = 120;
const WAITING_COPY = 'Live Captions are waiting for the local caption stream.';

const LiveCaptionsToggle = GObject.registerClass(
class LiveCaptionsToggle extends QuickSettings.QuickToggle {
    constructor(settings) {
        super({
            title: 'Live Captions',
            subtitle: 'Off',
            iconName: 'audio-input-microphone-symbolic',
            toggleMode: true,
        });

        this._settings = settings;
        this._settings.bind('enabled', this, 'checked', Gio.SettingsBindFlags.DEFAULT);
        this._settingsChangedId = this._settings.connect('changed::enabled', () => this._sync());
        this._sync();
    }

    destroy() {
        if (this._settingsChangedId) {
            this._settings.disconnect(this._settingsChangedId);
            this._settingsChangedId = 0;
        }
        this._settings = null;
        super.destroy();
    }

    _sync() {
        this.subtitle = this.checked ? 'Waiting for stream' : 'Off';
    }
});

const LiveCaptionsIndicator = GObject.registerClass(
class LiveCaptionsIndicator extends QuickSettings.SystemIndicator {
    constructor(settings) {
        super();

        this._settings = settings;
        this._indicator = this._addIndicator();
        this._indicator.icon_name = 'audio-input-microphone-symbolic';
        this._settings.bind('enabled', this._indicator, 'visible', Gio.SettingsBindFlags.DEFAULT);
        this.quickSettingsItems.push(new LiveCaptionsToggle(settings));
    }

    destroy() {
        this.quickSettingsItems.forEach(item => item.destroy());
        this._settings = null;
        super.destroy();
    }
});

function settingString(settings, key, fallback) {
    try {
        return settings.get_string(key) || fallback;
    } catch (_error) {
        return fallback;
    }
}

export default class GoblinsLiveCaptions extends Extension {
    enable() {
        this._settings = new Gio.Settings({schema_id: SCHEMA_ID});
        this._signals = [];
        this._renderProofActive = false;
        this._indicator = new LiveCaptionsIndicator(this._settings);
        Main.panel.statusArea.quickSettings.addExternalIndicator(this._indicator);

        this._overlay = new St.BoxLayout({
            style_class: 'goblins-captions-overlay',
            reactive: false,
            visible: false,
        });
        this._dot = new St.Widget({style_class: 'goblins-captions-dot idle'});
        this._label = new St.Label({
            style_class: 'goblins-captions-text',
            text: '',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._overlay.add_child(this._dot);
        this._overlay.add_child(this._label);
        Main.layoutManager.addChrome(this._overlay, {affectsStruts: false, trackFullscreen: true});

        // Chrome actors can be made visible again by a stage relayout when a
        // native window maps. Disabled captions must remain absent, and stale
        // caption text must never return merely because the desktop relaid out.
        this._signals.push([
            this._overlay,
            this._overlay.connect('notify::visible', () => {
                if (
                    this._overlay?.visible &&
                    !this._settings?.get_boolean('enabled') &&
                    !this._renderProofActive
                )
                    this.hide();
            }),
        ]);

        for (const key of ['enabled', 'text-size', 'position', 'keep-onscreen']) {
            this._signals.push([
                this._settings,
                this._settings.connect(`changed::${key}`, () => this._sync()),
            ]);
        }
        this._signals.push([
            Main.layoutManager,
            Main.layoutManager.connect('monitors-changed', () => this._reposition()),
        ]);

        globalThis.goblinsLiveCaptions = this;
        this._sync();
    }

    disable() {
        this.hide();
        for (const [actor, id] of this._signals)
            actor.disconnect(id);
        this._signals = [];

        if (globalThis.goblinsLiveCaptions === this)
            delete globalThis.goblinsLiveCaptions;

        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }

        if (this._overlay) {
            Main.layoutManager.removeChrome(this._overlay);
            this._overlay.destroy();
        }

        this._overlay = null;
        this._label = null;
        this._dot = null;
        this._settings = null;
        this._renderProofActive = false;
    }

    showStatus(text = WAITING_COPY, renderProof = false) {
        if (!this._overlay)
            return false;
        if (!renderProof && !this._settings?.get_boolean('enabled')) {
            this.hide();
            return false;
        }
        this._renderProofActive = renderProof;
        this._label.set_text(text);
        this._dot.set_style_class_name('goblins-captions-dot idle');
        this._overlay.show();
        this._reposition();
        return true;
    }

    showWaitingRenderProof() {
        if (!this.showStatus(WAITING_COPY, true))
            throw new Error('Live Captions waiting proof could not be shown');
        return {
            proof: 'waiting-overlay-only',
            waitingCopy: WAITING_COPY,
            captureRuntimeReadyClaim: false,
            transcriptionReadyClaim: false,
            liveCaptionTextClaim: false,
        };
    }

    showCaption(text) {
        if (!this._overlay || !text)
            return;
        if (!this._settings?.get_boolean('enabled')) {
            this.hide();
            return;
        }
        this._renderProofActive = false;
        this._label.set_text(text);
        this._dot.set_style_class_name('goblins-captions-dot live');
        this._overlay.show();
        this._reposition();
    }

    hide() {
        this._renderProofActive = false;
        if (this._label)
            this._label.set_text('');
        if (this._overlay)
            this._overlay.hide();
    }

    renderProofInactive() {
        return Boolean(
            this._settings &&
            !this._settings.get_boolean('enabled') &&
            !this._overlay?.visible &&
            !this._renderProofActive &&
            this._label?.text === ''
        );
    }

    renderProofWaiting() {
        return Boolean(
            this._settings &&
            !this._settings.get_boolean('enabled') &&
            this._renderProofActive &&
            this._overlay?.visible &&
            this._overlay.is_mapped() &&
            this._label?.text === WAITING_COPY
        );
    }

    _sync() {
        if (!this._settings || !this._overlay)
            return;
        this._applyStyle();
        if (this._settings.get_boolean('enabled'))
            this.showStatus();
        else
            this.hide();
    }

    _applyStyle() {
        const size = settingString(this._settings, 'text-size', 'medium');
        const position = settingString(this._settings, 'position', 'bottom');
        this._overlay.set_style_class_name(
            `goblins-captions-overlay size-${size} position-${position}`
        );
    }

    _reposition() {
        if (!this._overlay || !this._overlay.visible)
            return;
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;

        const position = settingString(this._settings, 'position', 'bottom');
        const [width, height] = this._overlay.get_size();
        const x = monitor.x + Math.max(MARGIN, Math.round((monitor.width - width) / 2));
        let y = monitor.y + monitor.height - height - BOTTOM_DOCK_CLEARANCE;
        if (position === 'top')
            y = monitor.y + MARGIN;
        else if (position === 'floating')
            y = monitor.y + Math.round(monitor.height * 0.66);

        this._overlay.set_position(x, y);
    }
}
