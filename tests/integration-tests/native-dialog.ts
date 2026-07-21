import { execFile } from 'node:child_process'

const SAVE_PROJECT_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
$projectPath = [Text.Encoding]::UTF8.GetString(
  [Convert]::FromBase64String('__PROJECT_PATH__')
)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class DialogMessages
{
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)]
    public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wParam, string text);

    [DllImport("user32.dll", EntryPoint = "SendMessageW")]
    public static extern IntPtr Click(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
'@

$root = [System.Windows.Automation.AutomationElement]::RootElement
$deadline = [DateTime]::UtcNow.AddSeconds(10)
$dialog = $null

while ($null -eq $dialog -and [DateTime]::UtcNow -lt $deadline) {
  $processes = @(Get-Process -Name 'koharu' -ErrorAction SilentlyContinue)
  if ($processes.Count -gt 1) {
    throw 'Expected one Koharu process, but found multiple processes.'
  }

  if ($processes.Count -eq 1) {
    $processCondition = New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
      $processes[0].Id
    )
    $nameCondition = New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::NameProperty,
      'Save As'
    )
    $dialogCondition = New-Object System.Windows.Automation.AndCondition(
      $processCondition,
      $nameCondition
    )
    $dialog = $root.FindFirst(
      [System.Windows.Automation.TreeScope]::Children,
      $dialogCondition
    )
  }

  if ($null -eq $dialog) {
    Start-Sleep -Milliseconds 100
  }
}

if ($null -eq $dialog) {
  throw 'Koharu Save As dialog was not found.'
}

$fileNameCondition = New-Object System.Windows.Automation.AndCondition(
  (New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
    '1001'
  )),
  (New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ClassNameProperty,
    'Edit'
  ))
)
$fileName = $dialog.FindFirst(
  [System.Windows.Automation.TreeScope]::Descendants,
  $fileNameCondition
)
if ($null -eq $fileName -or $fileName.Current.NativeWindowHandle -eq 0) {
  throw 'Koharu Save As file name field was not found.'
}

[void][DialogMessages]::SetText(
  [IntPtr]$fileName.Current.NativeWindowHandle,
  0x000C,
  [IntPtr]::Zero,
  $projectPath
)

$saveCondition = New-Object System.Windows.Automation.AndCondition(
  (New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
    '1'
  )),
  (New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ClassNameProperty,
    'Button'
  ))
)
$save = $dialog.FindFirst(
  [System.Windows.Automation.TreeScope]::Descendants,
  $saveCondition
)
if ($null -eq $save -or $save.Current.NativeWindowHandle -eq 0) {
  throw 'Koharu Save As button was not found.'
}

[void][DialogMessages]::Click(
  [IntPtr]$save.Current.NativeWindowHandle,
  0x00F5,
  [IntPtr]::Zero,
  [IntPtr]::Zero
)

$deadline = [DateTime]::UtcNow.AddSeconds(10)
while (-not (Test-Path -LiteralPath $projectPath -PathType Leaf) -and
       [DateTime]::UtcNow -lt $deadline) {
  Start-Sleep -Milliseconds 100
}
if (-not (Test-Path -LiteralPath $projectPath -PathType Leaf)) {
  throw "Koharu did not create the project at $projectPath."
}
`

export function saveProjectAs(projectPath: string): Promise<void> {
  if (process.platform !== 'win32') {
    throw new Error('Koharu native dialog automation requires Windows')
  }

  const encodedPath = Buffer.from(projectPath, 'utf8').toString('base64')
  const script = SAVE_PROJECT_SCRIPT.replace('__PROJECT_PATH__', encodedPath)
  const encodedScript = Buffer.from(script, 'utf16le').toString('base64')

  return new Promise((resolve, reject) => {
    execFile(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-EncodedCommand', encodedScript],
      { timeout: 25_000, windowsHide: true },
      (error, stdout, stderr) => {
        if (!error) {
          resolve()
          return
        }

        reject(
          new Error(
            ['Native Save As automation failed.', stderr.trim(), stdout.trim()]
              .filter(Boolean)
              .join(' '),
            { cause: error },
          ),
        )
      },
    )
  })
}
