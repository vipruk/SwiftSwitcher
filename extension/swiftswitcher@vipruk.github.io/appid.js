// Pure app id helpers (no GNOME imports, so gjs can unit-test them).

// The id the daemon matches exceptions against: the desktop file id without
// ".desktop", or for a window-backed (or unknown) app the window's WM class.
export function appIdOf(id, isWindowBacked, wmClass) {
    if (id && !isWindowBacked)
        return id.replace(/\.desktop$/, '');
    return wmClass ?? '';
}

// Drops empty ids and duplicates, keeping the first occurrence's order.
export function uniqueAppIds(ids) {
    return [...new Set(ids.filter(id => id))];
}
