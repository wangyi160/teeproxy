param([string]$Binary = "$PSScriptRoot/../target/release/teeproxy.exe")
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TeeproxyUi {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr value);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int capacity);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr hwnd, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int capacity);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr ReadText(IntPtr hwnd, uint message, IntPtr capacity, StringBuilder text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SetText(IntPtr hwnd, uint message, IntPtr wp, string text);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr Send(IntPtr hwnd, uint message, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wp, IntPtr lp);
    public static IntPtr Find(uint pid) {
        IntPtr result=IntPtr.Zero;
        EnumWindows((hwnd, value) => {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            var name=new StringBuilder(100); GetClassName(hwnd,name,100);
            if(owner==pid && name.ToString()=="teeproxy.NativeWindow") { result=hwnd; return false; }
            return true;
        },IntPtr.Zero);
        return result;
    }
}
'@
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$testDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) ('target/windows-ui-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'desktop.json'
$policyFile = Join-Path $testDirectory 'synthetic-test-only.json'
# Structurally valid but synthetic measurements; this never approves a real router.
$policy = @{schema_version=1; releases=@(@{
    id='UI TEST ONLY'; mr_td=('01'*48); rtmrs=@(('02'*48),('03'*48),('04'*48));
    events=@(@{event='compose-hash';payload=('06'*32)},@{event='boot-mr-done';payload=''},@{event='key-provider';payload='74657374'})
})}
[IO.File]::WriteAllText($policyFile, ($policy | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
$portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
$portProbe.Start()
$testPort = $portProbe.LocalEndpoint.Port
$portProbe.Stop()
function Wait-Until([scriptblock]$Condition) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Timed out waiting for desktop state'
}
function Read-Control([int]$Id) {
    $text = [Text.StringBuilder]::new(32768)
    [void][TeeproxyUi]::ReadText([TeeproxyUi]::GetDlgItem($script:window,$Id),0x000D,[IntPtr]32768,$text)
    $text.ToString()
}
function Set-Control([int]$Id, [string]$Value) {
    [void][TeeproxyUi]::SetText([TeeproxyUi]::GetDlgItem($script:window,$Id),0x000C,[IntPtr]::Zero,$Value)
}
function Click-Control([int]$Id) {
    [void][TeeproxyUi]::Send($script:window,0x0111,[IntPtr]$Id,[TeeproxyUi]::GetDlgItem($script:window,$Id))
}
function Open-TestWindow {
    $script:appProcess = Start-Process -FilePath $Binary -ArgumentList @('gui','--settings',('"' + $settingsFile + '"')) -WindowStyle Hidden -PassThru
    Wait-Until { $script:window=[TeeproxyUi]::Find($script:appProcess.Id); $script:window -ne [IntPtr]::Zero -and [TeeproxyUi]::GetDlgItem($script:window,203) -ne [IntPtr]::Zero }
}
function Close-TestWindow {
    if ($script:appProcess -and -not $script:appProcess.HasExited) {
        [void][TeeproxyUi]::PostMessage($script:window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
        if (-not $script:appProcess.WaitForExit(5000)) { throw 'Desktop failed to close' }
    }
}
try {
    Open-TestWindow
    Set-Control 101 'https://127.0.0.1:1'
    Set-Control 102 'desktop-smoke-key-not-a-real-credential'
    Set-Control 103 $testPort
    Set-Control 105 $policyFile
    Click-Control 201
    Wait-Until { Test-Path -LiteralPath $settingsFile }
    $saved = [IO.File]::ReadAllText($settingsFile)
    if ($saved.Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key persisted in plaintext' }
    Close-TestWindow
    Open-TestWindow
    if ((Read-Control 101) -ne 'https://127.0.0.1:1' -or (Read-Control 103) -ne "$testPort") { throw 'Saved address/port did not restore' }
    # Password controls are masked, including after reopening.
    if ([TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,102),0x00D2,[IntPtr]::Zero,[IntPtr]::Zero) -eq [IntPtr]::Zero) { throw 'Key control is not masked' }
    Click-Control 202
    Wait-Until { [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,203)) }
    if (-not (Read-Control 110).Contains('failed')) { throw 'Verification failure was not displayed' }
    if ((Read-Control 110).Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key leaked to operation log' }
    Click-Control 203
    Wait-Until { (Read-Control 110).Contains("http://127.0.0.1:$testPort/v1") }
    if ([TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,101))) { throw 'Upstream editable while running' }
    Add-Type -AssemblyName System.Net.Http
    $handler = [Net.Http.HttpClientHandler]::new(); $handler.UseProxy=$false
    $client = [Net.Http.HttpClient]::new($handler)
    try {
        $response = $client.GetAsync("http://127.0.0.1:$testPort/v1/models").GetAwaiter().GetResult()
        if ([int]$response.StatusCode -ne 502) { throw 'Unverified upstream accepted' }
        $response.Dispose()
    } finally { $client.Dispose(); $handler.Dispose() }
    Click-Control 204
    Wait-Until { [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,203)) }
    $portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$testPort)
    $portProbe.Start(); $portProbe.Stop()
    if ((Read-Control 110).Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key leaked to operation log' }
    Close-TestWindow
    'Native desktop smoke passed: save/reopen, masked key, verification failure, start/stop, closed listener'
} finally { Close-TestWindow }
