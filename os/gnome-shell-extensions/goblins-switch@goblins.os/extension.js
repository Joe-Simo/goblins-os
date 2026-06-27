// Goblins OS Switch Control scanner overlay.
//
// The core service owns preference writes. This extension owns the session
// overlay and scanner state machine only. It is installed in the shell mode but
// stays inert unless org.goblins.os.a11y.switch-control enabled is true.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const SCHEMA_ID = 'org.goblins.os.a11y.switch-control';
const MOTION_FAST_MS = 140;
const PANEL_MARGIN = 32;
const MAX_ATSPI_DEPTH = 5;
const MAX_ATSPI_CHILDREN = 64;
const MAX_TARGETS = 48;
const MIN_TARGET_SIZE = 8;
const POINT_STEPS = 12;
const SCANNABLE_ROLE = /push button|button|toggle|check box|radio|menu item|menu|link|entry|text|combo|slider|spin|tab|table cell|list item/i;

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

function settingString(settings, key, fallback) {
    try {
        return settings.get_string(key) || fallback;
    } catch (_error) {
        return fallback;
    }
}

function settingInt(settings, key, fallback) {
    try {
        return settings.get_int(key);
    } catch (_error) {
        return fallback;
    }
}

function actorDestroy(actor) {
    if (!actor)
        return;
    try {
        Main.layoutManager.removeChrome(actor);
    } catch (_error) {
        // The actor may not have been added yet.
    }
    actor.destroy();
}

function eventKeySymbol(event) {
    try {
        return event.get_key_symbol();
    } catch (_error) {
        return 0;
    }
}

function safeAccessibleText(accessible, method, fallback) {
    try {
        const value = accessible?.[method]?.();
        return value ? String(value) : fallback;
    } catch (_error) {
        return fallback;
    }
}

function boundsFromExtents(extents) {
    if (!extents)
        return null;
    const x = Number(extents.x ?? extents[0] ?? 0);
    const y = Number(extents.y ?? extents[1] ?? 0);
    const width = Number(extents.width ?? extents[2] ?? 0);
    const height = Number(extents.height ?? extents[3] ?? 0);
    if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(width) || !Number.isFinite(height))
        return null;
    if (width < MIN_TARGET_SIZE || height < MIN_TARGET_SIZE)
        return null;
    return {x, y, width, height};
}

function intersectsMonitor(bounds, monitor) {
    if (!bounds || !monitor)
        return false;
    return bounds.x < monitor.x + monitor.width &&
        bounds.x + bounds.width > monitor.x &&
        bounds.y < monitor.y + monitor.height &&
        bounds.y + bounds.height > monitor.y;
}

export default class GoblinsSwitchControl extends Extension {
    enable() {
        this._signals = [];
        this._tickId = 0;
        this._targetLoadId = 0;
        this._scanTargets = [];
        this._scanIndex = 0;
        this._pointColumn = 5;
        this._pointRow = 5;
        this._fallbackDetail = '';
        this._loadingTargets = false;
        this._atspiPromise = null;
        this._atspiReady = false;

        try {
            this._settings = new Gio.Settings({schema_id: SCHEMA_ID});
        } catch (error) {
            logError(error, 'goblins-switch: Switch Control schema is not available');
            this._settings = null;
            return;
        }

        this._interfaceSettings = new Gio.Settings({schema_id: 'org.gnome.desktop.interface'});
        this._buildActors();

        for (const key of ['enabled', 'mode', 'scanning', 'auto-interval-ms', 'dwell-ms', 'debounce-ms']) {
            this._signals.push([
                this._settings,
                this._settings.connect(`changed::${key}`, () => this._sync()),
            ]);
        }
        this._signals.push([
            Main.layoutManager,
            Main.layoutManager.connect('monitors-changed', () => this._positionPanel()),
        ]);
        this._signals.push([
            global.stage,
            global.stage.connect('captured-event', (_actor, event) => this._capturedEvent(event)),
        ]);

        globalThis.goblinsSwitchControl = this;
        this._sync();
    }

