# Dexter: «Biblioteca de músicas» + «Ordem aleatória e reproduzir» no Reprodutor Multimédia (WinUI/UWP).

param([int]$StartupWaitMs = 0)



if ($StartupWaitMs -gt 0) {

    Start-Sleep -Milliseconds $StartupWaitMs

}



$ErrorActionPreference = 'Continue'



try {

    Add-Type -AssemblyName UIAutomationClient

    Add-Type -AssemblyName UIAutomationTypes

}

catch {

    exit 2

}



function Scroll-IntoView([System.Windows.Automation.AutomationElement]$el) {

    try {

        $sip = $el.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern)

        if ($null -ne $sip) { $sip.ScrollIntoView() }

    }

    catch {}

}



function Try-Activate([System.Windows.Automation.AutomationElement]$el) {

    if (-not $el) { return $false }



    Scroll-IntoView $el

    Start-Sleep -Milliseconds 120



    try {

        $lap = $el.GetCurrentPattern([System.Windows.Automation.LegacyIAccessiblePattern]::Pattern)

        if ($null -ne $lap) {

            $lap.DoDefaultAction()

            return $true

        }

    }

    catch {}



    try {

        $ip = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)

        if ($null -ne $ip) {

            $ip.Invoke()

            return $true

        }

    }

    catch {}



    try {

        $sp = $el.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)

        if ($null -ne $sp) {

            $sp.Select()

            return $true

        }

    }

    catch {}



    try {

        $tp = $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)

        if ($null -ne $tp) {

            $tp.Toggle()

            return $true

        }

    }

    catch {}



    try {

        $ec = $el.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)

        if ($null -ne $ec) {

            $state = $ec.Current.ExpandCollapseState

            if ($state -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed -or

                    $state.ToString() -eq 'Collapsed') {

                $ec.Expand()

                return $true

            }

        }

    }

    catch {}



    return $false

}



function Get-CombinedLabel([System.Windows.Automation.AutomationElement]$e) {

    $parts = New-Object System.Collections.Generic.List[string]

    try {

        $n = $e.Current.Name

        if ($n) { $parts.Add($n) }

    }

    catch {}

    try {

        $h = $e.Current.HelpText

        if ($h) { $parts.Add($h) }

    }

    catch {}

    try {

        $id = $e.Current.AutomationId

        if ($id) { $parts.Add($id) }

    }

    catch {}

    try {

        $lv = $e.Current.LocalizedControlType

        if ($lv) { $parts.Add($lv) }

    }

    catch {}

    return ($parts -join ' ').Trim()

}



function Get-TreeDepth([System.Windows.Automation.AutomationElement]$e) {

    $d = 0

    $cur = $e

    while ($null -ne $cur -and $d -lt 60) {

        try {

            $cur = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($cur)

            if ($null -eq $cur) { break }

            $d++

        }

        catch { break }

    }

    return $d

}



function Try-ActivateAncestors([System.Windows.Automation.AutomationElement]$leaf, [int]$maxDepth = 18) {

    if (-not $leaf) { return $false }

    $cur = $leaf

    for ($i = 0; $i -lt $maxDepth -and $null -ne $cur; $i++) {

        if (Try-Activate $cur) { return $true }

        try {

            $cur = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($cur)

        }

        catch { break }

    }

    return $false

}



function Find-MediaPlayerWindow {

    $root = [System.Windows.Automation.AutomationElement]::RootElement

    $winType = [System.Windows.Automation.ControlType]::Window

    $condWin = New-Object System.Windows.Automation.PropertyCondition(

        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,

        $winType)

    $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $condWin)



    foreach ($w in $windows) {

        try {

            $pid = $w.Current.ProcessId

            $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue

            if (-not $proc) { continue }

            $path = $proc.Path

            if ($path) {

                $pl = $path.ToLowerInvariant()

                if ($pl -like '*\windowsapps\*' -and (

                        $pl -like '*zunemusic*' -or $pl -like '*media.player*' -or

                        $pl -like '*groove*' -or $pl -like '*music.ui*')) {

                    return $w

                }

            }

        }

        catch {}

    }



    foreach ($w in $windows) {

        try {

            $n = $w.Current.Name

            if ([string]::IsNullOrWhiteSpace($n)) { continue }

            $nl = $n.ToLowerInvariant()

            if ($nl -match 'multim[eé]dia|media player|groove|microsoft media|reprodutor|zune') {

                return $w

            }

        }

        catch {}

    }



    return $null

}



function Get-UiaSearchRoot([System.Windows.Automation.AutomationElement]$win) {

    try {

        $cc = New-Object System.Windows.Automation.PropertyCondition(

            [System.Windows.Automation.AutomationElement]::ClassNameProperty,

            'Windows.UI.Core.CoreWindow')

        $core = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cc)

        if ($null -ne $core) { return $core }

    }

    catch {}

    return $win

}



