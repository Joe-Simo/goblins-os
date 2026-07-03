// Goblins OS menu bar - the system mark at the far left of the top panel, a
// direct Goblins AI affordance, and a control-center button at the right. The
// mark is a non-interactive brand anchor; the AI and control-center buttons open
// OS-owned native surfaces.

import St from 'gi://St';
import Gio from 'gi://Gio';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

// The mark is a path-loaded non-symbolic SVG (CSS color can't recolor it), so we
// switch the gicon by scheme: white on the dark glass panel, ink on the light
// frosted panel. The control icon IS symbolic, so it recolors via CSS color.
const MARK_DARK = '/usr/share/goblins-os/brand/Goblins-white-mark.svg';
const MARK_LIGHT = '/usr/share/goblins-os/brand/Goblins-black-mark.svg';
const AI_ICON = '/usr/share/icons/GoblinsOS/scalable/actions/goblins-engine-symbolic.svg';
const CONTROL_ICON = '/usr/share/goblins-os/brand/icons/control-center-symbolic.svg';
const LAUNCHER = '/usr/libexec/goblins-os/goblins-os-launcher';
const SCREENSHOT_CONTEXT = '/usr/libexec/goblins-os/goblins-os-screenshot-context';
const CONTROL_CENTER = '/usr/libexec/goblins-os/goblins-os-control-center';
const SETTINGS = '/usr/libexec/goblins-os/goblins-os-settings';
const TODAY = '/usr/libexec/goblins-os/goblins-os-today';
const INPUT_SOURCES_SCHEMA = 'org.gnome.desktop.input-sources';
const FOCUS_SCHEMA = 'org.goblins.os.focus';
// Color-only overlay applied on top of the dark gnome-shell.css base when the
// desktop color-scheme is light, giving Goblins adaptive light/dark chrome.
const LIGHT_CHROME_CSS = '/usr/share/themes/GoblinsOS/gnome-shell/gnome-shell-light.css';