    disable() {
        this._stopTick();
        for (const [actor, id] of this._signals)
            actor.disconnect(id);
        this._signals = [];

        if (globalThis.goblinsSwitchControl === this)
            delete globalThis.goblinsSwitchControl;

        actorDestroy(this._panel);
        actorDestroy(this._ring);
        actorDestroy(this._crosshairX);
        actorDestroy(this._crosshairY);

        this._panel = null;
        this._title = null;
        this._detail = null;
        this._ring = null;
        this._crosshairX = null;
        this._crosshairY = null;
        this._settings = null;
        this._interfaceSettings = null;
        this._scanTargets = [];
    }

    // Stable hook used by render-desktop.sh for real-pixel proof captures.
    showPointScanDemo() {
        if (!this._settings)
            return;
        this._settings.set_string('mode', 'point');
        this._settings.set_string('scanning', 'step');
        this._settings.set_boolean('enabled', true);
        this._stopTick();
        this._panel?.show();
        this._panel?.grab_key_focus();
        this._pointColumn = 6;
        this._pointRow = 4;
        this._enterPointScan('Point scan is active. Selection stays paused until pointer injection is verified.');
    }

    hide() {
        if (this._settings)
            this._settings.set_boolean('enabled', false);
        else
            this._stopScanner();
    }

    _buildActors() {
        this._panel = new St.BoxLayout({
            style_class: 'goblins-switch-panel',
            vertical: true,
            reactive: true,
            can_focus: true,
            visible: false,
        });
        this._title = new St.Label({
            style_class: 'goblins-switch-title',
            text: 'Switch Control',
        });
        this._detail = new St.Label({
            style_class: 'goblins-switch-detail',
            text: '',
        });
        this._panel.add_child(this._title);
        this._panel.add_child(this._detail);

        this._ring = new St.Widget({
            style_class: 'goblins-switch-highlight',
            reactive: false,
            visible: false,
        });
        this._crosshairX = new St.Widget({
            style_class: 'goblins-switch-crosshair-x',
            reactive: false,
            visible: false,
        });
        this._crosshairY = new St.Widget({
            style_class: 'goblins-switch-crosshair-y',
            reactive: false,
            visible: false,
        });

        Main.layoutManager.addChrome(this._ring, {affectsStruts: false, trackFullscreen: true});
        Main.layoutManager.addChrome(this._crosshairX, {affectsStruts: false, trackFullscreen: true});
        Main.layoutManager.addChrome(this._crosshairY, {affectsStruts: false, trackFullscreen: true});
        Main.layoutManager.addChrome(this._panel, {affectsStruts: false, trackFullscreen: true});
    }

    _sync() {
        if (!this._settings)
            return;
        if (!this._settings.get_boolean('enabled')) {
            this._stopScanner();
            return;
        }
        this._startScanner();
    }

    _startScanner() {
        this._panel.show();
        this._positionPanel();
        this._panel.grab_key_focus();
        this._fallbackDetail = '';

        const mode = settingString(this._settings, 'mode', 'item');
        if (mode === 'point') {
            this._enterPointScan('Point scan is active. Space selects the current point; Escape turns Switch Control off.');
        } else {
            this._enterItemScan();
        }
        this._restartTick();
    }

    _stopScanner() {
        this._targetLoadId += 1;
        this._stopTick();
        this._loadingTargets = false;
        this._scanTargets = [];
        this._ring?.hide();
        this._crosshairX?.hide();
        this._crosshairY?.hide();
        this._panel?.hide();
    }

    _enterItemScan() {
        this._scanIndex = 0;
        this._loadingTargets = true;
        this._ring?.hide();
        this._crosshairX?.hide();
        this._crosshairY?.hide();
        this._setStatus('Finding controls', 'Looking for scannable controls in the focused session window.');
        this._reloadItemTargets();
    }

    async _reloadItemTargets() {
        const loadId = ++this._targetLoadId;
        try {
            const targets = await this._collectAtspiTargets();
            if (loadId !== this._targetLoadId || !this._settings?.get_boolean('enabled'))
                return;
            this._loadingTargets = false;
            this._scanTargets = targets;
            this._scanIndex = 0;
            if (targets.length === 0) {
                this._enterPointScan('This window has no scannable controls - using point scan.');
                return;
            }
            this._showCurrentTarget();
        } catch (error) {
            logError(error, 'goblins-switch: AT-SPI target collection failed');
            if (loadId === this._targetLoadId) {
                this._loadingTargets = false;
                this._enterPointScan('Desktop control discovery is not ready - using point scan.');
            }
        }
    }

