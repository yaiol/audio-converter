# Changelog

## 1.0.2 — 2026-10-15

- A command-line program, yac, installed beside the app, converts with the same saved presets, tag settings, effects and destinations as the window (-p, -t, -e, -d, -o, -j, --overwrite, --list, -q); the installer's last page offers, unticked, to add it to your PATH
- The equaliser is now ten vertical sliders with a centre line and a fill, drawn on one scale, instead of ten separate number fields
- The effects menu groups the effects under Time, Level and Sound, each in alphabetical order for the language in use
- In the effects editor a field's title no longer carries its unit: the unit is shown in plain text at the right of the field, in every language, and number fields are only as wide as their largest value needs
- A fade is limited to 9 999 ms, and a longer one saved earlier is shortened
- In a preset, the quality setting is a standard number field with its unit beside it, in steps the format allows, replacing the round knob
- Fixed a debug build of the app opening off screen after a previous session ended with its window minimized (this affects development runs only)

## 1.0.1 — 2026-10-15

- The preset, tags, effects, destination and if-exists choices now sit in a panel at the right of the window, with the number of conversions, Convert and the open-folder button at its foot; the header keeps only the file actions
- The number of conversions at once now stops at the physical cores of the machine, where it used to go up to the logical cores, and a saved higher value is lowered to match
- The built-in destination is now labelled "Source Folder" in all 52 languages
- The window can no longer be made smaller than what its options panel needs, with a width of at least 640

## 1.0.0 — 2026-10-15

- Initial release
