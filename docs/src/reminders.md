---
title: Reminders and the Focus Timer
description: Reminders to drink water, rest your eyes and stand up, and a focus timer, from a panel under the tray icon.
---

# Reminders and the Focus Timer

Zode can remind you to drink water, rest your eyes and stand up, and run a
focus timer (Pomodoro) that alternates work and breaks. Every interval is
yours to set. It is off by default.

## Turning it on

Click the Zode icon in the menu bar (macOS) or the system tray (Windows,
Linux) and switch **Zode Reminders** on, or set it in your settings file:

```json
{
  "reminders": { "enabled": true }
}
```

The panel that opens shows the focus timer and one card per reminder, each
with its own switch and interval. The panel never takes the keyboard, so
intervals change with the **−** and **+** buttons, five minutes at a time;
**Settings…** opens the full list, where any value from 1 to 480 minutes can
be typed in.

## The reminders

| Reminder    | Default          | What it asks                                             |
| ----------- | ---------------- | -------------------------------------------------------- |
| Drink water | every 45 minutes | Drink a glass of water.                                  |
| Eye break   | every 20 minutes | The 20-20-20 rule: look 6 m (20 ft) away for 20 seconds. |
| Stand up    | every 60 minutes | Stand up, stretch and move for a minute.                 |

Turning a reminder on does not start it: press its start button, and its ring
counts down to the next reminder. The stop button holds it until you start it again.

A reminder that comes due while you are away waits until you are back instead
of firing at an empty desk: after `idle_pause_minutes` (default 5) without any
input on the computer, due reminders are held. Linux cannot report that time,
so there reminders are never held for it. A machine that sleeps does not
fire the reminders it missed; their intervals start again when it wakes.

## The focus timer

Start it from the panel. A focus round (25 minutes) is followed by a short
break (5 minutes), and every fourth round by a long break (15 minutes). The
countdown shows on the status bar, and each change of phase is announced like
a reminder. Pause, Skip and Stop are in the panel.

## How reminders are shown

With `"style": "bubble"` (the default) a reminder opens as a small bubble under
the tray icon, with **Done** and **Snooze**. The bubble hides itself after 15
seconds; the reminder icon on the status bar stays colored until you answer it.

A bubble needs the tray icon to be seen. When it is not, the reminder comes as
an OS notification instead:

- macOS: behind a full-screen app, or with the menu bar set to hide.
- Windows: when the icon sits in the taskbar's overflow (the **^** button);
  pin it to the taskbar to get bubbles.
- Linux: Zode cannot place windows on Wayland, so reminders always come as
  notifications there. Stock GNOME has no tray at all without an extension such
  as AppIndicator; the status bar and notifications still work.

Set `"style": "system"` to always use OS notifications. Bubbles do not follow
the system's Focus or Do Not Disturb mode; notifications do.

The panel and the bubble take the app's own window background: `window_opacity`
and `window_vibrancy` make them as see-through as the editor's windows.

## Settings

```json
{
  "reminders": {
    "enabled": false,
    "style": "bubble",
    "idle_pause_minutes": 5,
    "snooze_minutes": 5,
    "water": { "enabled": true, "interval_minutes": 45 },
    "eye_break": { "enabled": true, "interval_minutes": 20 },
    "stand_up": { "enabled": true, "interval_minutes": 60 },
    "focus": {
      "focus_minutes": 25,
      "short_break_minutes": 5,
      "long_break_minutes": 15,
      "rounds_before_long_break": 4
    }
  },
  "tray_icon": true
}
```

Set `"tray_icon": false` to remove the icon from the menu bar or tray.