    _enterPointScan(detail) {
        this._targetLoadId += 1;
        this._loadingTargets = false;
        this._scanTargets = [];
        this._fallbackDetail = detail;
        this._ring?.hide();
        this._setStatus('Point scan', detail);
        this._showPoint();
    }

    async _collectAtspiTargets() {
        const Atspi = await this._loadAtspi();
        const targets = [];
        const desktopCount = Math.max(1, Number(Atspi.get_desktop_count?.() ?? 1));
        for (let i = 0; i < desktopCount && targets.length < MAX_TARGETS; i++) {
            let desktop = null;
            try {
                desktop = Atspi.get_desktop(i);
            } catch (_error) {
                continue;
            }
            this._walkAccessible(Atspi, desktop, 0, targets);
        }
        return targets.slice(0, MAX_TARGETS);
    }

    async _loadAtspi() {
        if (!this._atspiPromise) {
            this._atspiPromise = import('gi://Atspi').then(module => {
                const Atspi = module.default ?? module;
                Atspi.init?.();
                this._atspiReady = true;
                return Atspi;
            });
        }
        return this._atspiPromise;
    }

    _walkAccessible(Atspi, accessible, depth, targets) {
        if (!accessible || depth > MAX_ATSPI_DEPTH || targets.length >= MAX_TARGETS)
            return;

        const target = this._targetFromAccessible(Atspi, accessible);
        if (target)
            targets.push(target);

        let childCount = 0;
        try {
            childCount = Math.min(Number(accessible.get_child_count?.() ?? 0), MAX_ATSPI_CHILDREN);
        } catch (_error) {
            childCount = 0;
        }
        for (let i = 0; i < childCount && targets.length < MAX_TARGETS; i++) {
            try {
                this._walkAccessible(Atspi, accessible.get_child_at_index(i), depth + 1, targets);
            } catch (_error) {
                // Accessibility trees are live; stale nodes are ignored.
            }
        }
    }

    _targetFromAccessible(Atspi, accessible) {
        const role = safeAccessibleText(accessible, 'get_role_name', '');
        let action = null;
        try {
            action = accessible.get_action_iface?.() ?? null;
        } catch (_error) {
            action = null;
        }
        if (!action && !SCANNABLE_ROLE.test(role))
            return null;

        let component = null;
        let bounds = null;
        try {
            component = accessible.get_component_iface?.() ?? null;
            bounds = boundsFromExtents(component?.get_extents?.(Atspi.CoordType.SCREEN));
        } catch (_error) {
            bounds = null;
        }
        if (!bounds || !intersectsMonitor(bounds, Main.layoutManager.primaryMonitor))
            return null;

        const name = safeAccessibleText(accessible, 'get_name', '');
        const description = safeAccessibleText(accessible, 'get_description', '');
        const label = name || description || role || 'Control';
        return {label, role, bounds, action, component};
    }

