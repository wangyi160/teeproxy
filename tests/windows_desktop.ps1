param([string]$Binary = "$PSScriptRoot/../target/release/teeproxy.exe", [switch]$Portable)
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
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetStyle(IntPtr hwnd, int index);
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
$previousLocalAppData = $env:LOCALAPPDATA
if ($Portable) {
    $applicationDirectory = Join-Path $testDirectory 'application'
    New-Item -ItemType Directory -Path $applicationDirectory | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $applicationDirectory 'teeproxy.exe')
    $Binary = Join-Path $applicationDirectory 'teeproxy.exe'
    $settingsFile = Join-Path $applicationDirectory 'data/desktop.json'
    $env:LOCALAPPDATA = Join-Path $testDirectory 'unused-user-directory'
}
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
function Select-Page([int]$Page) {
    Click-Control (501 + $Page)
}
function Control-Shown([int]$Id) {
    ([TeeproxyUi]::GetStyle([TeeproxyUi]::GetDlgItem($script:window,$Id),-16).ToInt64() -band 0x10000000) -ne 0
}
function Open-TestWindow {
    if ($Portable) {
        $script:appProcess = Start-Process -FilePath $Binary -WorkingDirectory $testDirectory -WindowStyle Hidden -PassThru
    } else {
        $script:appProcess = Start-Process -FilePath $Binary -ArgumentList @('gui','--settings',('"' + $settingsFile + '"')) -WindowStyle Hidden -PassThru
    }
    Wait-Until { $script:window=[TeeproxyUi]::Find($script:appProcess.Id); $script:window -ne [IntPtr]::Zero -and [TeeproxyUi]::GetDlgItem($script:window,707) -ne [IntPtr]::Zero -and (Read-Control 110).Length -gt 0 }
}
function Close-TestWindow {
    if ($script:appProcess -and -not $script:appProcess.HasExited) {
        [void][TeeproxyUi]::PostMessage($script:window,0x0111,[IntPtr]403,[IntPtr]::Zero)
        if (-not $script:appProcess.WaitForExit(5000)) { throw 'Desktop failed to close' }
    }
}
try {
    Open-TestWindow
    foreach ($id in @(501,502,503,504,505,506,507,508,509,510,511,512,513,559,560,1511,1512,1513,1514,1515,1516,1522,1543,302,303,304,307,310,312,313,314,315,316,601,602,605,606,607,609,701,702,703,704,705,706,707)) {
        if ([TeeproxyUi]::GetDlgItem($script:window,$id) -eq [IntPtr]::Zero) { throw "Missing native inventory control $id" }
    }
    foreach ($id in @(104,309,311,514,603,604,608,1510,1517,1536)) {
        if ([TeeproxyUi]::GetDlgItem($script:window,$id) -ne [IntPtr]::Zero) { throw "Removed routing control $id still exists" }
    }
    if (-not [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,605))) { throw 'Fixed pool maximum is disabled' }
    Select-Page 6
    if (-not (Control-Shown 1512) -or -not (Control-Shown 1513) -or (Control-Shown 1514) -or (Control-Shown 1522) -or (Control-Shown 701)) { throw 'Native login form visibility failed' }
    if (([TeeproxyUi]::GetStyle([TeeproxyUi]::GetDlgItem($script:window,1513),-16).ToInt64() -band 0x20) -eq 0) { throw 'Account password is not masked' }
    Set-Control 1513 'temporary-draft-password'
    Click-Control 1516
    if (-not (Control-Shown 1514) -or (Read-Control 1513)) { throw 'Register switch did not clear password or reveal name' }
    Set-Control 1512 'test-native'
    Set-Control 1513 'short'
    Click-Control 1515
    if ((Read-Control 1524).Length -eq 0 -or (Control-Shown 1522)) { throw 'Native register validation failed' }
    Click-Control 1516
    if ((Control-Shown 1514) -or (Read-Control 1513)) { throw 'Login switch did not hide registration fields or clear password' }
    $identityTitle = Read-Control 559
    $accountTitles = [Collections.Generic.HashSet[string]]::new()
    [void]$accountTitles.Add($identityTitle)
    foreach ($page in 7..12) {
        Select-Page $page
        if (-not $accountTitles.Add((Read-Control 559))) { throw 'Account sidebar reused another page title' }
        if (-not (Control-Shown 1543) -or (Control-Shown 1512) -or (Control-Shown 1522)) { throw 'Logged-out account page displayed private data or the identity form' }
        Click-Control 1543
        if ((Read-Control 559) -ne $identityTitle -or -not (Control-Shown 1512)) { throw 'Account page sign-in link did not navigate to identity' }
    }
    $accountListeners = @(Get-NetTCPConnection -State Listen -OwningProcess $script:appProcess.Id -ErrorAction SilentlyContinue)
    if ($accountListeners.Count) { throw 'Native account UI unexpectedly opened a local HTTP listener' }
    Select-Page 5
    if (-not (Control-Shown 701) -or -not (Control-Shown 702) -or -not (Control-Shown 704) -or (Control-Shown 302) -or (Control-Shown 312)) { throw 'Official pricing page visibility failed' }
    Select-Page 1
    if (-not (Control-Shown 302) -or -not (Control-Shown 310) -or -not (Control-Shown 303) -or -not (Control-Shown 312) -or -not (Control-Shown 314) -or -not (Control-Shown 316) -or (Control-Shown 101)) { throw 'Linked inventory page visibility failed' }
    Select-Page 3
    if (-not (Control-Shown 110) -or (Control-Shown 303)) { throw 'Log tab visibility failed' }
    Select-Page 4
    if (-not (Control-Shown 601) -or -not (Control-Shown 602) -or (Control-Shown 303)) { throw 'Routing tab visibility failed' }
    if ([TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,606))) { throw 'Automatic fields enabled in fixed-pool mode' }
    Select-Page 2
    Set-Control 101 'https://127.0.0.1:1'
    Set-Control 102 'desktop-smoke-key-not-a-real-credential'
    Set-Control 103 $testPort
    if ((Read-Control 111) -ne '127.0.0.1') { throw 'Default listener is not loopback' }
    Set-Control 111 '0.0.0.0'
    [void][TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,601),0x014E,[IntPtr]1,[IntPtr]::Zero)
    [void][TeeproxyUi]::Send($script:window,0x0111,[IntPtr](601 -bor (1 -shl 16)),[TeeproxyUi]::GetDlgItem($script:window,601))
    Set-Control 105 $policyFile
    Click-Control 201
    if (Test-Path -LiteralPath $settingsFile) { throw 'Wildcard settings saved without client authentication' }
    $saveFailure = -join [char[]]@(0x4fdd,0x5b58,0x5931,0x8d25)
    if (-not (Read-Control 109).StartsWith($saveFailure) -or -not (Read-Control 109).Contains('API key')) { throw 'Save did not show the missing API key reason on the current page' }
    if ((Read-Control 111) -ne '0.0.0.0' -or (Read-Control 103) -ne "$testPort") { throw 'Failed save discarded the edited address' }
    Set-Control 107 'short-local-key'
    Click-Control 201
    if (Test-Path -LiteralPath $settingsFile) { throw 'Settings saved with a short key' }
    if (-not (Read-Control 109).StartsWith($saveFailure) -or -not (Read-Control 109).Contains('16')) { throw 'Save did not display the key length requirement' }
    if ((Read-Control 109).Contains('short-local-key') -or (Read-Control 110).Contains('short-local-key')) { throw 'Invalid local key leaked in the error message' }
    Set-Control 107 'desktop-local-client-key-for-test'
    Click-Control 201
    Wait-Until { Test-Path -LiteralPath $settingsFile }
    if (-not (Read-Control 109).StartsWith((-join [char[]]@(0x914d,0x7f6e,0x5df2,0x4fdd,0x5b58)))) { throw 'Successful save did not clear the earlier failure' }
    $saved = [IO.File]::ReadAllText($settingsFile)
    if ($saved.Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key persisted in plaintext' }
    if ($saved.Contains('desktop-local-client-key-for-test')) { throw 'Local key persisted in plaintext' }
    Close-TestWindow
    if ($Portable) {
        $portablePolicy = ($saved | ConvertFrom-Json).profile.tdx_policy
        if ([IO.Path]::IsPathRooted($portablePolicy) -or -not $portablePolicy.StartsWith('policies\')) { throw 'Policy did not become a portable relative path' }
        $copy = Join-Path (Split-Path $settingsFile -Parent) $portablePolicy
        if ((Get-FileHash -LiteralPath $copy).Hash -ne (Get-FileHash -LiteralPath $policyFile).Hash) { throw 'Policy copy changed approved contents' }
        $movedDirectory = Join-Path $testDirectory 'moved-application'
        $allowedRoot = [IO.Path]::GetFullPath($testDirectory).TrimEnd('\') + '\'
        foreach ($target in @($applicationDirectory,$movedDirectory,$policyFile,($policyFile + '.unavailable'))) {
            if (-not [IO.Path]::GetFullPath($target).StartsWith($allowedRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'Move escaped the test workspace' }
        }
        if (Test-Path -LiteralPath $movedDirectory) { throw 'Move target already exists' }
        Move-Item -LiteralPath $applicationDirectory -Destination $movedDirectory
        Move-Item -LiteralPath $policyFile -Destination ($policyFile + '.unavailable')
        $Binary = Join-Path $movedDirectory 'teeproxy.exe'
        $settingsFile = Join-Path $movedDirectory 'data/desktop.json'
        # Windows shell APIs may create Microsoft/Windows/Caches themselves.
        if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'teeproxy')) { throw 'Portable app wrote its profile to AppData' }
    }
    Open-TestWindow
    if ((Read-Control 101) -ne 'https://127.0.0.1:1' -or (Read-Control 103) -ne "$testPort") { throw 'Saved address/port did not restore' }
    if ((Read-Control 111) -ne '0.0.0.0') { throw 'Saved bind IP did not restore' }
    # The Agent API key is restored visibly; the Router business key stays masked.
    if ((Read-Control 107) -ne 'desktop-local-client-key-for-test') { throw 'Saved API key did not restore visibly' }
    if ([TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,107),0x00D2,[IntPtr]::Zero,[IntPtr]::Zero) -ne [IntPtr]::Zero) { throw 'Agent API key is masked after reopening' }
    if ([TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,102),0x00D2,[IntPtr]::Zero,[IntPtr]::Zero) -eq [IntPtr]::Zero) { throw 'Key control is not masked' }
    foreach ($checked in @(1,0)) {
        [void][TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,208),0x00F1,[IntPtr]$checked,[IntPtr]::Zero)
        Click-Control 208
        if ([TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,107),0x00D2,[IntPtr]::Zero,[IntPtr]::Zero) -ne [IntPtr]::Zero) { throw 'Router key toggle hid the Agent API key' }
    }
    Click-Control 202
    Wait-Until { [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,202)) }
    if (-not (Read-Control 110).Contains('failed')) { throw 'Verification failure was not displayed' }
    if ((Read-Control 110).Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key leaked to operation log' }
    Click-Control 307
    Wait-Until { [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,307)) }
    if ([TeeproxyUi]::Send([TeeproxyUi]::GetDlgItem($script:window,302),0x1004,[IntPtr]::Zero,[IntPtr]::Zero) -ne [IntPtr]::Zero) { throw 'Unverified inventory displayed' }
    Click-Control 203
    Wait-Until { (Read-Control 110).Contains("http://127.0.0.1:$testPort/v1") }
    if ([TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,101))) { throw 'Upstream editable while running' }
    Select-Page 4
    foreach ($id in @(601,605,607)) {
        if (-not [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,$id))) { throw "Routing control $id disabled while running" }
    }
    Set-Control 605 '0.5'
    if (-not (Read-Control 609).StartsWith((-join [char[]]@(0x672a,0x4fdd,0x5b58)))) { throw 'Unsaved rule was not identified' }
    Click-Control 607
    Wait-Until { (Read-Control 609).StartsWith((-join [char[]]@(0x5df2,0x751f,0x6548))) }
    $liveSaved = [IO.File]::ReadAllText($settingsFile) | ConvertFrom-Json
    if ($liveSaved.profile.routing.max_multiplier_millionths -ne 500000) { throw 'Live rule not saved' }
    if ((Read-Control 203) -ne (-join [char[]]@(0x505c,0x6b62,0x4ee3,0x7406))) { throw 'Saving routing stopped the proxy' }
    if (-not (Read-Control 109).StartsWith((-join [char[]]@(0x914d,0x7f6e,0x5df2,0x4fdd,0x5b58)))) { throw 'Live route save did not show confirmation' }
    # Exercise the actual notification-area callback, minimize, and close behavior.
    [void][TeeproxyUi]::Send($script:window,0x8011,[IntPtr]::Zero,[IntPtr]0x0400)
    Wait-Until { [TeeproxyUi]::IsWindowVisible($script:window) }
    [void][TeeproxyUi]::Send($script:window,0x0112,[IntPtr]0xF020,[IntPtr]::Zero)
    Wait-Until { -not [TeeproxyUi]::IsWindowVisible($script:window) }
    if ($script:appProcess.HasExited) { throw 'Minimizing exited the gateway' }
    Click-Control 401
    Wait-Until { [TeeproxyUi]::IsWindowVisible($script:window) }
    [void][TeeproxyUi]::Send($script:window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Wait-Until { -not [TeeproxyUi]::IsWindowVisible($script:window) }
    if ($script:appProcess.HasExited) { throw 'Closing the window exited the gateway' }
    Add-Type -AssemblyName System.Net.Http
    $handler = [Net.Http.HttpClientHandler]::new(); $handler.UseProxy=$false
    $client = [Net.Http.HttpClient]::new($handler)
    try {
        $response = $client.GetAsync("http://127.0.0.1:$testPort/v1/models").GetAwaiter().GetResult()
        if ([int]$response.StatusCode -ne 401) { throw 'Wildcard listener accepted missing client key' }
        $response.Dispose()
        $client.DefaultRequestHeaders.Authorization = [Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer','desktop-local-client-key-for-test')
        $response = $client.GetAsync("http://127.0.0.1:$testPort/v1/models").GetAwaiter().GetResult()
        if ([int]$response.StatusCode -ne 502) { throw 'Unverified upstream accepted' }
        $response.Dispose()
        $localNetworkIP = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.AddressState -eq 'Preferred' -and $_.IPAddress -notmatch '^(127\.|169\.254\.|198\.18\.)' } | Select-Object -First 1 -ExpandProperty IPAddress
        if ($localNetworkIP) {
            $response = $client.GetAsync("http://${localNetworkIP}:$testPort/v1/models").GetAwaiter().GetResult()
            if ([int]$response.StatusCode -ne 502) { throw 'Authenticated network client did not reach the TEE check' }
            $response.Dispose()
            $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get,"http://${localNetworkIP}:$testPort/v1/models")
            $request.Headers.Host = "localhost:$testPort"
            $response = $client.SendAsync($request).GetAwaiter().GetResult()
            if ([int]$response.StatusCode -ne 400) { throw 'Network listener accepted a mismatched localhost Host' }
            $response.Dispose();$request.Dispose()
        }
    } finally { $client.Dispose(); $handler.Dispose() }
    Click-Control 203
    Wait-Until { [TeeproxyUi]::IsWindowEnabled([TeeproxyUi]::GetDlgItem($script:window,202)) }
    $portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$testPort)
    $portProbe.Start(); $portProbe.Stop()
    if ((Read-Control 110).Contains('desktop-smoke-key-not-a-real-credential')) { throw 'Key leaked to operation log' }
    Close-TestWindow
    Open-TestWindow
    Click-Control 203
    Wait-Until { (Read-Control 110).Contains("http://127.0.0.1:$testPort/v1") }
    Close-TestWindow
    $portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$testPort)
    $portProbe.Start(); $portProbe.Stop()
    'Native desktop smoke passed: sidebar pages, save/reopen, live routing edits, masked key, inventory/verification failure, tray restore/minimize/close, background API, stop/exit listener cleanup'
    if ($Portable) { 'Portable desktop passed: executable-relative data, external policy import, folder move, encrypted credentials restored, no AppData writes' }
} finally {
    try { Close-TestWindow } finally { $env:LOCALAPPDATA = $previousLocalAppData }
}