export default class GoblinsMenuBar extends Extension {
    enable() {
        // Left: the system mark + wordmark.
        this._mark = new PanelMenu.Button(0.0, 'Goblins OS', true);
        const box = new St.BoxLayout({style_class: 'goblins-menubar'});
        this._markIcon = new St.Icon({
            gicon: Gio.icon_new_for_string(MARK_DARK),
            style_class: 'goblins-menubar-mark',
        });
        box.add_child(this._markIcon);
        box.add_child(new St.Label({
            text: 'Goblins OS',
            style_class: 'goblins-menubar-name',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        this._mark.add_child(box);
        // Position 0 in the left box: the very first item on the menu bar.
        Main.panel.addToStatusArea('goblins-mark', this._mark, 0, 'left');

        // Right: Goblins AI, Today, Focus (only when active), input source
        // (only when there are multiple sources), then Control Center. The AI
        // button is a compact command menu so system-wide assistant actions
        // stay one menu-bar click away without crowding the top panel.
        this._ai = new PanelMenu.Button(0.0, 'Goblins AI');
        this._ai.add_child(new St.Icon({
            gicon: Gio.icon_new_for_string(AI_ICON),
            style_class: 'goblins-ai-icon',
        }));
        this._addAiMenuItem('Ask Goblin', [LAUNCHER, '--assistant']);
        this._addAiMenuItem('Ask About Selected Text', [LAUNCHER, '--selected-text']);
        this._addAiMenuItem('Write with Goblin', [LAUNCHER, '--writing-tools']);
        this._addScreenContextMenuItem();
        this._addVisualContextMenuItem();
        this._ai.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._addAiMenuItem('Goblins AI Settings', [SETTINGS, '--panel=models']);
        Main.panel.addToStatusArea('goblins-ai', this._ai, 2, 'right');

        this._interfaceSettings = new Gio.Settings({schema_id: 'org.gnome.desktop.interface'});

        this._today = new PanelMenu.Button(0.0, 'Today', true);
        this._todayLabel = new St.Label({
            text: 'Today',
            style_class: 'goblins-date-indicator',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._today.add_child(this._todayLabel);
        this._today.connect('button-press-event', () => {
            this._openToday();
            return Clutter.EVENT_STOP;
        });
        this._today.connect('touch-event', (_actor, event) => {
            if (event.type() === Clutter.EventType.TOUCH_BEGIN) {
                this._openToday();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        Main.panel.addToStatusArea('goblins-today', this._today, 1, 'right');
        this._bindTodayClock();

        this._inputSource = new PanelMenu.Button(0.0, 'Input Source', true);
        this._inputSourceLabel = new St.Label({
            text: '',
            style_class: 'goblins-input-source-indicator',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._inputSource.add_child(this._inputSourceLabel);
        this._inputSource.hide();
        Main.panel.addToStatusArea('goblins-input-source', this._inputSource, 3, 'right');
        this._bindInputSourceIndicator();

        this._focus = new PanelMenu.Button(0.0, 'Focus', true);
        this._focusLabel = new St.Label({
            text: '',
            style_class: 'goblins-focus-indicator',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._focus.add_child(this._focusLabel);
        this._focus.connect('button-press-event', () => {
            this._openFocusSettings();
            return Clutter.EVENT_STOP;
        });
        this._focus.connect('touch-event', (_actor, event) => {
            if (event.type() === Clutter.EventType.TOUCH_BEGIN) {
                this._openFocusSettings();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        this._focus.hide();
        Main.panel.addToStatusArea('goblins-focus', this._focus, 4, 'right');
        this._bindFocusIndicator();

        this._control = new PanelMenu.Button(0.0, 'Control Center', true);
        this._control.add_child(new St.Icon({
            gicon: Gio.icon_new_for_string(CONTROL_ICON),
            style_class: 'goblins-control-icon',
        }));
        this._control.connect('button-press-event', () => {
            this._openControlCenter();
            return Clutter.EVENT_STOP;
        });
        this._control.connect('touch-event', (_actor, event) => {
            if (event.type() === Clutter.EventType.TOUCH_BEGIN) {
                this._openControlCenter();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        // Position 0 in the right box: nearest the screen edge.
        Main.panel.addToStatusArea('goblins-control', this._control, 0, 'right');

        // Adaptive chrome: in light mode the menu bar, popovers, and control
        // center read as Goblins light frosted glass; in dark mode they stay
        // dark glass. The dark gnome-shell.css is the always-loaded base (geometry
        // + dark colors); we overlay the light color sheet on top only in light
        // mode and unload it in dark. If anything here fails, the dark base remains
        // — no half-styled mix, no regression.
        this._lightChromeFile = Gio.File.new_for_path(LIGHT_CHROME_CSS);
        this._lightChromeLoaded = false;
        this._schemeChangedId = this._interfaceSettings.connect(
            'changed::color-scheme',
            () => this._applySchemeChrome()
        );
        // user-theme loads gnome-shell.css onto a fresh St.Theme that may replace
        // ours after we enable; each theme swap emits ThemeContext::changed and
        // drops our overlay with the old theme, so reset and re-apply on every swap.
        this._themeContext = St.ThemeContext.get_for_stage(global.stage);
        this._themeChangedId = this._themeContext.connect('changed', () => {
            this._lightChromeLoaded = false;
            this._applySchemeChrome();
        });
        this._applySchemeChrome();
    }

    _applySchemeChrome() {
        try {
            const theme = this._themeContext?.get_theme();
            if (!theme)
                return;
            const isLight =
                this._interfaceSettings.get_string('color-scheme') !== 'prefer-dark';
            // The mark is non-symbolic, so swap the gicon by scheme: ink on the
            // light frosted panel, white on the dark glass panel.
            this._markIcon?.set_gicon(
                Gio.icon_new_for_string(isLight ? MARK_LIGHT : MARK_DARK)
            );
            if (isLight && !this._lightChromeLoaded && this._lightChromeFile.query_exists(null)) {
                theme.load_stylesheet(this._lightChromeFile);
                this._lightChromeLoaded = true;
            } else if (!isLight && this._lightChromeLoaded) {
                theme.unload_stylesheet(this._lightChromeFile);
                this._lightChromeLoaded = false;
            }
        } catch (error) {
            logError(error, 'goblins-menubar: failed to apply adaptive chrome stylesheet');
        }
    }

    _bindTodayClock() {
        try {
            this._todayClockChangedIds = [
                this._interfaceSettings.connect(
                    'changed::clock-format',
                    () => this._restartTodayClock()
                ),
                this._interfaceSettings.connect(
                    'changed::clock-show-weekday',
                    () => this._restartTodayClock()
                ),
                this._interfaceSettings.connect(
                    'changed::clock-show-seconds',
                    () => this._restartTodayClock()
                ),
            ];
            this._restartTodayClock();
        } catch (error) {
            this._todayLabel?.set_text('Today');
            logError(error, 'goblins-menubar: Today clock settings unavailable');
        }
    }

    _restartTodayClock() {
        this._clearTodayClockTimer();
        this._refreshTodayClock();
        const intervalSeconds = this._todayClockShowsSeconds() ? 1 : 30;
        this._todayClockTimeoutId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            intervalSeconds,
            () => {
                this._refreshTodayClock();
                return GLib.SOURCE_CONTINUE;
            }
        );
    }

    _clearTodayClockTimer() {
        if (this._todayClockTimeoutId) {
            GLib.source_remove(this._todayClockTimeoutId);
            this._todayClockTimeoutId = null;
        }
    }

    _todayClockShowsSeconds() {
        try {
            return this._interfaceSettings.get_boolean('clock-show-seconds');
        } catch (error) {
            logError(error, 'goblins-menubar: failed to read clock seconds setting');
        }
        return false;
    }

    _refreshTodayClock() {
        try {
            this._todayLabel?.set_text(this._todayClockLabel());
        } catch (error) {
            this._todayLabel?.set_text('Today');
            logError(error, 'goblins-menubar: failed to refresh Today clock');
        }
    }

    _todayClockLabel() {
        const clockFormat = this._interfaceSettings.get_string('clock-format');
        const showWeekday = this._interfaceSettings.get_boolean('clock-show-weekday');
        const showSeconds = this._interfaceSettings.get_boolean('clock-show-seconds');
        const timePattern = clockFormat === '24h'
            ? (showSeconds ? '%H:%M:%S' : '%H:%M')
            : (showSeconds ? '%I:%M:%S %p' : '%I:%M %p');
        const weekdayPrefix = showWeekday ? '%a ' : '';
        const formatted = GLib.DateTime.new_now_local().format(`${weekdayPrefix}${timePattern}`) || 'Today';
        return this._safeTodayClockLabel(formatted, clockFormat === '24h' ? 14 : 18);
    }

    _safeTodayClockLabel(value, maxChars) {
        const text = String(value || '')
            .replace(/[\r\n\t]+/g, ' ')
            .replace(/\s+/g, ' ')
            .replace(/\b0([1-9]:)/, '$1')
            .trim();
        if (!text)
            return 'Today';
        if (text.length <= maxChars)
            return text;
        return `${text.slice(0, Math.max(1, maxChars - 3))}...`;
    }

    _bindInputSourceIndicator() {
        try {
            this._inputSourceSettings = new Gio.Settings({schema_id: INPUT_SOURCES_SCHEMA});
            this._inputSourcesChangedId = this._inputSourceSettings.connect(
                'changed::sources',
                () => this._refreshInputSourceIndicator()
            );
            this._inputSourceCurrentChangedId = this._inputSourceSettings.connect(
                'changed::current',
                () => this._refreshInputSourceIndicator()
            );
            this._refreshInputSourceIndicator();
        } catch (error) {
            this._setInputSourceIndicator('', false);
            logError(error, 'goblins-menubar: input source indicator unavailable');
        }
    }

    _refreshInputSourceIndicator() {
        if (!this._inputSourceSettings) {
            this._setInputSourceIndicator('', false);
            return;
        }

        try {
            const sources = this._readInputSources();
            if (sources.length <= 1) {
                this._setInputSourceIndicator('', false);
                return;
            }

            const current = this._currentInputSourceIndex(sources.length);
            if (current === null) {
                this._setInputSourceIndicator('', false);
                return;
            }

            const source = sources[current];
            this._setInputSourceIndicator(
                this._inputSourceAbbreviation(source.kind, source.id),
                true
            );
        } catch (error) {
            this._setInputSourceIndicator('', false);
            logError(error, 'goblins-menubar: failed to refresh input source indicator');
        }
    }

    _readInputSources() {
        const variant = this._inputSourceSettings.get_value('sources');
        const unpacked = typeof variant?.deep_unpack === 'function'
            ? variant.deep_unpack()
            : [];
        if (!Array.isArray(unpacked))
            return [];

        const sources = [];
        for (const entry of unpacked) {
            const normalized = this._normalizeInputSourceEntry(entry);
            if (normalized)
                sources.push(normalized);
        }
        return sources;
    }

    _normalizeInputSourceEntry(entry) {
        const pair = entry && typeof entry.deep_unpack === 'function'
            ? entry.deep_unpack()
            : entry;
        if (!Array.isArray(pair) || pair.length < 2)
            return null;

        const kind = this._safeInputSourceToken(pair[0]);
        const id = this._safeInputSourceToken(pair[1]);
        if (!kind || !id)
            return null;
        return {kind, id};
    }

    _currentInputSourceIndex(sourceCount) {
        try {
            const current = this._inputSourceSettings.get_uint('current');
            if (Number.isInteger(current) && current >= 0 && current < sourceCount)
                return current;
        } catch (error) {
            logError(error, 'goblins-menubar: failed to read current input source');
        }
        return null;
    }

    _setInputSourceIndicator(text, visible) {
        this._inputSourceLabel?.set_text(text);
        if (visible)
            this._inputSource?.show();
        else
            this._inputSource?.hide();
    }

    _inputSourceAbbreviation(kind, id) {
        const normalizedKind = kind.toLowerCase();
        const normalizedId = id.toLowerCase();
        if (normalizedKind === 'xkb')
            return this._layoutAbbreviation(normalizedId);

        if (normalizedKind === 'ibus') {
            if (normalizedId === 'libpinyin' || normalizedId === 'pinyin')
                return 'PY';
            if (normalizedId === 'anthy' || normalizedId === 'mozc')
                return 'JP';
            if (normalizedId === 'hangul')
                return 'KO';
            return this._compactInputSourceCode(normalizedId);
        }

        return this._compactInputSourceCode(normalizedId || normalizedKind);
    }

    _layoutAbbreviation(id) {
        if (id === 'us')
            return 'US';
        if (id === 'gb')
            return 'GB';
        return this._compactInputSourceCode(id);
    }

    _compactInputSourceCode(value) {
        const cleaned = String(value || '')
            .replace(/^.*:/, '')
            .replace(/[^a-z0-9]+/gi, ' ')
            .trim();
        const token = cleaned.split(/\s+/)[0] || 'IM';
        return token.slice(0, 3).toUpperCase();
    }

    _safeInputSourceToken(value) {
        return String(value || '')
            .replace(/[\r\n\t]+/g, ' ')
            .replace(/\s+/g, ' ')
            .trim()
            .slice(0, 64);
    }

    _bindFocusIndicator() {
        try {
            this._focusSettings = new Gio.Settings({schema_id: FOCUS_SCHEMA});
            this._focusActiveChangedId = this._focusSettings.connect(
                'changed::active-mode',
                () => this._refreshFocusIndicator()
            );
            this._focusModesChangedId = this._focusSettings.connect(
                'changed::modes',
                () => this._refreshFocusIndicator()
            );
            this._focusScheduledChangedId = this._focusSettings.connect(
                'changed::armed-by-schedule',
                () => this._refreshFocusIndicator()
            );
            this._refreshFocusIndicator();
        } catch (error) {
            this._setFocusIndicator('', false);
            logError(error, 'goblins-menubar: Focus indicator unavailable');
        }
    }

    _refreshFocusIndicator() {
        if (!this._focusSettings) {
            this._setFocusIndicator('', false);
            return;
        }

        try {
            const activeMode = this._safeFocusToken(this._focusSettings.get_string('active-mode'));
            if (!activeMode) {
                this._setFocusIndicator('', false);
                return;
            }

            const modes = this._readFocusModes();
            const mode = modes.find(entry => entry.id === activeMode);
            if (!mode) {
                this._setFocusIndicator('', false);
                return;
            }

            this._setFocusIndicator(this._focusIndicatorText(mode.name), true);
        } catch (error) {
            this._setFocusIndicator('', false);
            logError(error, 'goblins-menubar: failed to refresh Focus indicator');
        }
    }

    _readFocusModes() {
        const raw = this._focusSettings.get_string('modes');
        const parsed = JSON.parse(raw || '[]');
        if (!Array.isArray(parsed))
            return [];

        const modes = [];
        for (const entry of parsed) {
            const mode = this._normalizeFocusMode(entry);
            if (mode)
                modes.push(mode);
        }
        return modes;
    }

    _normalizeFocusMode(entry) {
        if (!entry || typeof entry !== 'object' || Array.isArray(entry))
            return null;

        const id = this._safeFocusToken(entry.id);
        const name = this._safeFocusLabel(entry.name, '', 80);
        if (!id || !name)
            return null;
        return {id, name};
    }

    _setFocusIndicator(text, visible) {
        this._focusLabel?.set_text(text);
        if (visible)
            this._focus?.show();
        else
            this._focus?.hide();
    }

    _focusIndicatorText(name) {
        return this._safeFocusLabel(name, 'Focus', 14);
    }

    _safeFocusToken(value) {
        return String(value || '')
            .replace(/[^A-Za-z0-9._:-]+/g, '')
            .slice(0, 64);
    }

    _safeFocusLabel(value, fallback, maxChars) {
        const text = String(value || '')
            .replace(/[\r\n\t]+/g, ' ')
            .replace(/\s+/g, ' ')
            .trim();
        if (!text)
            return fallback;
        if (text.length <= maxChars)
            return text;
        return `${text.slice(0, Math.max(1, maxChars - 3))}...`;
    }

    _addAiMenuItem(label, argv) {
        const item = new PopupMenu.PopupMenuItem(label);
        item.connect('activate', () => this._spawn(argv, 'goblins-menubar: failed to open Goblins AI action'));
        this._ai.menu.addMenuItem(item);
    }

    _addScreenContextMenuItem() {
        const item = new PopupMenu.PopupMenuItem('Summarize Screen Context');
        item.connect('activate', () => this._openScreenContext());
        this._ai.menu.addMenuItem(item);
    }

    _addVisualContextMenuItem() {
        const item = new PopupMenu.PopupMenuItem('Ask About Screenshot');
        item.connect('activate', () => this._openVisualContext());
        this._ai.menu.addMenuItem(item);
    }

    _spawn(argv, errorMessage) {
        try {
            Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
        } catch (error) {
            logError(error, errorMessage);
        }
    }

    _spawnWithEnv(argv, env, errorMessage) {
        try {
            const launcher = new Gio.SubprocessLauncher({flags: Gio.SubprocessFlags.NONE});
            for (const [key, value] of Object.entries(env))
                launcher.setenv(key, value, true);
            launcher.spawnv(argv);
        } catch (error) {
            logError(error, errorMessage);
        }
    }

    _openScreenContext() {
        const context = this._activeWindowContext();
        this._spawnWithEnv([LAUNCHER, '--screen-context'], {
            GOBLINS_OS_SCREEN_CONTEXT_SOURCE: 'menubar-screen-context',
            GOBLINS_OS_SCREEN_CONTEXT_TEXT: context.visibleText,
            GOBLINS_OS_CONTEXT_APP: context.app,
            GOBLINS_OS_CONTEXT_WINDOW_TITLE: context.windowTitle,
        }, 'goblins-menubar: failed to open screen context');
    }

    _openVisualContext() {
        const context = this._activeWindowContext();
        this._spawnWithEnv([SCREENSHOT_CONTEXT], {
            GOBLINS_OS_SCREEN_CONTEXT_SOURCE: 'menubar-screenshot-context',
            GOBLINS_OS_CONTEXT_APP: context.app,
            GOBLINS_OS_CONTEXT_WINDOW_TITLE: context.windowTitle,
        }, 'goblins-menubar: failed to open screenshot context');
    }

    _openControlCenter() {
        this._spawn([CONTROL_CENTER], 'goblins-menubar: failed to open control center');
    }

    _openToday() {
        this._spawn([TODAY], 'goblins-menubar: failed to open Today');
    }

    _openFocusSettings() {
        this._spawn([SETTINGS, '--panel=notifications'], 'goblins-menubar: failed to open Focus settings');
    }

    _activeWindowContext() {
        const win = global.display?.focus_window || null;
        const windowTitle = this._safeContextValue(win?.get_title?.(), 'Active window', 180);
        let app = 'Current app';
        try {
            const tracked = win ? Shell.WindowTracker.get_default().get_window_app(win) : null;
            app = this._safeContextValue(tracked?.get_name?.(), app, 120);
        } catch (error) {
            logError(error, 'goblins-menubar: failed to read active app context');
        }

        const visibleText = windowTitle === 'Active window'
            ? 'User requested screen context from the Goblins OS menu bar. No screen content was captured automatically.'
            : `Active window: ${windowTitle}`;
        return {app, windowTitle, visibleText};
    }

    _safeContextValue(value, fallback, maxChars) {
        const text = String(value || '')
            .replace(/[\r\n\t]+/g, ' ')
            .replace(/\s+/g, ' ')
            .trim()
            .slice(0, maxChars);
        return text || fallback;
    }

    disable() {
        if (this._themeChangedId && this._themeContext) {
            this._themeContext.disconnect(this._themeChangedId);
            this._themeChangedId = null;
        }
        if (this._schemeChangedId && this._interfaceSettings) {
            this._interfaceSettings.disconnect(this._schemeChangedId);
            this._schemeChangedId = null;
        }
        if (this._todayClockChangedIds && this._interfaceSettings) {
            for (const id of this._todayClockChangedIds)
                this._interfaceSettings.disconnect(id);
            this._todayClockChangedIds = null;
        }
        this._clearTodayClockTimer();
        if (this._inputSourcesChangedId && this._inputSourceSettings) {
            this._inputSourceSettings.disconnect(this._inputSourcesChangedId);
            this._inputSourcesChangedId = null;
        }
        if (this._inputSourceCurrentChangedId && this._inputSourceSettings) {
            this._inputSourceSettings.disconnect(this._inputSourceCurrentChangedId);
            this._inputSourceCurrentChangedId = null;
        }
        if (this._focusActiveChangedId && this._focusSettings) {
            this._focusSettings.disconnect(this._focusActiveChangedId);
            this._focusActiveChangedId = null;
        }
        if (this._focusModesChangedId && this._focusSettings) {
            this._focusSettings.disconnect(this._focusModesChangedId);
            this._focusModesChangedId = null;
        }
        if (this._focusScheduledChangedId && this._focusSettings) {
            this._focusSettings.disconnect(this._focusScheduledChangedId);
            this._focusScheduledChangedId = null;
        }
        if (this._lightChromeLoaded && this._lightChromeFile) {
            try {
                const theme = this._themeContext?.get_theme();
                theme?.unload_stylesheet(this._lightChromeFile);
            } catch (error) {
                logError(error, 'goblins-menubar: failed to unload adaptive chrome stylesheet');
            }
            this._lightChromeLoaded = false;
        }
        this._themeContext = null;
        this._interfaceSettings = null;
        this._inputSourceSettings = null;
        this._focusSettings = null;
        this._lightChromeFile = null;
        if (this._mark) {
            this._mark.destroy();
            this._mark = null;
        }
        this._markIcon = null;
        if (this._control) {
            this._control.destroy();
            this._control = null;
        }
        if (this._ai) {
            this._ai.destroy();
            this._ai = null;
        }
        if (this._today) {
            this._today.destroy();
            this._today = null;
        }
        this._todayLabel = null;
        if (this._inputSource) {
            this._inputSource.destroy();
            this._inputSource = null;
        }
        this._inputSourceLabel = null;
        if (this._focus) {
            this._focus.destroy();
            this._focus = null;
        }
        this._focusLabel = null;
    }
}
