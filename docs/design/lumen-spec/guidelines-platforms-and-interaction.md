# Platform and interaction guidelines

## Shared first

Now Playing and other product surfaces use the same layout, type scale, brand color, source tags, and behavior across platforms. Implement them as shared components. Platform selection should change only genuine chrome tokens.

## Platform tokens

| Concern | iOS | Android | Windows | macOS |
| --- | --- | --- | --- | --- |
| Shell radius | 30 | 34 | 8 | 12 |
| Primary play shape | Circle | M3 squircle | Circle | Circle |
| Navigation | Tab bar | Pill bottom nav | Left pane | Left pane |
| Title chrome | Grabber sheet | App bar | Caption buttons | Traffic lights |
| Material | Opaque | Opaque | Mica | Vibrancy |
| Body font | SF Pro | Roboto | Segoe UI | System body |
| Motion | Smooth ease | Spring with slight overshoot | Restrained/native | Restrained/native |

Schibsted Grotesk headlines, amber semantics, content layout, type scale, and source identity do not change.

## Back navigation

The desktop Back component changes only its presentation:

- Windows: full arrow, 6 px rounded rectangle, positioned left of the wordmark and excluded from the drag region; shortcut `Alt+Left`.
- macOS: chevron in a round glass toolbar button, positioned right of the traffic lights; shortcut `Command+[`.
- Mobile: do not add this desktop component. Use iOS edge-swipe and Android system back gestures.

## Mobile swipe grammar

- Swipe right means **queue/play next** anywhere a track appears.
- Swipe left means the contextual action for the current surface; in queue it removes.
- Reveal color and icon before commit.
- Commit threshold is 40% of row width with a haptic tick.
- A partial swipe reveals a tappable action; a full swipe commits directly.
- Destructive reveal panels use ember.

## Pointer, press, and drag

- Mobile track-row tap plays; long-press opens actions. Desktop click selects, double-click plays, and right-click opens actions.
- Desktop selection follows file-browser conventions: Shift-click extends a range and Ctrl/Cmd-click toggles one row. Context actions apply to the complete selection.
- Desktop drag begins by click-dragging the row itself and shows no handle.
- Mobile queue and edit surfaces expose a drag handle; a 250 ms long-press can also lift a row before drag.
- A lifted row scales to `1.015` and adds elevation.
- Cards lift 2 px on desktop hover.
- Primary controls press to `0.96` over 120 ms.

## Plugin boundaries

Plugins declare feeds, charts, profiles, genre hubs, radio, and other views as pure data. They return the same result rows used across the product, optional social counts such as follows, likes, and reposts, supported actions, and opaque handles for follow-on views. Plugins ship no UI. Ristle owns rows, cards, social elements, playback behavior, menus, source identity, lock and empty states, and application chrome.

On desktop, provider spaces nest beneath their source in the sidebar. On mobile, they surface as cards on Home. Conditional artist modules render only when the owning provider exposes the necessary data.

## Plugin installation and authorization

- Group plugin candidates by provider and rank maintained candidates by cached GitHub stars. Show archived status and last update, and never query GitHub live while rendering the list.
- Verify every file in a `.ristleplugin` archive against its checksum manifest before staging any part of the plugin.
- Open provider sign-in in the system browser through an authorization session. Passwords and other credentials never pass through Ristle or the plugin; the plugin receives tokens only.
- Represent waiting, connected, unfinished, and expired sessions explicitly. Reconnect uses the same browser authorization flow.
- While a source is disconnected, use a hollow source dot, keep its cached library browsable, and show its tracks with the shared Unavailable row state.
