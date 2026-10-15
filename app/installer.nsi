; The installer script `dx bundle` renders (Dioxus.toml → [bundle.windows.nsis] template).
;
; It is dx's built-in script with four changes, and nothing else:
;   - the name shown in Settings > Apps is "yaiol <name>" (like the electron installers), where dx
;     would show its PascalCased executable name ("AudioConverter");
;   - the uninstall entry carries an icon (DisplayIcon) - dx writes none, so Settings showed a blank;
;   - the Start Menu / Desktop shortcuts are named with the real display name, spaces included;
;   - the last page offers (UNTICKED) to put the install folder on the user's PATH, so the command line
;     `yac` that sits beside the window runs from any terminal. Silent installs never add it.
;
; ⚠ CLAUDE: the display name is the one literal below. It is identity, so it is written from the
; birth certificate by the cockpit's app-info (and by the scaffolder) - never edit it by hand.
; Everything in double curly braces is dx's own template syntax, filled in at bundle time.
!define PRODUCT_DISPLAY "Audio Converter"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

; Basic installer attributes
Name "${PRODUCT_DISPLAY}"
OutFile "{{output_path}}"
Unicode true
; Same default folder as every yaiol electron app: Program Files\yaiol\<Product Name>. The installer
; asks for administrator in every mode but per-user, so the folder is writable; the user can still
; pick another one and the cockpit finds it through the uninstall entry's InstallLocation.
InstallDir "$PROGRAMFILES64\yaiol\${PRODUCT_DISPLAY}"

; Request appropriate privileges
{{#if install_mode_per_machine}}
RequestExecutionLevel admin
{{else if install_mode_both}}
RequestExecutionLevel admin
{{else}}
RequestExecutionLevel user
{{/if}}

; Version information
VIProductVersion "{{version}}.0"
VIAddVersionKey "ProductName" "${PRODUCT_DISPLAY}"
VIAddVersionKey "FileVersion" "{{version}}"
VIAddVersionKey "ProductVersion" "{{version}}"
VIAddVersionKey "FileDescription" "{{short_description}}"
{{#if publisher}}
VIAddVersionKey "CompanyName" "{{publisher}}"
{{/if}}
{{#if copyright}}
VIAddVersionKey "LegalCopyright" "{{copyright}}"
{{/if}}

; MUI settings
!define MUI_ABORTWARNING
{{#if installer_icon}}
!define MUI_ICON "{{installer_icon}}"
{{/if}}
{{#if header_image}}
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "{{header_image}}"
{{/if}}
{{#if sidebar_image}}
!define MUI_WELCOMEFINISHPAGE_BITMAP "{{sidebar_image}}"
{{/if}}

; Pages
{{#if license}}
!insertmacro MUI_PAGE_LICENSE "{{license}}"
{{/if}}
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; The finish page's one checkbox is the PATH option (the "show readme" slot, repurposed): unticked by
; default, because changing someone's PATH is not something an installer does unasked. The command
; takes effect in terminals opened afterwards.
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Add the yac command line to the PATH"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION AddToUserPath
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Language
!insertmacro MUI_LANGUAGE "English"
{{#each additional_languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}

; The user's own PATH (never the machine's): one entry, added once, removed on uninstall.
; PowerShell does the edit because NSIS has no string functions worth the lines, and setting the
; variable through .NET also tells running programs the environment changed.
Function AddToUserPath
    nsExec::Exec 'powershell -NoProfile -ExecutionPolicy Bypass -Command "$$p=[Environment]::GetEnvironmentVariable(\"Path\",\"User\"); if(-not $$p){$$p=\"\"}; if((($$p -split \";\") -notcontains \"$INSTDIR\")){[Environment]::SetEnvironmentVariable(\"Path\",(($$p.TrimEnd(\";\")+\";$INSTDIR\").TrimStart(\";\")),\"User\")}"'
FunctionEnd

Function un.RemoveFromUserPath
    nsExec::Exec 'powershell -NoProfile -ExecutionPolicy Bypass -Command "$$p=[Environment]::GetEnvironmentVariable(\"Path\",\"User\"); if($$p){[Environment]::SetEnvironmentVariable(\"Path\",((($$p -split \";\") | Where-Object { $$_ -ne \"$INSTDIR\" -and $$_ -ne \"\" }) -join \";\"),\"User\")}"'
FunctionEnd

; Installer section
Section "Install"
    SetOutPath $INSTDIR

    ; Install main binary
    File "{{main_binary_path}}"

    ; Install resources
    {{#each staged_files}}
    SetOutPath "$INSTDIR{{#if this.target_dir}}\{{this.target_dir}}{{/if}}"
    File "{{this.source}}"
    {{/each}}

    SetOutPath $INSTDIR

    {{#if installer_icon}}
    ; The icon Settings > Apps shows for the uninstall entry
    File /oname=app.ico "{{installer_icon}}"
    {{/if}}

    ; Create uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Create Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\{{start_menu_folder}}"
    CreateShortcut "$SMPROGRAMS\{{start_menu_folder}}\${PRODUCT_DISPLAY}.lnk" "$INSTDIR\{{main_binary_name}}"
    CreateShortcut "$SMPROGRAMS\{{start_menu_folder}}\Uninstall ${PRODUCT_DISPLAY}.lnk" "$INSTDIR\uninstall.exe"

    ; Create Desktop shortcut
    CreateShortcut "$DESKTOP\${PRODUCT_DISPLAY}.lnk" "$INSTDIR\{{main_binary_name}}"

    ; Write registry keys for Add/Remove Programs
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "DisplayName" "yaiol ${PRODUCT_DISPLAY}"
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "DisplayVersion" "{{version}}"
    {{#if publisher}}
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "Publisher" "{{publisher}}"
    {{/if}}
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "InstallLocation" "$INSTDIR"
    {{#if installer_icon}}
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "DisplayIcon" "$INSTDIR\app.ico"
    {{/if}}

    ; Get installed size
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}" \
        "EstimatedSize" "$0"

    {{#if install_webview}}
    ; WebView2 installation
    {{webview_install_code}}
    {{/if}}

SectionEnd

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

; Uninstaller section
Section "Uninstall"
    ; Take the install folder off the user's PATH (a no-op when it was never added)
    Call un.RemoveFromUserPath

    ; Remove files
    RMDir /r "$INSTDIR"

    ; Remove this app's Start Menu shortcuts. ⚠ CLAUDE: NOT `RMDir /r` on the folder as dx's script
    ; does - the folder is "yaiol", shared by every yaiol app, so a recursive remove deleted the
    ; others' shortcuts. A plain RMDir only removes it once nothing else is left inside.
    Delete "$SMPROGRAMS\{{start_menu_folder}}\${PRODUCT_DISPLAY}.lnk"
    Delete "$SMPROGRAMS\{{start_menu_folder}}\Uninstall ${PRODUCT_DISPLAY}.lnk"
    RMDir "$SMPROGRAMS\{{start_menu_folder}}"

    ; Remove Desktop shortcut
    Delete "$DESKTOP\${PRODUCT_DISPLAY}.lnk"

    ; Remove registry keys
    DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{{bundle_id}}"
SectionEnd
