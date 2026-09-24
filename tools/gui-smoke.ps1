$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$qa = Join-Path $projectRoot 'target/gui-smoke'
New-Item -ItemType Directory -Force -Path $qa | Out-Null
$config = Join-Path $qa 'settings.json'
[System.IO.File]::WriteAllText($config, '{"schema_version":1,"width":360,"reserve_space":false,"autostart":false}', [System.Text.UTF8Encoding]::new($false))
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class MHSmoke {
  public delegate bool Callback(IntPtr hwnd,IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback,IntPtr data);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hwnd,uint message,IntPtr wparam,IntPtr lparam);
  [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd,ref Point point);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
  [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] static extern bool GetCursorPos(out Point point);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hwnd,IntPtr after,int x,int y,int cx,int cy,uint flags);
  [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
  [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
  public static IntPtr Find(uint pid,string wanted) {
    IntPtr result=IntPtr.Zero;
    EnumWindows((h,d)=>{uint p;GetWindowThreadProcessId(h,out p);var s=new StringBuilder(256);GetWindowText(h,s,256);if(p==pid&&s.ToString()==wanted&&IsWindowVisible(h))result=h;return true;},IntPtr.Zero);return result;
  }
  public static void Click(IntPtr hwnd,int x,int y) {
    Point original;GetCursorPos(out original);var point=new Point{X=x,Y=y};
    if(!ClientToScreen(hwnd,ref point))throw new Exception("No client coordinates");
    SetWindowPos(hwnd,(IntPtr)(-1),0,0,0,0,3);
    SetForegroundWindow(hwnd);System.Threading.Thread.Sleep(200);
    if(WindowFromPoint(point)!=hwnd)throw new Exception("Target is occluded; refusing to click another window");
    if(!SetCursorPos(point.X,point.Y))throw new Exception("Desktop cursor not accessible");
    System.Threading.Thread.Sleep(150);mouse_event(2,0,0,0,UIntPtr.Zero);
    System.Threading.Thread.Sleep(100);mouse_event(4,0,0,0,UIntPtr.Zero);
    System.Threading.Thread.Sleep(150);SetCursorPos(original.X,original.Y);
  }
}
'@
$exe = Join-Path $projectRoot 'target/debug/MH-Sidebar.exe'
$process = Start-Process -FilePath $exe -ArgumentList "--config `"$config`" --settings --smoke-test 35" -WindowStyle Hidden -PassThru
try {
    $settings = [IntPtr]::Zero
    for ($attempt=0; $attempt -lt 40 -and $settings -eq [IntPtr]::Zero; $attempt++) {
        Start-Sleep -Milliseconds 150
        $settings = [MHSmoke]::Find($process.Id,'MH Sidebar — настройки')
    }
    if ($settings -eq [IntPtr]::Zero) { throw 'Settings window did not open' }
    Start-Sleep -Milliseconds 800
    [MHSmoke]::Click($settings,100,744)
    Start-Sleep -Milliseconds 250
    [MHSmoke]::Click($settings,1170,808)
    Start-Sleep -Milliseconds 1400
    if ((Get-Content -Raw -LiteralPath $config | ConvertFrom-Json).width -ne 360) { throw 'Cancel changed the saved settings' }
    if ([MHSmoke]::IsWindowVisible($settings)) { throw 'Cancel failed to close settings' }
    $sidebar = [MHSmoke]::Find($process.Id,'MH Sidebar')
    if ($sidebar -eq [IntPtr]::Zero) { throw 'Sidebar missing after Cancel' }
    [MHSmoke]::Click($sidebar,286,33)
    Start-Sleep -Milliseconds 1400
    $settings = [MHSmoke]::Find($process.Id,'MH Sidebar — настройки')
    if ($settings -eq [IntPtr]::Zero) { throw 'Sidebar settings button did not reopen settings' }
    [MHSmoke]::Click($settings,100,744)
    Start-Sleep -Milliseconds 200
    [MHSmoke]::Click($settings,1018,808)
    Start-Sleep -Milliseconds 1400
    if ((Get-Content -Raw -LiteralPath $config | ConvertFrom-Json).width -ne 280) { throw 'Apply failed to persist the draft' }
    [MHSmoke]::Click($settings,1170,808)
    Start-Sleep -Milliseconds 1200
    [MHSmoke]::Click($sidebar,246,33)
    Start-Sleep -Milliseconds 1400
    if ([MHSmoke]::IsWindowVisible($sidebar)) { throw 'Hide failed' }
    Write-Output 'GUI smoke passed: settings open, Reset/Cancel, reopen, Reset/Apply, hide.'
} finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id }
}