function Get-AllDescendants([System.Windows.Automation.AutomationElement]$parent) {

    try {

        return $parent.FindAll([System.Windows.Automation.TreeScope]::Descendants,

            [System.Windows.Automation.Condition]::TrueCondition)

    }

    catch {

        return @()

    }

}



function Rank-Library([System.Windows.Automation.AutomationElement]$e) {

    try {

        $pn = $e.Current.ControlType.ProgrammaticName

        if ($pn -match 'ListItem') { return 0 }

        if ($pn -match 'TreeItem') { return 1 }

        if ($pn -match 'Button') { return 2 }

        if ($pn -match 'Hyperlink') { return 3 }

        if ($pn -match 'TabItem') { return 4 }

        return 9

    }

    catch { return 9 }

}



function Rank-Shuffle([System.Windows.Automation.AutomationElement]$e) {

    try {

        $pn = $e.Current.ControlType.ProgrammaticName

        if ($pn -match 'Button') { return 0 }

        if ($pn -match 'Hyperlink') { return 1 }

        if ($pn -match 'MenuItem') { return 2 }

        if ($pn -match 'ListItem') { return 3 }

        if ($pn -match 'Text') { return 4 }

        return 6

    }

    catch { return 6 }

}



function Try-LibrarySection([System.Windows.Automation.AutomationElement]$root) {

    $rx = '(?i)biblioteca\s+de\s+m[uú]sicas|biblioteca\s+de\s+musica|music\s+library|my\s+music'

    $hits = New-Object System.Collections.Generic.List[System.Windows.Automation.AutomationElement]

    foreach ($e in Get-AllDescendants $root) {

        try {

            $label = Get-CombinedLabel $e

            if ([string]::IsNullOrWhiteSpace($label)) { continue }

            if (-not [regex]::IsMatch($label, $rx)) { continue }

            $hits.Add($e)

        }

        catch {}

    }

    $sorted = $hits | Sort-Object @{ Expression = { Rank-Library $_ }; Ascending = $true }, @{ Expression = { Get-TreeDepth $_ }; Ascending = $false }

    foreach ($h in $sorted) {

        if (Try-Activate $h) {

            Start-Sleep -Milliseconds 1400

            return $true

        }

        if (Try-ActivateAncestors $h) {

            Start-Sleep -Milliseconds 1400

            return $true

        }

    }

    return $false

}



function Try-ShufflePlay([System.Windows.Automation.AutomationElement]$root) {

    $rx = '(?i)(?:ordem\s+aleatoria\s+e\s+reproduzir|ordem\s+aleat[oó]ria\s+e\s+reproduzir|ordem\s+aleat[oó]ria\b|shuffle\s*(and\s*)?play|aleat[oó]ria\s+e\s+reproduzir|random(ize)?\s+and\s+play|reprodu[cç][aã]o\s+aleat[oó]ria)'

    $hits = New-Object System.Collections.Generic.List[System.Windows.Automation.AutomationElement]

    foreach ($e in Get-AllDescendants $root) {

        try {

            $label = Get-CombinedLabel $e

            if ([string]::IsNullOrWhiteSpace($label)) { continue }

            if (-not [regex]::IsMatch($label, $rx)) { continue }

            $hits.Add($e)

        }

        catch {}

    }

    # Prefer botões/hiperligações; depois nós mais profundos (texto dentro do botão).

    $sorted = $hits | Sort-Object @{ Expression = { Rank-Shuffle $_ }; Ascending = $true }, @{ Expression = { Get-TreeDepth $_ }; Ascending = $false }

    foreach ($h in $sorted) {

        if (Try-Activate $h) { return $true }

        if (Try-ActivateAncestors $h) { return $true }

    }

    return $false

}



function Bring-ToFront([System.Windows.Automation.AutomationElement]$win) {

    try {

        $hwnd = $win.Current.NativeWindowHandle

        if ($hwnd -ne 0) {

            Add-Type @"

using System;

using System.Runtime.InteropServices;

public class DexterFg {

  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);

  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

}

"@

            [void][DexterFg]::ShowWindow([IntPtr]$hwnd, 9)

            Start-Sleep -Milliseconds 80

            [void][DexterFg]::SetForegroundWindow([IntPtr]$hwnd)

            Start-Sleep -Milliseconds 220

        }

    }

    catch {}

}



$win = Find-MediaPlayerWindow

if (-not $win) { exit 10 }



Bring-ToFront $win

$searchRoot = Get-UiaSearchRoot $win



if (-not (Try-LibrarySection $searchRoot)) {

    Try-LibrarySection $win | Out-Null

}



Start-Sleep -Milliseconds 600



if (Try-ShufflePlay $searchRoot) { exit 0 }

if (Try-ShufflePlay $win) { exit 0 }



exit 11