    _capturedEvent(event) {
        if (!this._settings?.get_boolean('enabled'))
            return Clutter.EVENT_PROPAGATE;
        try {
            if (event.type() !== Clutter.EventType.KEY_PRESS)
                return Clutter.EVENT_PROPAGATE;
        } catch (_error) {
            return Clutter.EVENT_PROPAGATE;
        }

        const symbol = eventKeySymbol(event);
        if (symbol === Clutter.KEY_Escape) {
            this._settings.set_boolean('enabled', false);
            return Clutter.EVENT_STOP;
        }
        if (symbol === Clutter.KEY_space || symbol === Clutter.KEY_Return || symbol === Clutter.KEY_KP_Enter) {
            this._selectCurrent();
            return Clutter.EVENT_STOP;
        }
        if (symbol === Clutter.KEY_Tab || symbol === Clutter.KEY_Right || symbol === Clutter.KEY_Down) {
            this._advanceScan();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    _restartTick() {
        this._stopTick();
        if (settingString(this._settings, 'scanning', 'auto') !== 'auto')
            return;
        const interval = clamp(settingInt(this._settings, 'auto-interval-ms', 1200), 300, 5000);
        this._tickId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, interval, () => {
            this._advanceScan();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _stopTick() {
        if (!this._tickId)
            return;
        GLib.source_remove(this._tickId);
        this._tickId = 0;
    }

    _advanceScan() {
        if (!this._settings?.get_boolean('enabled'))
            return;
        if (this._loadingTargets)
            return;
        if (this._scanTargets.length > 0) {
            this._scanIndex = (this._scanIndex + 1) % this._scanTargets.length;
            this._showCurrentTarget();
        } else {
            this._pointColumn = (this._pointColumn + 1) % POINT_STEPS;
            if (this._pointColumn === 0)
                this._pointRow = (this._pointRow + 1) % POINT_STEPS;
            this._showPoint();
        }
    }

    _showCurrentTarget() {
        const target = this._scanTargets[this._scanIndex];
        if (!target) {
            this._enterPointScan('This window has no scannable controls - using point scan.');
            return;
        }
        this._crosshairX?.hide();
        this._crosshairY?.hide();
        this._ring?.show();
        this._ring.set_position(Math.round(target.bounds.x - 5), Math.round(target.bounds.y - 5));
        this._ring.set_size(Math.round(target.bounds.width + 10), Math.round(target.bounds.height + 10));
        this._animateActor(this._ring);
        this._setStatus(
            `Scanning ${this._scanIndex + 1} of ${this._scanTargets.length}`,
            `${target.label}. Space selects; Tab advances; Escape turns Switch Control off.`
        );
    }

    _showPoint() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const x = monitor.x + Math.round(((this._pointColumn + 1) / (POINT_STEPS + 1)) * monitor.width);
        const y = monitor.y + Math.round(((this._pointRow + 1) / (POINT_STEPS + 1)) * monitor.height);
        this._crosshairX?.show();
        this._crosshairY?.show();
        this._crosshairX.set_position(monitor.x, y);
        this._crosshairX.set_size(monitor.width, 2);
        this._crosshairY.set_position(x, monitor.y);
        this._crosshairY.set_size(2, monitor.height);
        this._animateActor(this._crosshairX);
        this._animateActor(this._crosshairY);
        this._setStatus('Point scan', `${this._fallbackDetail || 'Point scan is active.'} Space is held until qemu proves gated pointer injection.`);
    }

    _selectCurrent() {
        if (this._scanTargets.length === 0) {
            this._setStatus('Selection paused', 'Point selection needs live qemu proof before pointer injection is enabled.');
            return;
        }

        const target = this._scanTargets[this._scanIndex];
        if (!target) {
            this._setStatus('Selection paused', 'The current control disappeared before selection.');
            return;
        }
        try {
            if (target.action?.do_action?.(0)) {
                this._setStatus('Selected', `${target.label} accepted the primary accessibility action.`);
                return;
            }
        } catch (_error) {
            // Fall through to focus attempt.
        }
        try {
            if (target.component?.grab_focus?.()) {
                this._setStatus('Focused', `${target.label} accepted focus. Selection is paused on this screen.`);
                return;
            }
        } catch (_error) {
            // Fall through to honest failure.
        }
        this._setStatus('Selection paused', 'Selection is paused on this screen.');
    }

    _setStatus(title, detail) {
        this._title?.set_text(title);
        this._detail?.set_text(detail);
        this._positionPanel();
    }

    _positionPanel() {
        if (!this._panel?.visible)
            return;
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const [width, height] = this._panel.get_size();
        this._panel.set_position(
            monitor.x + Math.max(PANEL_MARGIN, Math.round((monitor.width - width) / 2)),
            monitor.y + monitor.height - height - PANEL_MARGIN
        );
    }

    _animateActor(actor) {
        if (!actor)
            return;
        if (!this._animationsEnabled()) {
            actor.set_opacity(255);
            return;
        }
        actor.set_opacity(180);
        actor.ease({
            opacity: 255,
            duration: MOTION_FAST_MS,
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
}
