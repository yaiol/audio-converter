# Changelog

## 1.5.0 — 2026-10-27

- ffmpeg is no longer shipped with the app or taken from the machine's PATH: the app downloads the build yaiol approves into the folder shared by all yaiol apps and checks it first; Settings has a new ffmpeg section with the installed version, the latest version and Update, and a first run without ffmpeg offers Download ffmpeg with a progress readout
- The yac command line reports a clear message when ffmpeg is not installed yet
- The effects are now built from ffmpeg's own filter list, so far more effects are available; the effects menu has an All category and buttons to open or close every effect at once
- Effect fields are generated from each filter's parameters: time and frame-rate fields accept values such as 1:30 or 30000/1001, colours have a picker, files and folders are chosen from a dialog, each field has a reset to default, and an effect can be dragged to change when it runs
- The hand-built effect editors (silence trimming, volume, fades, channels, equaliser, dynamics, tempo and the rest) are replaced by the generated effects
- The effect and ffmpeg texts are updated in all 52 languages

## 1.0.3 — 2026-10-23

- Per-file settings: select one or more files in the list and press Apply to give them the preset, tags setting, effects setting and destination currently chosen in the panel; a file keeps the settings it was given, and the footer shows how many files are selected
- Timed lyrics held in an ID3 SYLT frame (MP3, AIFF, WAV) were silently lost on conversion and now carry over as .lrc text; the tag-cleaning entries gain lyrics options: embed the .lrc file that sits beside the source (same name) as the lyrics, replacing any the source has; choose the frame written (automatic by the text, both, always USLT, always SYLT); remove the times when writing USLT
- A relative destination folder such as convert, ./convert or ../convert is now counted from each source file's own folder, so one entry serves files in many folders; the folder structure is not kept then
- No tag value is lost when a source holds two spellings of one field (PUBLISHER and ORGANIZATION with different values): the second stays as its own free-form field
- A date with a stray carriage return no longer gives an MP3 two year frames
- A newer version now appears as a card in a news dialog instead of a banner strip: the card lists the release notes newer than the running version, links to the download page, and Don't show again hides it until a still newer version is out; occasional yaiol announcements appear in the same dialog, and Settings > General has a new Show news from yaiol switch that turns them off completely (the new-version card is not governed by it); the Display tab is now called General

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
